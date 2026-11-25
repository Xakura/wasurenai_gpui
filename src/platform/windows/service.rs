//! Windows 托盘图标 + 全局热键专用消息泵线程。
//!
//! 为什么必须单开一个线程：`tray-icon` 内部是 `Rc`（非 `Send`），并且要求在**创建它
//! 的线程上有一个 Win32 消息泵**；`global-hotkey` 的 `RegisterHotKey` 也要求同一个
//! 消息泵。所以托盘和热键都放在这里。
//!
//! 两个方向：
//! - 平台 → UI：`async_channel<PlatformEvent>`（托盘点击、热键触发、热键注册结果）
//! - UI → 平台：`async_channel<PlatformCommand>`，用 `PostThreadMessageW(WM_APP)` 唤醒泵

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use async_channel::Sender;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_APP, WM_QUIT,
};

use crate::core::hotkey::HotkeySpec;
use crate::platform::{PlatformCommand, PlatformEvent, icon};

/// 唤醒消息泵用的自定义消息。
const WM_APP_COMMAND: u32 = WM_APP + 1;

/// 当前注册的热键 id（热键事件回调里用来比对）。
static CURRENT_ID: AtomicU32 = AtomicU32::new(0);

/// 平台线程 id，供 UI → 平台方向唤醒泵。
static PUMP_THREAD: OnceLock<u32> = OnceLock::new();

/// 命令队列（创建一次，供任意线程投递）。
static COMMAND_QUEUE: OnceLock<Sender<PlatformCommand>> = OnceLock::new();

/// 给平台线程发命令。返回是否成功唤醒。
pub fn send_command(cmd: PlatformCommand) -> bool {
    let Some(thread_id) = PUMP_THREAD.get() else {
        eprintln!("[wasurenai] 平台线程还没就绪，命令被丢弃");
        return false;
    };
    if let Some(queue) = COMMAND_QUEUE.get() {
        let _ = queue.send_blocking(cmd);
    }
    unsafe { PostThreadMessageW(*thread_id, WM_APP_COMMAND, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 启动平台线程。`initial_hotkey` / `initial_pending` / `initial_overdue` 用于首帧初始化托盘与热键。
pub fn spawn(
    tx: Sender<PlatformEvent>,
    initial_hotkey: HotkeySpec,
    initial_pending: usize,
    initial_overdue: usize,
) {
    let (command_tx, command_rx) = async_channel::unbounded();
    let _ = COMMAND_QUEUE.set(command_tx);

    std::thread::Builder::new()
        .name("wasurenai-platform".into())
        .spawn(move || unsafe {
            let tray_bundle = match create_tray(tx.clone(), initial_pending, initial_overdue) {
                Ok(bundle) => Some(bundle),
                Err(err) => {
                    eprintln!("[wasurenai] 托盘创建失败：{err:#}");
                    None
                }
            };
            let mut last_icon_state = (initial_pending > 0, initial_overdue > 0);

            let mut hotkeys = Hotkeys::new(tx.clone());
            hotkeys.apply(initial_hotkey);

            let _ = PUMP_THREAD.set(get_current_thread_id());

            let mut msg = MSG::default();
            loop {
                let got = GetMessageW(&mut msg, None, 0, 0);
                if !got.as_bool() || msg.message == WM_QUIT {
                    break;
                }
                if msg.message == WM_APP_COMMAND {
                    while let Ok(command) = command_rx.try_recv() {
                        match command {
                            PlatformCommand::SetHotkey(spec) => hotkeys.apply(spec),
                            PlatformCommand::PauseHotkey => hotkeys.pause(),
                            PlatformCommand::ResumeHotkey => hotkeys.resume(),
                            PlatformCommand::UpdateTray {
                                pending,
                                overdue,
                                widget_visible,
                            } => {
                                if let Some((ref tray, ref toggle_item)) = tray_bundle {
                                    let tip = format_tooltip(pending, overdue);
                                    let _ = tray.set_tooltip(Some(&tip));
                                    toggle_item.set_text(toggle_label(widget_visible));
                                    let next_icon_state = (pending > 0, overdue > 0);
                                    if next_icon_state != last_icon_state {
                                        let _ = tray
                                            .set_icon(Some(icon::tray_icon(pending, overdue)));
                                        last_icon_state = next_icon_state;
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn platform thread");
}

fn get_current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

/// 全局热键的注册/换绑。
struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    current: Option<HotKey>,
    saved_spec: Option<HotkeySpec>,
    tx: Sender<PlatformEvent>,
}

impl Hotkeys {
    fn new(tx: Sender<PlatformEvent>) -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(err) => {
                eprintln!("[wasurenai] 热键管理器创建失败：{err}");
                None
            }
        };

        let tx_hotkey = tx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            if event.id == CURRENT_ID.load(Ordering::Relaxed) {
                let _ = tx_hotkey.send_blocking(PlatformEvent::ToggleFloat);
            }
        }));

        Self {
            manager,
            current: None,
            saved_spec: None,
            tx,
        }
    }

    fn pause(&mut self) {
        if let (Some(manager), Some(old)) = (&self.manager, self.current.take()) {
            let _ = manager.unregister(old);
        }
        CURRENT_ID.store(0, Ordering::Relaxed);
    }

    fn resume(&mut self) {
        if self.current.is_some() {
            return;
        }
        if let Some(spec) = self.saved_spec.clone() {
            self.apply(spec);
        }
    }

    fn apply(&mut self, spec: HotkeySpec) {
        self.saved_spec = Some(spec.clone());
        let Some(manager) = &self.manager else {
            self.report(spec, Err("热键管理器不可用".into()));
            return;
        };
        if let Some(old) = self.current.take() {
            let _ = manager.unregister(old);
        }

        let Some(hotkey) = to_hotkey(&spec) else {
            let message = format!("「{}」不是系统能识别的组合键", spec.format());
            eprintln!("[wasurenai] 热键注册失败：{message}");
            CURRENT_ID.store(0, Ordering::Relaxed);
            self.report(spec, Err(message));
            return;
        };

        match manager.register(hotkey) {
            Ok(()) => {
                CURRENT_ID.store(hotkey.id(), Ordering::Relaxed);
                self.current = Some(hotkey);
                println!("[wasurenai] 热键已注册 {}", spec.format());
                self.report(spec, Ok(()));
            }
            Err(err) => {
                CURRENT_ID.store(0, Ordering::Relaxed);
                let message = format!("注册失败（可能被别的程序占用）：{err}");
                eprintln!("[wasurenai] 热键{message}");
                self.report(spec, Err(message));
            }
        }
    }

    fn report(&self, spec: HotkeySpec, result: Result<(), String>) {
        let _ = self.tx.send_blocking(PlatformEvent::HotkeyChanged {
            spec: spec.format(),
            error: result.err(),
        });
    }
}

fn to_hotkey(spec: &HotkeySpec) -> Option<HotKey> {
    let mut modifiers = Modifiers::empty();
    if spec.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if spec.alt {
        modifiers |= Modifiers::ALT;
    }
    if spec.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if spec.win {
        #[allow(deprecated)]
        {
            modifiers |= Modifiers::SUPER;
        }
    }
    Some(HotKey::new(Some(modifiers), key_code(&spec.key)?))
}

fn key_code(key: &str) -> Option<Code> {
    let code = match key {
        "Space" => Code::Space,
        "Enter" => Code::Enter,
        "Tab" => Code::Tab,
        "Up" => Code::ArrowUp,
        "Down" => Code::ArrowDown,
        "Left" => Code::ArrowLeft,
        "Right" => Code::ArrowRight,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "C" => Code::KeyC,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "H" => Code::KeyH,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "U" => Code::KeyU,
        "V" => Code::KeyV,
        "W" => Code::KeyW,
        "X" => Code::KeyX,
        "Y" => Code::KeyY,
        "Z" => Code::KeyZ,
        _ => return None,
    };
    Some(code)
}

fn create_tray(
    tx: Sender<PlatformEvent>,
    initial_pending: usize,
    initial_overdue: usize,
) -> anyhow::Result<(tray_icon::TrayIcon, MenuItem)> {
    let menu = Menu::new();
    let edit = MenuItem::new("编辑待办", true, None);
    let settings = MenuItem::new("设置", true, None);
    let toggle = MenuItem::new(toggle_label(true), true, None);
    let quit = MenuItem::new("退出", true, None);
    menu.append_items(&[
        &edit,
        &settings,
        &toggle,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;
    let edit_id = edit.id().clone();
    let settings_id = settings.id().clone();
    let toggle_id = toggle.id().clone();
    let quit_id = quit.id().clone();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format_tooltip(initial_pending, initial_overdue))
        .with_icon(icon::tray_icon(initial_pending, initial_overdue))
        .build()?;

    let tx_menu = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let mapped = if event.id == edit_id {
            Some(PlatformEvent::OpenEditor)
        } else if event.id == settings_id {
            Some(PlatformEvent::OpenSettings)
        } else if event.id == toggle_id {
            Some(PlatformEvent::ToggleWidget)
        } else if event.id == quit_id {
            Some(PlatformEvent::Quit)
        } else {
            None
        };
        if let Some(ev) = mapped {
            let _ = tx_menu.send_blocking(ev);
        }
    }));

    println!("[wasurenai] 托盘就绪");
    Ok((tray, toggle))
}

fn format_tooltip(pending: usize, overdue: usize) -> String {
    if overdue > 0 {
        format!("Wasurenai · {pending} 项未完成（{overdue} 项已逾期）")
    } else if pending > 0 {
        format!("Wasurenai · {pending} 项未完成")
    } else {
        "Wasurenai · 全部已完成".to_string()
    }
}

fn toggle_label(widget_visible: bool) -> &'static str {
    if widget_visible {
        "收起面板"
    } else {
        "展开面板"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_tooltip_and_toggle_label_reflect_counts() {
        assert_eq!(format_tooltip(0, 0), "Wasurenai · 全部已完成");
        assert_eq!(format_tooltip(3, 0), "Wasurenai · 3 项未完成");
        assert_eq!(
            format_tooltip(4, 2),
            "Wasurenai · 4 项未完成（2 项已逾期）"
        );
        assert_eq!(toggle_label(true), "收起面板");
        assert_eq!(toggle_label(false), "展开面板");
    }
}

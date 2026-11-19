//! 托盘图标 + 全局热键所在的"平台线程"。
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
use windows::Win32::Foundation::{LPARAM, WPARAM};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_APP, WM_QUIT,
};

use crate::core::hotkey::HotkeySpec;
use crate::platform::PlatformEvent;

/// 唤醒消息泵用的自定义消息。
const WM_APP_COMMAND: u32 = WM_APP + 1;

/// 当前注册的热键 id（热键事件回调里用来比对）。
static CURRENT_ID: AtomicU32 = AtomicU32::new(0);

/// 平台线程 id，供 UI → 平台方向唤醒泵。
static PUMP_THREAD: OnceLock<u32> = OnceLock::new();

/// UI 发给平台线程的命令。
#[derive(Debug, Clone)]
pub enum PlatformCommand {
    /// 重新注册全局快捷键。
    SetHotkey(HotkeySpec),
}

/// 给平台线程发命令（必要时先启动它）。返回是否成功唤醒。
pub fn send_command(cmd: PlatformCommand) -> bool {
    let Some(thread_id) = PUMP_THREAD.get() else {
        eprintln!("[wasurenai] 平台线程还没就绪，命令被丢弃");
        return false;
    };
    // 命令由泵线程自己从 channel 里取，这条线程消息只负责把它叫醒。
    if let Some(queue) = COMMAND_QUEUE.get() {
        let _ = queue.send_blocking(cmd);
    }
    unsafe { PostThreadMessageW(*thread_id, WM_APP_COMMAND, WPARAM(0), LPARAM(0)).is_ok() }
}

/// 命令队列（创建一次，供任意线程投递）。
static COMMAND_QUEUE: OnceLock<Sender<PlatformCommand>> = OnceLock::new();

/// 启动平台线程。`initial_hotkey` 启动时就注册。
pub fn spawn(tx: Sender<PlatformEvent>, initial_hotkey: HotkeySpec) {
    let (command_tx, command_rx) = async_channel::unbounded();
    let _ = COMMAND_QUEUE.set(command_tx);

    std::thread::Builder::new()
        .name("wasurenai-platform".into())
        .spawn(move || unsafe {
            // 托盘图标必须活到消息泵结束：一旦 drop，图标就从托盘里消失了。
            let _tray = match create_tray(tx.clone()) {
                Ok(tray) => Some(tray),
                Err(err) => {
                    eprintln!("[wasurenai] 托盘创建失败：{err:#}");
                    None
                }
            };

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
                    // 把积压的命令都处理掉
                    while let Ok(command) = command_rx.try_recv() {
                        match command {
                            PlatformCommand::SetHotkey(spec) => hotkeys.apply(spec),
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
    /// 创建失败时为 None：热键用不了，但托盘还得工作。
    manager: Option<GlobalHotKeyManager>,
    current: Option<HotKey>,
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
            tx,
        }
    }

    fn apply(&mut self, spec: HotkeySpec) {
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

/// 把规范化的快捷键翻译成 `global_hotkey` 的组合键。
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

fn create_tray(tx: Sender<PlatformEvent>) -> anyhow::Result<tray_icon::TrayIcon> {
    let menu = Menu::new();
    let edit = MenuItem::new("编辑内容", true, None);
    let toggle = MenuItem::new("收起 / 展开", true, None);
    let quit = MenuItem::new("退出", true, None);
    menu.append_items(&[&edit, &toggle, &PredefinedMenuItem::separator(), &quit])?;
    let edit_id = edit.id().clone();
    let toggle_id = toggle.id().clone();
    let quit_id = quit.id().clone();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("wasurenai")
        .with_icon(app_icon())
        .build()?;

    // 菜单事件在平台线程的消息泵里派发，这里只负责把事件转发出去。
    let tx_menu = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let mapped = if event.id == edit_id {
            Some(PlatformEvent::OpenEditor)
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
    Ok(tray)
}

/// 临时的纯色方块图标（M5 换成正式图标）。
fn app_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let inner = x > 4 && x < 27 && y > 4 && y < 27;
            let (r, g, b) = if inner {
                (0x5b, 0xc4, 0xff)
            } else {
                (0x1e, 0x6f, 0xd9)
            };
            rgba.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("内置图标数据合法")
}

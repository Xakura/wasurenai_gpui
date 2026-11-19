//! 托盘图标（M1 只有「退出」）所在的消息泵线程。
//!
//! 为什么必须单开一个线程：`tray-icon` 内部是 `Rc`（非 `Send`），并且要求在**创建它
//! 的线程上有一个 Win32 消息泵**（`Shell_NotifyIcon` 的回调靠窗口消息驱动）。
//! 后面 M3 的全局热键（`global-hotkey` 的 `RegisterHotKey`）也要同一个消息泵，
//! 所以这里就是"平台线程"，托盘和热键都放在里面。
//!
//! 事件通过 `async_channel` 单向送到 gpui 主线程；**所有状态变更只在 gpui 主线程发生**。

use async_channel::Sender;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, TranslateMessage,
};

use crate::platform::PlatformEvent;

/// 启动平台线程（托盘 + 消息泵）。返回时线程可能还没建好托盘，失败会打印到 stderr。
pub fn spawn(tx: Sender<PlatformEvent>) {
    std::thread::Builder::new()
        .name("wasurenai-platform".into())
        .spawn(move || unsafe {
            // 托盘图标必须活到消息泵结束：一旦 drop，图标就从托盘里消失。
            let _tray = match create_tray(tx) {
                Ok(tray) => Some(tray),
                Err(err) => {
                    eprintln!("[wasurenai] 托盘创建失败：{err:#}");
                    None
                }
            };

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn platform thread");
}

fn create_tray(tx: Sender<PlatformEvent>) -> anyhow::Result<tray_icon::TrayIcon> {
    let menu = Menu::new();
    let edit = MenuItem::new("编辑内容", true, None);
    let toggle = MenuItem::new("收起 / 展开", true, None);
    let quit = MenuItem::new("退出", true, None);
    menu.append_items(&[
        &edit,
        &toggle,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;
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

    // ---- 全局热键（失败不致命：可能被别的程序占用了）----
    let mods = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT;
    let overlay_hotkey = HotKey::new(Some(mods), Code::KeyW);
    let overlay_id = overlay_hotkey.id();
    // 调试用：WASURENAI_DEBUG=1 时额外注册「收起/展开」，方便无托盘环境验证
    let widget_hotkey = std::env::var_os("WASURENAI_DEBUG")
        .is_some()
        .then(|| HotKey::new(Some(mods), Code::KeyE));
    let widget_id = widget_hotkey.as_ref().map(|hotkey| hotkey.id());

    let tx_hotkey = tx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }
        let mapped = if event.id == overlay_id {
            Some(PlatformEvent::ToggleFloat)
        } else if Some(event.id) == widget_id {
            Some(PlatformEvent::ToggleWidget)
        } else {
            None
        };
        if let Some(ev) = mapped {
            let _ = tx_hotkey.send_blocking(ev);
        }
    }));

    match GlobalHotKeyManager::new() {
        Ok(manager) => {
            let registered = match &widget_hotkey {
                Some(widget_hotkey) => {
                    let all = [overlay_hotkey, *widget_hotkey];
                    match manager.register_all(&all) {
                        Ok(()) => Some("Ctrl+Alt+Shift+W / Ctrl+Alt+Shift+E（调试）"),
                        Err(err) => {
                            eprintln!("[wasurenai] 热键注册失败：{err}");
                            None
                        }
                    }
                }
                None => match manager.register(overlay_hotkey) {
                    Ok(()) => Some("Ctrl+Alt+Shift+W"),
                    Err(err) => {
                        eprintln!("[wasurenai] 热键注册失败：{err}");
                        None
                    }
                },
            };
            if let Some(list) = registered {
                println!("[wasurenai] 热键已注册 {list}");
                // 泄漏到进程结束：注册必须一直有效
                std::mem::forget(manager);
            }
        }
        Err(err) => eprintln!("[wasurenai] 热键管理器创建失败：{err}"),
    }

    println!("[wasurenai] 托盘就绪");
    Ok(tray)
}

/// 临时的纯色方块图标（M5 换成正式图标：TODO 在 docs/plan.md）。
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

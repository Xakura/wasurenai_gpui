//! 托盘图标（M1 只有「退出」）所在的消息泵线程。
//!
//! 为什么必须单开一个线程：`tray-icon` 内部是 `Rc`（非 `Send`），并且要求在**创建它
//! 的线程上有一个 Win32 消息泵**（`Shell_NotifyIcon` 的回调靠窗口消息驱动）。
//! 后面 M3 的全局热键（`global-hotkey` 的 `RegisterHotKey`）也要同一个消息泵，
//! 所以这里就是"平台线程"，托盘和热键都放在里面。
//!
//! 事件通过 `async_channel` 单向送到 gpui 主线程；**所有状态变更只在 gpui 主线程发生**。

use async_channel::Sender;
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
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
    let quit = MenuItem::new("退出", true, None);
    menu.append_items(&[&quit])?;
    let quit_id = quit.id().clone();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("wasurenai")
        .with_icon(app_icon())
        .build()?;

    // 菜单事件在平台线程的消息泵里派发，这里只负责把事件转发出去。
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            let _ = tx.send_blocking(PlatformEvent::Quit);
        }
    }));

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

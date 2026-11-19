//! wasurenai —— 常驻桌面的待办清单（GPUI 版）。

use async_channel::unbounded;
use gpui_kit::*;

use wasurenai_gpui::{platform, ui};

fn main() {
    let (tx, rx) = unbounded::<platform::PlatformEvent>();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // 所有窗口都关掉后进程继续活着（托盘常驻的前提）
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            cx.set_global(ui::windows::Windows::default());

            let state = cx.new(|_| ui::state::AppState::load());

            // 平台线程 → gpui 主线程：这里只做窗口/应用级操作，
            // 数据变更交给 entity（分开处理，避免 entity 重入 lease）。
            // M1 只有「退出」一个事件；M2/M3 增加事件时这里扩展成 while let 循环。
            let pump = cx.spawn(async move |cx| {
                if let Ok(platform::PlatformEvent::Quit) = rx.recv().await {
                    cx.update(|app| app.quit());
                }
            });
            pump.detach();

            platform::win_thread::spawn(tx);

            ui::windows::open_widget(cx, state);
        });
}

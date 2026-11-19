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
            // 编辑窗口用浅色（组件库默认就是 Light）；桌面层/浮层不用组件主题，自己画深色
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Light, None, cx);
            cx.set_global(ui::windows::Windows::default());

            cx.on_window_closed(|_app, id| {
                println!("[wasurenai] 窗口已关闭 id={id:?}");
            })
            .detach();

            let state = cx.new(|_| ui::state::AppState::load());

            // 平台线程 → gpui 主线程：这里只做窗口/应用级操作，
            // 数据变更交给 entity（分开处理，避免 entity 重入 lease）。
            let pump_state = state.clone();
            let pump = cx.spawn(async move |cx| {
                while let Ok(event) = rx.recv().await {
                    match event {
                        platform::PlatformEvent::OpenEditor => {
                            cx.update(|app| ui::windows::open_editor(app, pump_state.clone()));
                        }
                        platform::PlatformEvent::ToggleWidget => {
                            cx.update(|app| ui::windows::toggle_widget(app, pump_state.clone()));
                        }
                        platform::PlatformEvent::ToggleOverlay => {
                            cx.update(|app| ui::windows::toggle_overlay(app, pump_state.clone()));
                        }
                        platform::PlatformEvent::Quit => {
                            println!("[wasurenai] 收到退出请求（托盘）");
                            cx.update(|app| app.quit());
                            return;
                        }
                    }
                }
            });
            pump.detach();

            // 到期提醒：每 500ms 检查一次（条目量很小，这点开销可以忽略）
            let reminder_state = state.clone();
            let reminder_task = cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    cx.update(|app| ui::windows::refresh_reminder(app, reminder_state.clone()));
                }
            });
            reminder_task.detach();

            platform::win_thread::spawn(tx);

            // 启动一律展开：上次是收起状态也开，避免用户下次启动看不到面板以为坏了。
            // （widget_visible 仍然记录/落盘，只是不作为启动条件。）
            ui::windows::open_widget(cx, state);
        });
}

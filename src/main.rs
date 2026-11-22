//! wasurenai —— 常驻桌面的待办清单（GPUI 版）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use async_channel::unbounded;
use chrono::Local;
use gpui_kit::*;

use wasurenai_gpui::{core, platform, ui};

fn main() {
    let Some(_instance_guard) = platform::single_instance::SingleInstanceGuard::acquire() else {
        println!("[wasurenai] 已有实例在运行，退出重复启动");
        return;
    };

    let (tx, rx) = unbounded::<platform::PlatformEvent>();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // 所有窗口都关掉后进程继续活着（托盘常驻的前提）
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale("zh-CN");
            // 编辑窗口用浅色（组件库默认就是 Light）；桌面层/浮层不用组件主题，自己画深色
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Light, None, cx);
            cx.set_global(ui::windows::Windows::default());

            let state = cx.new(|_| ui::state::AppState::load());

            // 任何窗口关闭前强制刷盘（防止最后 350ms 防抖期内的编辑丢失）
            let close_state = state.clone();
            cx.on_window_closed(move |app, id| {
                println!("[wasurenai] 窗口已关闭 id={id:?}");
                close_state.update(app, |s, _| s.flush_save());
            })
            .detach();

            // 状态变化时同步托盘 tooltip / 图标角标
            cx.observe(&state, |state, app| {
                ui::windows::sync_tray(app, &state);
            })
            .detach();

            let (initial_hotkey, initial_pending, initial_overdue) = {
                let app = state.read(cx);
                let now = Local::now().naive_local();
                let pending = core::ordering::pending_count(&app.store.items);
                let overdue = app
                    .store
                    .items
                    .iter()
                    .filter(|item| item.is_overdue(now))
                    .count();
                (app.hotkey(), pending, overdue)
            };

            // 平台线程 → gpui 主线程：这里只做窗口/应用级操作，
            // 数据变更交给 entity（分开处理，避免 entity 重入 lease）。
            let pump_state = state.clone();
            let pump = cx.spawn(async move |cx| {
                while let Ok(event) = rx.recv().await {
                    match event {
                        platform::PlatformEvent::OpenEditor => {
                            cx.update(|app| {
                                ui::windows::open_editor(
                                    app,
                                    pump_state.clone(),
                                    ui::editor::Tab::Todos,
                                )
                            });
                        }
                        platform::PlatformEvent::OpenSettings => {
                            cx.update(|app| {
                                ui::windows::open_editor(
                                    app,
                                    pump_state.clone(),
                                    ui::editor::Tab::Settings,
                                )
                            });
                        }
                        platform::PlatformEvent::ToggleWidget => {
                            cx.update(|app| ui::windows::toggle_widget(app, pump_state.clone()));
                        }
                        platform::PlatformEvent::ToggleFloat => {
                            cx.update(|app| ui::windows::toggle_float(app, pump_state.clone()));
                        }
                        platform::PlatformEvent::HotkeyChanged { spec, error } => {
                            let (spec, error) = (spec.clone(), error.clone());
                            println!(
                                "[wasurenai] 快捷键 {spec} -> {}",
                                error.as_deref().unwrap_or("已生效")
                            );
                            pump_state.update(cx, |app, cx| {
                                app.hotkey_error = error;
                                cx.notify();
                            });
                        }
                        platform::PlatformEvent::Quit => {
                            println!("[wasurenai] 收到退出请求（托盘）");
                            pump_state.update(cx, |s, _| s.flush_save());
                            cx.update(|app| app.quit());
                            return;
                        }
                    }
                }
            });
            pump.detach();

            // 每 500ms 对齐一次窗口层级：有到期提醒就自动上浮，确认完自动回落
            let tick_state = state.clone();
            let tick_task = cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    cx.update(|app| ui::windows::sync_layer(app, tick_state.clone()));
                }
            });
            tick_task.detach();

            platform::win_thread::spawn(tx, initial_hotkey, initial_pending, initial_overdue);

            // 启动一律展开：上次是收起状态也开，避免用户下次启动看不到面板以为坏了。
            // （widget_visible 仍然记录/落盘，只是不作为启动条件。）
            ui::windows::open_widget(cx, state);
        });
}

//! 窗口生命周期与层级。所有函数都接收 `&mut App`，**绝不在 entity update 内部调用**
//! （窗口更新会同步触发绘制，进而 lease 视图 entity，见 `ui/mod.rs`）。

use gpui_kit::*;

use crate::core::ordering;
use crate::platform::icon;
use crate::platform::win_layer::{self, Layer};
use crate::platform::win_thread::{self, PlatformCommand};
use crate::ui::editor::{EditorWindow, Tab};
use crate::ui::state::AppState;
use crate::ui::widget::{PanelMotion, WidgetWindow};

/// 窗口句柄注册表。gpui Global，任何拿到 `&mut App` 的地方都能访问。
#[derive(Default)]
pub struct Windows {
    pub widget: Option<AnyWindowHandle>,
    pub widget_view: Option<Entity<WidgetWindow>>,
    pub editor: Option<AnyWindowHandle>,
    /// 编辑窗口的根视图，用来在窗口已打开时切页签。
    pub editor_view: Option<Entity<EditorWindow>>,
    /// 桌面面板当前是否处于显示状态（用于同步托盘菜单文案，避免异步竞态）。
    pub widget_shown: bool,
    /// 面板显隐/层级退场动画代际，打断旧的延迟隐藏或延迟回落。
    transition_gen: u64,
    /// 是否正在播放回落桌面层的先导淡出动画（200ms 后切 Bottom）。
    dropping_to_bottom: bool,
    /// 是否正在等待 `Bottom -> Top` 升层前的全透明预清帧提交。
    raising_to_top: bool,
    /// 上次推送给托盘的状态，避免重复发命令。
    last_tray: Option<(usize, usize, bool)>,
}

impl Global for Windows {}

fn screen_bounds(cx: &mut App) -> Bounds<Pixels> {
    cx.primary_display()
        .map(|display| display.visible_bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))))
}

/// 桌面层窗口的位置：主屏右下角，320x560，边距 24。上浮时位置尺寸不变。
pub fn widget_bounds(cx: &mut App) -> Bounds<Pixels> {
    let screen = screen_bounds(cx);
    let (width, height, margin) = (px(320.0), px(560.0), px(24.0));
    Bounds::new(
        point(
            screen.origin.x + screen.size.width - width - margin,
            screen.origin.y + screen.size.height - height - margin,
        ),
        size(width, height),
    )
}

/// 从 gpui 窗口拿 Win32 HWND。
pub(crate) fn hwnd_of(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(windows::Win32::Foundation::HWND(
            win32.hwnd.get() as *mut core::ffi::c_void
        )),
        _ => None,
    }
}

/// 打开桌面层窗口。
///
/// 用裸 `cx.open_window`（不套 `gpui_base::Root`）：Root 的 surface 背景是不透明的，
/// 会把窗口的透明/圆角盖死。
pub fn open_widget(cx: &mut App, state: Entity<AppState>) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(widget_bounds(cx))),
        titlebar: None,
        focus: false,
        show: true,
        // 用 PopUp（WS_EX_TOOLWINDOW）避免 GPUI 给 Normal 窗口附加 WS_EX_APPWINDOW，
        // 否则即使设了 Progman owner，任务栏仍会显示桌面面板图标。
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        // 不活跃窗口默认被限速到 30fps；桌面常驻面板本身不抢焦点，解除限速保证动画顺滑
        inactive_frame_interval: None,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    let tray_state = state.clone();
    let widget_state = state.clone();
    let mut created_view = None;
    match cx.open_window(options, |window, cx| {
        window.set_window_title("wasurenai");
        let view = cx.new(|cx| WidgetWindow::new(widget_state, cx));
        created_view = Some(view.clone());
        view
    }) {
        Ok(handle) => {
            println!("[wasurenai] 桌面层窗口已打开 {handle:?}");
            let win = cx.global_mut::<Windows>();
            win.widget = Some(handle.into());
            win.widget_view = created_view;
            win.widget_shown = true;
            win.dropping_to_bottom = false;
            win.raising_to_top = false;
            sync_tray(cx, &tray_state);
        }
        Err(err) => eprintln!("[wasurenai] 桌面层窗口打开失败：{err:#}"),
    }
}

/// 从隐藏态恢复面板：先设目标层级、启动入场动画（第 0 帧透明），再 `ShowWindow` 避免旧帧闪烁。
fn reveal_hidden_widget(
    cx: &mut App,
    handle: AnyWindowHandle,
    target: Layer,
    motion: PanelMotion,
) -> bool {
    let layered = handle
        .update(cx, |_, window, _| {
            let hwnd = hwnd_of(window)?;
            win_layer::set_layer(hwnd, target);
            Some(())
        })
        .ok()
        .flatten();
    if layered.is_none() {
        return false;
    }

    let view = {
        let win = cx.global_mut::<Windows>();
        win.widget_shown = true;
        win.dropping_to_bottom = false;
        win.raising_to_top = false;
        win.transition_gen += 1;
        win.widget_view.clone()
    };
    if let Some(view) = view {
        view.update(cx, |w, cx| w.play_motion(motion, cx));
    }

    let _ = handle.update(cx, |_, window, _| {
        if let Some(hwnd) = hwnd_of(window) {
            win_layer::set_visible(hwnd, true);
        }
    });
    true
}

/// 收起 / 展开桌面层窗口（抽屉弹出/缩入动画，隐藏而不是销毁）。
pub fn toggle_widget(cx: &mut App, state: Entity<AppState>) {
    let Some(handle) = cx.global::<Windows>().widget else {
        open_widget(cx, state);
        return;
    };

    let next = !cx.global::<Windows>().widget_shown;
    if next {
        let target = desired_layer(cx, &state);
        if !reveal_hidden_widget(cx, handle, target, PanelMotion::Expand) {
            clear_widget_handles(cx);
            open_widget(cx, state);
            return;
        }

        state.update(cx, |app, cx| {
            let changed = !app.store.widget_visible;
            app.store.widget_visible = true;
            app.commit_click(changed, cx);
            cx.notify();
        });
        println!("[wasurenai] 桌面层展开 visible=true layer={target:?}");
        sync_tray(cx, &state);
    } else {
        let (generation, view) = {
            let win = cx.global_mut::<Windows>();
            win.widget_shown = false;
            win.dropping_to_bottom = false;
            win.raising_to_top = false;
            win.transition_gen += 1;
            (win.transition_gen, win.widget_view.clone())
        };
        // 从托盘主动收起时一并退出上浮态，否则 500ms tick 或状态对齐会把窗口判定为应保持 Top
        state.update(cx, |app, cx| {
            app.user_floated = false;
            let changed = app.store.widget_visible;
            app.store.widget_visible = false;
            app.commit_click(changed, cx);
            cx.notify();
        });
        if let Some(view) = view {
            view.update(cx, |w, cx| w.play_motion(PanelMotion::Collapse, cx));
        }

        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            cx.update(|app| {
                let still_hidden = {
                    let win = app.global::<Windows>();
                    win.transition_gen == generation && !win.widget_shown
                };
                if !still_hidden {
                    return;
                }
                let _ = handle.update(app, |_, window, _| {
                    if let Some(hwnd) = hwnd_of(window) {
                        win_layer::set_visible(hwnd, false);
                        win_layer::set_layer(hwnd, Layer::Bottom);
                    }
                });
                if let Some(view) = app.global::<Windows>().widget_view.clone() {
                    view.update(app, |w, cx| w.set_hidden(cx));
                }
            });
        })
        .detach();

        println!("[wasurenai] 桌面层收起 visible=false");
        sync_tray(cx, &state);
    }
}

/// 快捷键：在「桌面层」和「置顶层」之间切换同一个窗口；若已收起则展开并上浮。
pub fn toggle_float(cx: &mut App, state: Entity<AppState>) {
    let was_shown = cx.global::<Windows>().widget_shown;
    if !was_shown {
        let Some(handle) = cx.global::<Windows>().widget else {
            state.update(cx, |app, _| app.user_floated = true);
            open_widget(cx, state.clone());
            sync_layer(cx, state);
            return;
        };

        if !reveal_hidden_widget(cx, handle, Layer::Top, PanelMotion::FloatIn) {
            clear_widget_handles(cx);
            state.update(cx, |app, _| app.user_floated = true);
            open_widget(cx, state.clone());
            sync_layer(cx, state);
            return;
        }

        state.update(cx, |app, cx| {
            app.user_floated = true;
            let changed = !app.store.widget_visible;
            app.store.widget_visible = true;
            app.commit_click(changed, cx);
            cx.notify();
        });
        println!("[wasurenai] 快捷键：收起态直接展开并上浮 user_floated=true");
        sync_tray(cx, &state);
        return;
    }

    let floated = state.update(cx, |app, cx| {
        app.user_floated = !app.user_floated;
        cx.notify();
        app.user_floated
    });
    println!("[wasurenai] 快捷键：user_floated={floated}");
    sync_layer(cx, state);
}

fn has_due_reminders(cx: &App, state: &Entity<AppState>) -> bool {
    let app = state.read(cx);
    let now = chrono::Local::now().naive_local();
    !app.due_items(now).is_empty()
}

/// 期望的层级：用户手动上浮 或 有到期提醒 → 置顶；否则沉底。
fn desired_layer(cx: &App, state: &Entity<AppState>) -> Layer {
    let app = state.read(cx);
    let now = chrono::Local::now().naive_local();
    if app.user_floated || !app.due_items(now).is_empty() {
        Layer::Top
    } else {
        Layer::Bottom
    }
}

fn clear_widget_handles(cx: &mut App) {
    let win = cx.global_mut::<Windows>();
    win.widget = None;
    win.widget_view = None;
    win.widget_shown = false;
    win.dropping_to_bottom = false;
    win.raising_to_top = false;
    win.transition_gen += 1;
}

/// `Bottom -> Top` 升层第 2 帧回调：此时全透明预清帧已完成 draw+present，
/// 可以安全置顶并立刻在同一帧开始 `FloatIn` 动效，不会在顶层闪出旧不透明画面。
fn finish_raise_to_top(
    window: &mut Window,
    cx: &mut App,
    state: Entity<AppState>,
    generation: u64,
) {
    let view = {
        let win = cx.global_mut::<Windows>();
        if win.transition_gen != generation || !win.widget_shown || !win.raising_to_top {
            return;
        }
        win.raising_to_top = false;
        win.widget_view.clone()
    };

    if desired_layer(cx, &state) != Layer::Top {
        if let Some(view) = view {
            view.update(cx, |w, cx| w.play_motion(PanelMotion::SettleFadeIn, cx));
        }
        return;
    }

    if let Some(hwnd) = hwnd_of(window) {
        let ok = win_layer::set_layer(hwnd, Layer::Top);
        println!("[wasurenai] 层级 -> Top ok={ok}");
    }
    if let Some(view) = view {
        view.update(cx, |w, cx| w.play_motion(PanelMotion::FloatIn, cx));
    }
}

/// 把窗口层级 / 可见性对齐到当前状态。幂等，可以每 500ms 调一次。
pub fn sync_layer(cx: &mut App, state: Entity<AppState>) {
    let Some(handle) = cx.global::<Windows>().widget else {
        sync_tray(cx, &state);
        return;
    };

    let must_show = has_due_reminders(cx, &state);
    let target = desired_layer(cx, &state);
    let (was_shown, dropping, raising) = {
        let win = cx.global::<Windows>();
        (win.widget_shown, win.dropping_to_bottom, win.raising_to_top)
    };

    // 用户已主动收起且当前无强提醒：绝不因为历史标志或 tick 把窗口重新弹出
    if !was_shown && !must_show {
        sync_tray(cx, &state);
        return;
    }

    if !was_shown && must_show {
        if !reveal_hidden_widget(cx, handle, Layer::Top, PanelMotion::FloatIn) {
            clear_widget_handles(cx);
            sync_tray(cx, &state);
            return;
        }
        state.update(cx, |app, cx| {
            let changed = !app.store.widget_visible;
            app.store.widget_visible = true;
            app.commit_click(changed, cx);
            cx.notify();
        });
        sync_tray(cx, &state);
        return;
    }

    match target {
        Layer::Top => {
            if dropping {
                // 仍在 Top 态播放回落淡出时取消回落：无需重新 SetWindowPos，直接原地 FloatIn
                let view = {
                    let win = cx.global_mut::<Windows>();
                    win.dropping_to_bottom = false;
                    win.raising_to_top = false;
                    win.transition_gen += 1;
                    win.widget_view.clone()
                };
                if let Some(view) = view {
                    view.update(cx, |w, cx| w.play_motion(PanelMotion::FloatIn, cx));
                }
            } else if win_layer::current_layer() != Layer::Top && !raising {
                let visible = handle
                    .update(cx, |_, window, _| Some(window.is_visible()))
                    .ok()
                    .flatten();
                match visible {
                    None => clear_widget_handles(cx),
                    Some(false) => {
                        if !reveal_hidden_widget(cx, handle, Layer::Top, PanelMotion::FloatIn) {
                            clear_widget_handles(cx);
                        }
                    }
                    Some(true) => {
                        let (generation, view) = {
                            let win = cx.global_mut::<Windows>();
                            win.dropping_to_bottom = false;
                            win.raising_to_top = true;
                            win.transition_gen += 1;
                            (win.transition_gen, win.widget_view.clone())
                        };
                        if let Some(view) = view {
                            view.update(cx, |w, cx| w.set_hidden(cx));
                        }
                        let state_for_raise = state.clone();
                        let _ = handle.update(cx, |_, window, _| {
                            // gpui-pre 在 draw/present 之前执行 next_frame_callbacks，
                            // 嵌套两步才能保证先在 Bottom 层完成一帧全透明 present，再置顶并 FloatIn。
                            window.on_next_frame(move |window, _cx| {
                                window.on_next_frame(move |window, cx| {
                                    finish_raise_to_top(window, cx, state_for_raise, generation);
                                });
                            });
                        });
                    }
                }
            }
        }
        Layer::Bottom => {
            if raising {
                // 在第 2 帧升层之前又切回 Bottom：取消升层并平滑恢复桌面层显示，避免留在透明态
                let view = {
                    let win = cx.global_mut::<Windows>();
                    win.raising_to_top = false;
                    win.transition_gen += 1;
                    win.widget_view.clone()
                };
                if let Some(view) = view {
                    view.update(cx, |w, cx| w.play_motion(PanelMotion::SettleFadeIn, cx));
                }
            } else if win_layer::current_layer() == Layer::Top && !dropping {
                let (generation, view) = {
                    let win = cx.global_mut::<Windows>();
                    win.dropping_to_bottom = true;
                    win.transition_gen += 1;
                    (win.transition_gen, win.widget_view.clone())
                };
                if let Some(view) = view {
                    view.update(cx, |w, cx| w.play_motion(PanelMotion::DropFadeOut, cx));
                }
                let drop_state = state.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(200))
                        .await;
                    cx.update(|app| {
                        let should_finish = {
                            let win = app.global::<Windows>();
                            win.transition_gen == generation
                                && win.widget_shown
                                && win.dropping_to_bottom
                        };
                        if !should_finish {
                            return;
                        }
                        app.global_mut::<Windows>().dropping_to_bottom = false;
                        if desired_layer(app, &drop_state) != Layer::Bottom {
                            return;
                        }
                        let applied = handle
                            .update(app, |_, window, _| {
                                let hwnd = hwnd_of(window)?;
                                let ok = win_layer::set_layer(hwnd, Layer::Bottom);
                                println!("[wasurenai] 层级 -> Bottom ok={ok}");
                                Some(())
                            })
                            .ok()
                            .flatten();
                        if applied.is_none() {
                            clear_widget_handles(app);
                            return;
                        }
                        if let Some(view) = app.global::<Windows>().widget_view.clone() {
                            view.update(app, |w, cx| w.play_motion(PanelMotion::SettleFadeIn, cx));
                        }
                    });
                })
                .detach();
            }
        }
    }

    sync_tray(cx, &state);
}

/// 将未完成/逾期计数与面板可见状态同步到系统托盘（状态未变化时不重复发命令）。
pub fn sync_tray(cx: &mut App, state: &Entity<AppState>) {
    let now = chrono::Local::now().naive_local();
    let (pending, overdue) = {
        let app = state.read(cx);
        let pending = ordering::pending_count(&app.store.items);
        let overdue = app
            .store
            .items
            .iter()
            .filter(|item| item.is_overdue(now))
            .count();
        (pending, overdue)
    };
    let widget_visible = cx.global::<Windows>().widget_shown;
    let snapshot = (pending, overdue, widget_visible);
    if cx.global::<Windows>().last_tray == Some(snapshot) {
        return;
    }
    cx.global_mut::<Windows>().last_tray = Some(snapshot);
    win_thread::send_command(PlatformCommand::UpdateTray {
        pending,
        overdue,
        widget_visible,
    });
}

/// 编辑窗口的位置：屏幕居中，860x620。
fn editor_bounds(cx: &mut App) -> Bounds<Pixels> {
    let screen = screen_bounds(cx);
    let (width, height) = (px(860.0), px(620.0));
    Bounds::new(
        point(
            screen.origin.x + (screen.size.width - width) * 0.5,
            screen.origin.y + (screen.size.height - height) * 0.5,
        ),
        size(width, height),
    )
}

/// 打开编辑窗口并切到指定页签；已经开着就切页 + 拉到前台。
pub fn open_editor(cx: &mut App, state: Entity<AppState>, tab: Tab) {
    if let Some(handle) = cx.global::<Windows>().editor {
        let alive = handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok();
        if alive {
            if let Some(view) = cx.global::<Windows>().editor_view.clone() {
                view.update(cx, |editor, cx| editor.set_tab(tab, cx));
            }
            return;
        }
        cx.global_mut::<Windows>().editor = None;
        cx.global_mut::<Windows>().editor_view = None;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(editor_bounds(cx))),
        titlebar: Some(TitlebarOptions {
            title: Some("Wasurenai".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_background: WindowBackgroundAppearance::Opaque,
        window_min_size: Some(size(px(720.0), px(480.0))),
        ..Default::default()
    };

    match gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| EditorWindow::new(state, tab, window, cx))
    }) {
        Ok((handle, view)) => {
            println!("[wasurenai] 编辑窗口已打开 tab={tab:?}");
            cx.global_mut::<Windows>().editor = Some(handle);
            cx.global_mut::<Windows>().editor_view = Some(view);
            // 新建的窗口不一定会被系统提到前台（实测会被终端等窗口挡住），同时设置内嵌窗口图标
            let _ = handle.update(cx, |_, window, _| {
                if let Some(hwnd) = hwnd_of(window) {
                    icon::apply_window_icon(hwnd);
                }
                window.activate_window();
            });
        }
        Err(err) => eprintln!("[wasurenai] 编辑窗口打开失败：{err:#}"),
    }
}

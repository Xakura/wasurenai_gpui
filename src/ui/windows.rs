//! 窗口生命周期。所有函数都接收 `&mut App`，**绝不在 entity update 内部调用**
//! （`open_window` 会同步绘制新窗口并 lease 视图 entity，见 `ui/mod.rs`）。

use gpui_kit::*;

use crate::platform::win_bottom;
use crate::ui::editor::EditorWindow;
use crate::ui::overlay::{OverlayMode, OverlayWindow};
use crate::ui::state::AppState;
use crate::ui::widget::WidgetWindow;

/// 窗口句柄注册表。gpui Global，任何拿到 `&mut App` 的地方都能访问。
#[derive(Default)]
pub struct Windows {
    pub widget: Option<AnyWindowHandle>,
    pub editor: Option<AnyWindowHandle>,
    pub overlay: Option<AnyWindowHandle>,
}

impl Global for Windows {}

fn screen_bounds(cx: &mut App) -> Bounds<Pixels> {
    cx.primary_display()
        .map(|display| display.visible_bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))))
}

/// 桌面层窗口的位置：主屏右下角，320x560，边距 24。
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

/// 打开桌面层窗口（收起后重新展开也走这里）。
pub fn open_widget(cx: &mut App, state: Entity<AppState>) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(widget_bounds(cx))),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::Normal,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    match cx.open_window(options, move |window, cx| {
        window.set_window_title("wasurenai");
        cx.new(|cx| WidgetWindow::new(state, cx))
    }) {
        Ok(handle) => {
            println!("[wasurenai] 桌面层窗口已打开 {handle:?}");
            cx.global_mut::<Windows>().widget = Some(handle.into());
        }
        Err(err) => eprintln!("[wasurenai] 桌面层窗口打开失败：{err:#}"),
    }
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

/// 收起 / 展开桌面层窗口。
///
/// gpui 没有隐藏窗口的 API，但窗口是我们自己的 Win32 窗口，直接 `ShowWindow` 即可。
/// **不走"关窗再重建"**：实测重建出来的第二个窗口内容不呈现（见 `win_bottom::set_visible` 注释）。
pub fn toggle_widget(cx: &mut App, state: Entity<AppState>) {
    let Some(handle) = cx.global::<Windows>().widget else {
        open_widget(cx, state);
        return;
    };

    let toggled = handle
        .update(cx, |_, window, _| {
            let hwnd = hwnd_of(window)?;
            let next = !window.is_visible();
            let ok = win_bottom::set_visible(hwnd, next);
            Some(next && ok)
        })
        .ok()
        .flatten();

    match toggled {
        Some(visible) => {
            println!("[wasurenai] 桌面层 visible={visible}");
            // 触发一次重绘，避免展开时停留在旧帧
            state.update(cx, |_app, cx| cx.notify());
            state.update(cx, |app, cx| {
                let changed = app.store.widget_visible != visible;
                app.store.widget_visible = visible;
                app.commit_click(changed, cx);
            });
        }
        None => eprintln!("[wasurenai] 收起/展开失败：拿不到窗口"),
    }
}

/// 编辑窗口的位置：屏幕居中，760x560。
fn editor_bounds(cx: &mut App) -> Bounds<Pixels> {
    let screen = screen_bounds(cx);
    let (width, height) = (px(760.0), px(560.0));
    Bounds::new(
        point(
            screen.origin.x + (screen.size.width - width) * 0.5,
            screen.origin.y + (screen.size.height - height) * 0.5,
        ),
        size(width, height),
    )
}

/// 打开编辑窗口；已经开着就把它拉到前台。
pub fn open_editor(cx: &mut App, state: Entity<AppState>) {
    if let Some(handle) = cx.global::<Windows>().editor {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        cx.global_mut::<Windows>().editor = None;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(editor_bounds(cx))),
        titlebar: Some(TitlebarOptions {
            title: Some("wasurenai · 编辑".into()),
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
        window_min_size: Some(size(px(560.0), px(320.0))),
        ..Default::default()
    };

    match gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| EditorWindow::new(state, window, cx))
    }) {
        Ok((handle, _)) => {
            println!("[wasurenai] 编辑窗口已打开");
            cx.global_mut::<Windows>().editor = Some(handle);
        }
        Err(err) => eprintln!("[wasurenai] 编辑窗口打开失败：{err:#}"),
    }
}

/// 浮层位置：水平居中偏上，460x400。
fn overlay_bounds(cx: &mut App) -> Bounds<Pixels> {
    let screen = screen_bounds(cx);
    let (width, height) = (px(460.0), px(400.0));
    Bounds::new(
        point(
            screen.origin.x + (screen.size.width - width) * 0.5,
            screen.origin.y + screen.size.height * 0.12,
        ),
        size(width, height),
    )
}

/// 开浮层。已经开着就换成新的（不会开第二个）。
pub fn open_overlay(cx: &mut App, state: Entity<AppState>, mode: OverlayMode) {
    close_overlay(cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(overlay_bounds(cx))),
        titlebar: None,
        focus: true,
        show: true,
        // PopUp => WS_EX_TOPMOST | WS_EX_TOOLWINDOW：置顶且不进任务栏
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    match cx.open_window(options, move |window, cx| {
        window.set_window_title("wasurenai overlay");
        cx.new(|cx| OverlayWindow::new(state, mode, cx))
    }) {
        Ok(handle) => {
            println!("[wasurenai] 浮层已打开 mode={mode:?}");
            cx.global_mut::<Windows>().overlay = Some(handle.into());
        }
        Err(err) => eprintln!("[wasurenai] 浮层打开失败：{err:#}"),
    }
}

pub fn close_overlay(cx: &mut App) -> bool {
    let Some(handle) = cx.global_mut::<Windows>().overlay.take() else {
        return false;
    };
    let closed = handle
        .update(cx, |_, window, _| window.remove_window())
        .is_ok();
    if closed {
        println!("[wasurenai] 浮层已关闭");
    }
    closed
}

pub fn overlay_alive(cx: &mut App) -> bool {
    let Some(handle) = cx.global::<Windows>().overlay else {
        return false;
    };
    if handle.update(cx, |_, window, _| window.is_visible()).is_ok() {
        true
    } else {
        cx.global_mut::<Windows>().overlay = None;
        false
    }
}

/// 快捷键行为：没开就开（浏览模式），开着就关。
pub fn toggle_overlay(cx: &mut App, state: Entity<AppState>) {
    if overlay_alive(cx) {
        close_overlay(cx);
    } else {
        open_overlay(cx, state, OverlayMode::Browse);
    }
}

/// 定时检查到期提醒。
///
/// 规则（plan.md M4）：有逾期未确认的条目就必须弹；浮层已经是提醒模式且内容变化时
/// 让它自己重画；一条都没有了就把提醒浮层关掉（用户已经「知道了」或勾掉了）。
pub fn refresh_reminder(cx: &mut App, state: Entity<AppState>) {
    let now = chrono::Local::now().naive_local();
    let due_count = state.read(cx).due_items(now).len();
    let overlay_open = overlay_alive(cx);

    if due_count > 0 {
        if !overlay_open {
            open_overlay(cx, state, OverlayMode::Reminder);
        }
        return;
    }

    // 没有到期待办了：提醒浮层该收起来（浏览浮层不动）
    if overlay_open {
        let is_reminder = cx
            .global::<Windows>()
            .overlay
            .and_then(|handle| {
                handle
                    .read(cx, |overlay: Entity<OverlayWindow>, app: &App| {
                        overlay.read(app).mode()
                    })
                    .ok()
            })
            .map(|mode| mode == OverlayMode::Reminder)
            .unwrap_or(false);
        if is_reminder {
            close_overlay(cx);
        }
    }
}

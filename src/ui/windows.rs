//! 窗口生命周期与层级。所有函数都接收 `&mut App`，**绝不在 entity update 内部调用**
//! （窗口更新会同步触发绘制，进而 lease 视图 entity，见 `ui/mod.rs`）。

use gpui_kit::*;

use crate::platform::win_layer::{self, Layer};
use crate::ui::editor::EditorWindow;
use crate::ui::state::AppState;
use crate::ui::widget::WidgetWindow;

/// 窗口句柄注册表。gpui Global，任何拿到 `&mut App` 的地方都能访问。
#[derive(Default)]
pub struct Windows {
    pub widget: Option<AnyWindowHandle>,
    pub editor: Option<AnyWindowHandle>,
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

/// 收起 / 展开桌面层窗口（隐藏而不是销毁，内容保留）。
pub fn toggle_widget(cx: &mut App, state: Entity<AppState>) {
    let Some(handle) = cx.global::<Windows>().widget else {
        open_widget(cx, state);
        return;
    };

    let toggled = handle
        .update(cx, |_, window, _| {
            let hwnd = hwnd_of(window)?;
            let next = !window.is_visible();
            let ok = win_layer::set_visible(hwnd, next);
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

/// 快捷键：在「桌面层」和「置顶层」之间切换同一个窗口。
pub fn toggle_float(cx: &mut App, state: Entity<AppState>) {
    let floated = state.update(cx, |app, cx| {
        app.user_floated = !app.user_floated;
        cx.notify();
        app.user_floated
    });
    println!("[wasurenai] 快捷键：user_floated={floated}");
    sync_layer(cx, state);
}

/// 期望的层级：用户手动上浮 或 有到期提醒 → 置顶；否则沉底。
fn desired_layer(cx: &mut App, state: &Entity<AppState>) -> Layer {
    let app = state.read(cx);
    let now = chrono::Local::now().naive_local();
    if app.user_floated || !app.due_items(now).is_empty() {
        Layer::Top
    } else {
        Layer::Bottom
    }
}

/// 把窗口层级 / 可见性对齐到当前状态。幂等，可以每 500ms 调一次。
pub fn sync_layer(cx: &mut App, state: Entity<AppState>) {
    let Some(handle) = cx.global::<Windows>().widget else {
        return;
    };

    let target = desired_layer(cx, &state);
    // 到期提醒必须能看见：窗口被收起时先展开
    let must_show = target == Layer::Top;

    let applied = handle
        .update(cx, |_, window, _| {
            let hwnd = hwnd_of(window)?;
            if must_show && !window.is_visible() {
                win_layer::set_visible(hwnd, true);
            }
            if win_layer::current_layer() != target {
                let ok = win_layer::set_layer(hwnd, target);
                println!("[wasurenai] 层级 -> {target:?} ok={ok}");
            }
            Some(())
        })
        .ok()
        .flatten();

    if applied.is_none() {
        // 窗口没了（被关掉）：清掉句柄，下次 toggle 会重新打开
        cx.global_mut::<Windows>().widget = None;
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
            // 新建的窗口不一定会被系统提到前台（实测会被终端等窗口挡住），显式激活一次
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
        Err(err) => eprintln!("[wasurenai] 编辑窗口打开失败：{err:#}"),
    }
}

//! 窗口生命周期。所有函数都接收 `&mut App`，**绝不在 entity update 内部调用**
//! （`open_window` 会同步绘制新窗口并 lease 视图 entity，见 `ui/mod.rs`）。

use gpui_kit::*;

use crate::ui::state::AppState;
use crate::ui::widget::WidgetWindow;

/// 窗口句柄注册表。gpui Global，任何拿到 `&mut App` 的地方都能访问。
#[derive(Default)]
pub struct Windows {
    pub widget: Option<AnyWindowHandle>,
}

impl Global for Windows {}

/// 桌面层窗口的位置：主屏右下角，320x560，边距 24。
pub fn widget_bounds(cx: &mut App) -> Bounds<Pixels> {
    let screen = cx
        .primary_display()
        .map(|display| display.visible_bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))));

    let (width, height, margin) = (px(320.0), px(560.0), px(24.0));
    Bounds::new(
        point(
            screen.origin.x + screen.size.width - width - margin,
            screen.origin.y + screen.size.height - height - margin,
        ),
        size(width, height),
    )
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
        cx.new(|_| WidgetWindow::new(state))
    }) {
        Ok(handle) => {
            println!("[wasurenai] 桌面层窗口已打开 {handle:?}");
            cx.global_mut::<Windows>().widget = Some(handle.into());
        }
        Err(err) => eprintln!("[wasurenai] 桌面层窗口打开失败：{err:#}"),
    }
}

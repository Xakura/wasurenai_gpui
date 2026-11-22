//! 桌面层/浮层共用的小控件。
//!
//! 这两个窗口是"裸窗口"（没有 `gpui_base::Root`），而且用的是深色皮肤，
//! 所以不用 gpui-component 的 `Checkbox`（它跟随全局主题，浅色主题下在深色底上
//! 几乎看不见），这里手绘一个 16px 的方框。

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::theme::panel;

/// 手绘勾选框。`on_toggle` 收到点击后的目标状态。
pub fn tick_box(
    id: impl Into<ElementId>,
    done: bool,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .size(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if done {
            rgb(panel::ACCENT)
        } else {
            rgba(panel::BORDER)
        })
        .when(done, |box_| box_.bg(rgb(panel::ACCENT)))
        .flex()
        .items_center()
        .justify_center()
        .on_click(move |_event, window, cx| {
            on_toggle(!done, window, cx);
        })
        .when(done, |box_| {
            box_.child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(0xffffff))
                    .child("✓"),
            )
        })
}

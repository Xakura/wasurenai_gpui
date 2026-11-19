//! gpui 界面层。
//!
//! 硬约束（M0 spike 实测，详见 docs/plan.md §3.5）：
//!
//! - **窗口开关只能在 App 层做**：`App::open_window` 会同步绘制新窗口，绘制又会
//!   lease 视图 entity。在任何 entity update / render 内部开关窗口 → 同一 entity
//!   二次 lease → panic `cannot update ... while it is already being updated`。
//!   所以窗口句柄放 [`windows::Windows`]（gpui Global），open/close 函数的签名都是
//!   `fn xxx(cx: &mut App, ...)`。
//! - **透明窗口不能用 `gpui_kit::open_window`**：它会套一层 `gpui_base::Root`，
//!   而 gpui-component 的 Root 插件会把 surface 背景刷成不透明主题色。桌面层窗口
//!   用裸 `cx.open_window`。

pub mod editor;
pub mod state;
pub mod theme;
pub mod widget;
pub mod widgets;
pub mod windows;

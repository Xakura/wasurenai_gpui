//! 桌面层窗口的列表视图：勾选框 + 文本 + 截止时间 + 标签筛选条。
//!
//! 这个窗口是"裸窗口"（没有 `gpui_base::Root`），所以不能依赖 Root 提供的
//! Tooltip / Dialog / Popover / Notification；基础元素（`Checkbox` / `Label` 等）
//! 可以正常渲染。

use chrono::NaiveDateTime;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::ordering;
use crate::platform::win_bottom;
use crate::ui::state::AppState;
use crate::ui::theme;

pub struct WidgetWindow {
    state: Entity<AppState>,
    /// 第一帧做的窗口级初始化（置底）是否已执行。
    initialized: bool,
}

impl WidgetWindow {
    pub fn new(state: Entity<AppState>) -> Self {
        Self {
            state,
            initialized: false,
        }
    }

    /// 首帧：拿到 HWND 后把窗口压到桌面层。
    fn init_window(&mut self, window: &Window) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        match hwnd_of(window) {
            Some(hwnd) => {
                let ok = win_bottom::apply_bottom(hwnd);
                println!(
                    "[wasurenai] 桌面层置底={ok} 子类化={} hwnd={hwnd:?}",
                    win_bottom::subclassed()
                );
            }
            None => eprintln!("[wasurenai] 拿不到窗口 HWND，置底失败"),
        }
    }
}

impl Render for WidgetWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.init_window(window);

        let state = self.state.clone();
        let now = chrono::Local::now().naive_local();

        // 先把要画的数据全部复制出来，渲染时不再持有 AppState 的借用。
        let (rows, tags, active_tag) = {
            let app = state.read(cx);
            let rows = ordering::visible(&app.store)
                .into_iter()
                .map(|item| Row {
                    id: item.id,
                    text: item.text.clone(),
                    done: item.done,
                    due: item.due,
                    overdue: item.is_overdue(now),
                })
                .collect::<Vec<_>>();
            (
                rows,
                ordering::tags(&app.store.items),
                app.store.tag_filter.clone(),
            )
        };

        let empty = rows.is_empty();

        div()
            .size_full()
            .bg(rgba(theme::SURFACE))
            .rounded(px(theme::RADIUS))
            .p(px(theme::PADDING))
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(13.0))
            .text_color(rgb(theme::TEXT))
            // 标签筛选条：没有标签时不占地方
            .when(!tags.is_empty(), |panel| {
                panel.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(tag_chip(state.clone(), "全部".to_string(), None, active_tag.is_none()))
                        .children(tags.into_iter().map(|tag| {
                            let active = active_tag.as_deref() == Some(tag.as_str());
                            tag_chip(state.clone(), tag.clone(), Some(tag), active)
                        })),
                )
            })
            .child(
                div()
                    .id("todo-list")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .overflow_y_scroll()
                    .when(empty, |list| {
                        list.child(
                            div()
                                .text_color(rgb(theme::TEXT_MUTED))
                                .child(if active_tag.is_some() {
                                    "这个标签下没有待办"
                                } else {
                                    "暂无待办"
                                }),
                        )
                    })
                    .children(rows.into_iter().map(|row| todo_row(state.clone(), row))),
            )
    }
}

/// 一行要画的数据（从 `Store` 复制出来，避免渲染期间持有借用）。
struct Row {
    id: TodoId,
    text: String,
    done: bool,
    due: Option<NaiveDateTime>,
    overdue: bool,
}

fn todo_row(state: Entity<AppState>, row: Row) -> impl IntoElement {
    let id = row.id;
    let toggler = state;

    div()
        .flex()
        .items_start()
        .gap_2()
        .py(px(3.0))
        .child(
            Checkbox::new(format!("todo-done-{id}"))
                .checked(row.done)
                .on_change(move |checked, _window, cx| {
                    toggler.update(cx, |app, cx| {
                        app.set_done(id, *checked);
                        cx.notify();
                    });
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .flex_1()
                .child(
                    div()
                        .text_color(if row.done {
                            rgb(theme::TEXT_MUTED)
                        } else {
                            rgb(theme::TEXT)
                        })
                        .when(row.done, |text| text.line_through())
                        .child(row.text),
                )
                .when_some(row.due, |column, due| {
                    column.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(if row.overdue {
                                rgb(theme::DUE_OVERDUE)
                            } else {
                                rgb(theme::DUE)
                            })
                            .child(due.format("%m-%d %H:%M").to_string()),
                    )
                }),
        )
}

/// 标签筛选条上的一颗按钮。`tag = None` 表示「全部」。
fn tag_chip(
    state: Entity<AppState>,
    label: String,
    tag: Option<String>,
    active: bool,
) -> impl IntoElement {
    div()
        .id(format!("tag-chip-{label}"))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .bg(if active {
            rgba(theme::CHIP_ACTIVE)
        } else {
            rgba(theme::CHIP)
        })
        .text_size(px(11.0))
        .text_color(rgb(theme::CHIP_TEXT))
        .child(label)
        .on_click(move |_event, _window, cx| {
            state.update(cx, |app, cx| {
                match &tag {
                    Some(tag) => app.toggle_tag_filter(tag),
                    None => app.clear_tag_filter(),
                }
                cx.notify();
            });
        })
}

fn hwnd_of(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(windows::Win32::Foundation::HWND(
            win32.hwnd.get() as *mut core::ffi::c_void
        )),
        _ => None,
    }
}

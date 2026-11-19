//! 桌面层窗口的列表视图：勾选框 + 文本 + 截止时间 + 标签筛选条。
//!
//! 这个窗口是"裸窗口"（没有 `gpui_base::Root`），所以不能依赖 Root 提供的
//! Tooltip / Dialog / Popover / Notification；勾选框用 [`crate::ui::widgets::tick_box`] 手绘。

use chrono::NaiveDateTime;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::ordering;
use crate::platform::win_bottom;
use crate::ui::state::AppState;
use crate::ui::theme::panel;
use crate::ui::widgets::tick_box;
use crate::ui::windows::hwnd_of;

pub struct WidgetWindow {
    state: Entity<AppState>,
    /// 第一帧做的窗口级初始化（置底）是否已执行。
    initialized: bool,
}

impl WidgetWindow {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        // store 变化（例如在编辑窗口里改内容）时重画
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self {
            state,
            initialized: false,
        }
    }

    /// 首帧：把窗口压到桌面层。
    ///
    /// 置底动作（改 owner + 强制 HWND_BOTTOM）**延迟到首帧呈现之后**执行：
    /// 在首帧绘制过程中改 owner 会把 DirectComposition 的呈现目标搞坏，
    /// 表现为窗口 Z 序正常、`WindowFromPoint` 也能命中，但画面全空。
    fn init_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let Some(hwnd) = hwnd_of(window) else {
            eprintln!("[wasurenai] 拿不到窗口 HWND，置底失败");
            return;
        };
        let ok = win_bottom::apply_bottom(hwnd);
        println!(
            "[wasurenai] 桌面层置底={ok} 子类化={} hwnd={hwnd:?}",
            win_bottom::subclassed()
        );
        let _ = cx;
    }
}

impl Render for WidgetWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.init_window(window, cx);
        if std::env::var_os("WASURENAI_DEBUG").is_some() {
            println!(
                "[wasurenai] widget render visible={} bounds={:?}",
                window.is_visible(),
                window.bounds()
            );
        }

        let state = self.state.clone();
        let now = chrono::Local::now().naive_local();

        // 先把要画的数据全部复制出来，渲染时不再持有 AppState 的借用。
        let (rows, tags, active_tag, pending) = {
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
                ordering::pending_count(&app.store.items),
            )
        };

        let empty = rows.is_empty();

        div()
            .size_full()
            .bg(rgba(panel::BG))
            .rounded(px(panel::RADIUS))
            .p(px(panel::PADDING))
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(13.0))
            .text_color(rgb(panel::TEXT))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(panel::MUTED))
                            .child("待办"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(panel::ACCENT))
                            .child(format!("{pending} 项未完成")),
                    ),
            )
            // 标签筛选条：没有标签时不占地方
            .when(!tags.is_empty(), |panel_| {
                panel_.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(tag_chip(
                            state.clone(),
                            "全部".to_string(),
                            None,
                            active_tag.is_none(),
                        ))
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
                    .gap_0p5()
                    .flex_1()
                    .overflow_y_scroll()
                    .when(empty, |list| {
                        list.child(
                            div()
                                .pt(px(6.0))
                                .text_size(px(12.0))
                                .text_color(rgb(panel::MUTED))
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
        .id(("todo-row", id as usize))
        .flex()
        .items_center()
        .gap_2()
        .px(px(4.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(panel::ROW_HOVER)))
        .child(tick_box(("todo-done", id as usize), row.done, move |next, _window, cx| {
            toggler.update(cx, |app, cx| {
                let changed = app.set_done(id, next);
                app.commit_click(changed, cx);
            });
        }))
        .child(
            div()
                .flex_1()
                .text_color(if row.done {
                    rgb(panel::MUTED)
                } else {
                    rgb(panel::TEXT)
                })
                .when(row.done, |text| text.line_through())
                .child(row.text),
        )
        .when_some(row.due, |line, due| {
            line.child(
                div()
                    .text_size(px(11.0))
                    .text_color(if row.overdue {
                        rgb(panel::DANGER)
                    } else {
                        rgb(panel::MUTED)
                    })
                    .child(due.format("%m-%d %H:%M").to_string()),
            )
        })
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
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .bg(if active {
            rgba(panel::ACCENT)
        } else {
            rgba(panel::CHIP)
        })
        .text_size(px(11.0))
        .text_color(rgb(panel::CHIP_TEXT))
        .child(label)
        .on_click(move |_event, _window, cx| {
            state.update(cx, |app, cx| {
                let changed = match &tag {
                    Some(tag) => app.toggle_tag_filter(tag),
                    None => app.clear_tag_filter(),
                };
                app.commit_click(changed, cx);
            });
        })
}

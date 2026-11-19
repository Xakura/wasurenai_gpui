//! 桌面层 / 置顶层共用的待办视图。
//!
//! 同一个窗口有两种内容：
//! - **面板**：标签筛选条 + 全部待办（平常沉在桌面层）。
//! - **提醒**：有到期未确认的条目时自动切换过来，并让窗口上浮到最上层；
//!   逐条「知道了」/「全部知道了」/ 勾选完成后自动回到面板并回落桌面层。
//!
//! 这个窗口是"裸窗口"（没有 `gpui_base::Root`），所以不能依赖 Root 提供的
//! Tooltip / Dialog / Popover / Notification；勾选框用 [`crate::ui::widgets::tick_box`] 手绘。

use chrono::NaiveDateTime;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::ordering;
use crate::platform::win_layer;
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

    /// 首帧：接管窗口（owner + 子类化 + 压到桌面层）。
    fn init_window(&mut self, window: &Window) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let Some(hwnd) = hwnd_of(window) else {
            eprintln!("[wasurenai] 拿不到窗口 HWND，置底失败");
            return;
        };
        let ok = win_layer::attach(hwnd);
        println!(
            "[wasurenai] 桌面层接管={ok} 子类化={} hwnd={hwnd:?}",
            win_layer::subclassed()
        );
    }
}

impl Render for WidgetWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.init_window(window);

        let state = self.state.clone();
        let now = chrono::Local::now().naive_local();

        let (rows, tags, active_tag, pending, reminder) = {
            let app = state.read(cx);
            let due = app.due_items(now);
            let reminder = !due.is_empty();
            let source: Vec<Row> = if reminder {
                due.into_iter()
                    .map(|item| Row::from_item(item, now))
                    .collect()
            } else {
                ordering::visible(&app.store)
                    .into_iter()
                    .map(|item| Row::from_item(item, now))
                    .collect()
            };
            (
                source,
                ordering::tags(&app.store.items),
                app.store.tag_filter.clone(),
                ordering::pending_count(&app.store.items),
                reminder,
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
            .child(if reminder {
                reminder_header(state.clone(), rows.len())
            } else {
                panel_header(pending)
            })
            // 标签筛选条只在面板模式下出现
            .when(!reminder && !tags.is_empty(), |body| {
                body.child(
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
                                .child(if reminder {
                                    "没有到期的待办"
                                } else if active_tag.is_some() {
                                    "这个标签下没有待办"
                                } else {
                                    "暂无待办"
                                }),
                        )
                    })
                    .children(
                        rows.into_iter()
                            .map(|row| todo_row(state.clone(), row, reminder)),
                    ),
            )
    }
}

fn panel_header(pending: usize) -> AnyElement {
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
        )
        .into_any_element()
}

fn reminder_header(state: Entity<AppState>, count: usize) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(panel::DANGER))
                .child(format!("到期提醒 · {count} 项")),
        )
        .child(div().flex_1())
        .child(
            div()
                .id("ack-all")
                .px(px(7.0))
                .py(px(2.0))
                .rounded(px(5.0))
                .bg(rgba(panel::ACCENT))
                .text_size(px(11.0))
                .text_color(rgb(0xffffff))
                .child("全部知道了")
                .on_click(move |_event, _window, cx| {
                    state.update(cx, |app, cx| {
                        app.ack_all(chrono::Local::now().naive_local());
                        app.commit_runtime(true, cx);
                    });
                }),
        )
        .into_any_element()
}

/// 一行要画的数据（从 `Store` 复制出来，避免渲染期间持有借用）。
struct Row {
    id: TodoId,
    text: String,
    done: bool,
    due: Option<NaiveDateTime>,
    overdue: bool,
}

impl Row {
    fn from_item(item: &crate::core::model::TodoItem, now: NaiveDateTime) -> Self {
        Self {
            id: item.id,
            text: item.text.clone(),
            done: item.done,
            due: item.due,
            overdue: item.is_overdue(now),
        }
    }
}

fn todo_row(state: Entity<AppState>, row: Row, reminder: bool) -> impl IntoElement {
    let id = row.id;
    let toggler = state.clone();
    let acker = state;

    div()
        .id(("todo-row", id as usize))
        .flex()
        .items_center()
        .gap_2()
        .px(px(4.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(panel::ROW_HOVER)))
        .child(tick_box(
            ("todo-done", id as usize),
            row.done,
            move |next, _window, cx| {
                toggler.update(cx, |app, cx| {
                    let changed = app.set_done(id, next);
                    app.commit_click(changed, cx);
                });
            },
        ))
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
        .when(reminder, |line| {
            line.child(
                div()
                    .id(("todo-ack", id as usize))
                    .px(px(7.0))
                    .py(px(1.0))
                    .rounded(px(5.0))
                    .bg(rgba(panel::CHIP))
                    .text_size(px(11.0))
                    .text_color(rgb(panel::CHIP_TEXT))
                    .child("知道了")
                    .on_click(move |_event, _window, cx| {
                        acker.update(cx, |app, cx| {
                            app.ack(id);
                            app.commit_runtime(true, cx);
                        });
                    }),
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

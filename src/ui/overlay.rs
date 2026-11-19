//! 置顶浮层：快捷键唤起时看全量列表，到期提醒时看逾期项。
//!
//! 和桌面层一样是裸窗口（`WindowKind::PopUp` 自带 `WS_EX_TOPMOST | WS_EX_TOOLWINDOW`，
//! 不进任务栏），所以同样手绘勾选框。

use chrono::NaiveDateTime;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::ordering;
use crate::ui::state::AppState;
use crate::ui::theme::panel;
use crate::ui::widgets::tick_box;

/// 浮层当前展示什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    /// 快捷键叫出来的全量列表。
    Browse,
    /// 到期提醒（只列逾期未确认的条目，必须逐个「知道了」）。
    Reminder,
}

pub struct OverlayWindow {
    state: Entity<AppState>,
    mode: OverlayMode,
}

impl OverlayWindow {
    pub fn new(state: Entity<AppState>, mode: OverlayMode, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state, mode }
    }

    pub fn mode(&self) -> OverlayMode {
        self.mode
    }
}

impl Render for OverlayWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let now = chrono::Local::now().naive_local();

        let (rows, title) = {
            let app = state.read(cx);
            match self.mode {
                OverlayMode::Browse => {
                    let rows = ordering::visible(&app.store)
                        .into_iter()
                        .map(|item| Row::from_item(item, now))
                        .collect::<Vec<_>>();
                    let title = format!("待办 · {} 项", rows.len());
                    (rows, title)
                }
                OverlayMode::Reminder => {
                    let rows = app
                        .due_items(now)
                        .into_iter()
                        .map(|item| Row::from_item(item, now))
                        .collect::<Vec<_>>();
                    let title = format!("到期提醒 · {} 项", rows.len());
                    (rows, title)
                }
            }
        };

        let is_reminder = self.mode == OverlayMode::Reminder;
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
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(panel::MUTED))
                            .child(title),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("overlay-close")
                            .px(px(6.0))
                            .py(px(1.0))
                            .rounded(px(5.0))
                            .text_size(px(12.0))
                            .text_color(rgb(panel::MUTED))
                            .hover(|style| style.bg(rgba(panel::ROW_HOVER)))
                            .child("×")
                            .on_click(|_event, window, _cx| {
                                window.remove_window();
                            }),
                    )
                    .when(is_reminder && !empty, |line| {
                        let ack_state = state.clone();
                        line.child(
                            div()
                                .id("ack-all")
                                .px(px(8.0))
                                .py(px(2.0))
                                .rounded(px(5.0))
                                .bg(rgba(panel::ACCENT))
                                .text_size(px(11.0))
                                .text_color(rgb(0xffffff))
                                .child("全部知道了")
                                .on_click(move |_event, _window, cx| {
                                    ack_state.update(cx, |app, cx| {
                                        app.ack_all(chrono::Local::now().naive_local());
                                        app.commit_runtime(true, cx);
                                    });
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .id("overlay-list")
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .flex_1()
                    .overflow_y_scroll()
                    .when(empty, |list| {
                        list.child(
                            div()
                                .py(px(6.0))
                                .text_size(px(12.0))
                                .text_color(rgb(panel::MUTED))
                                .child(if is_reminder {
                                    "没有到期的待办"
                                } else {
                                    "暂无待办"
                                }),
                        )
                    })
                    .children(rows.into_iter().map(|row| {
                        overlay_row(state.clone(), row, is_reminder)
                    })),
            )
    }
}

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

fn overlay_row(state: Entity<AppState>, row: Row, reminder: bool) -> impl IntoElement {
    let id = row.id;
    let toggler = state.clone();
    let acker = state;

    div()
        .id(("overlay-row", id as usize))
        .flex()
        .items_center()
        .gap_2()
        .px(px(4.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(panel::ROW_HOVER)))
        .child(tick_box(
            ("overlay-done", id as usize),
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
                    .id(("overlay-ack", id as usize))
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

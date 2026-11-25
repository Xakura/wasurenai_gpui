//! 编辑窗口「日志」页签：按日期分组预览全部已完成工作，支持打开日志、定位目录与导出副本。

use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::theme::editor as palette;

use super::components::{count_badge, secondary_button};
use super::{EditorWindow, PAD_X};

pub(super) struct LogItemSnapshot {
    pub(super) text: String,
    pub(super) time: Option<String>,
    pub(super) tags: Vec<String>,
}

pub(super) struct LogDaySnapshot {
    pub(super) title: String,
    pub(super) badge: Option<&'static str>,
    pub(super) items: Vec<LogItemSnapshot>,
}

impl EditorWindow {
    pub(super) fn render_log(
        &mut self,
        cx: &mut Context<Self>,
        total_completed: usize,
        days: Vec<LogDaySnapshot>,
        log_path: &str,
    ) -> AnyElement {
        let notice = self.log_notice.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            // 顶部固定日志操作卡片
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(PAD_X))
                    .pt(px(16.0))
                    .pb(px(12.0))
                    .child(
                        div()
                            .bg(rgb(palette::SURFACE))
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(rgba(palette::BORDER))
                            .p(px(14.0))
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_size(px(14.0))
                                                            .font_semibold()
                                                            .text_color(rgb(palette::TEXT))
                                                            .child("已完成工作日志"),
                                                    )
                                                    .child(count_badge(total_completed, true)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(rgb(palette::SUBTLE))
                                                    .child("每次完成事项自动按时间顺序同步到本地 Markdown 文件"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(secondary_button(
                                                "log-open-file",
                                                "打开日志文件",
                                                cx.listener(|this, _, _, cx| {
                                                    this.open_log_file(cx);
                                                }),
                                            ))
                                            .child(secondary_button(
                                                "log-reveal-dir",
                                                "打开目录",
                                                cx.listener(|this, _, _, cx| {
                                                    this.reveal_log_dir(cx);
                                                }),
                                            ))
                                            .child(
                                                div()
                                                    .id("log-export-btn")
                                                    .px(px(12.0))
                                                    .py(px(5.0))
                                                    .rounded(px(7.0))
                                                    .bg(rgb(palette::ACCENT))
                                                    .hover(|s| s.bg(rgb(palette::ACCENT_HOVER)))
                                                    .text_size(px(12.0))
                                                    .font_medium()
                                                    .text_color(rgb(palette::SURFACE))
                                                    .cursor_pointer()
                                                    .child("导出副本…")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.export_log_as(cx);
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(10.0))
                                    .py(px(5.0))
                                    .rounded(px(6.0))
                                    .bg(rgb(palette::SURFACE_SUBTLE))
                                    .border_1()
                                    .border_color(rgba(palette::BORDER))
                                    .text_size(px(11.5))
                                    .text_color(rgb(palette::MUTED))
                                    .child(format!("自动日志路径：{log_path}")),
                            )
                            .when_some(notice, |card, (is_err, msg)| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(if is_err {
                                            rgba(palette::DANGER_SOFT)
                                        } else {
                                            rgba(palette::ACCENT_SOFT)
                                        })
                                        .text_size(px(11.5))
                                        .text_color(if is_err {
                                            rgb(palette::DANGER)
                                        } else {
                                            rgb(palette::ACCENT_TEXT)
                                        })
                                        .child(msg),
                                )
                            }),
                    ),
            )
            // 可滚动日志预览区
            .child(
                div()
                    .id("log-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(PAD_X))
                    .pb(px(20.0))
                    .gap(px(16.0))
                    .when(days.is_empty(), |body| body.child(render_empty_log_card()))
                    .children(days.into_iter().map(render_log_day_section)),
            )
            .into_any_element()
    }
}

fn render_log_day_section(day: LogDaySnapshot) -> AnyElement {
    let count = day.items.len();
    let mut card = div()
        .bg(rgb(palette::SURFACE))
        .rounded(px(12.0))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .overflow_hidden()
        .flex()
        .flex_col();

    for (index, item) in day.items.into_iter().enumerate() {
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(14.0))
                .py(px(9.0))
                .when(index > 0, |el| {
                    el.border_t_1().border_color(rgba(palette::BORDER))
                })
                .child(
                    div()
                        .w(px(42.0))
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(rgb(palette::SUBTLE))
                        .child(item.time.unwrap_or_else(|| "—".to_string())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.0))
                        .text_color(rgb(palette::TEXT))
                        .child(item.text),
                )
                .when(!item.tags.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(4.0))
                            .children(item.tags.into_iter().map(|tag| {
                                div()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(5.0))
                                    .bg(rgba(palette::TAG_BG))
                                    .text_size(px(11.0))
                                    .text_color(rgb(palette::TAG_TEXT))
                                    .child(format!("#{tag}"))
                            })),
                    )
                }),
        );
    }

    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px(px(4.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_semibold()
                        .text_color(rgb(palette::TEXT))
                        .child(day.title),
                )
                .when_some(day.badge, |row, label| {
                    row.child(
                        div()
                            .px(px(6.0))
                            .py(px(0.5))
                            .rounded(px(5.0))
                            .bg(rgba(palette::ACCENT_SOFT))
                            .text_size(px(10.5))
                            .font_medium()
                            .text_color(rgb(palette::ACCENT_TEXT))
                            .child(label),
                    )
                })
                .child(count_badge(count, false)),
        )
        .child(card)
        .into_any_element()
}

fn render_empty_log_card() -> AnyElement {
    div()
        .flex_shrink_0()
        .bg(rgb(palette::SURFACE))
        .rounded(px(12.0))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .py(px(42.0))
        .px(px(24.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .size(px(40.0))
                .rounded(px(20.0))
                .bg(rgba(palette::ACCENT_SOFT))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(18.0))
                .font_bold()
                .text_color(rgb(palette::ACCENT))
                .child("✓"),
        )
        .child(
            div()
                .text_size(px(14.0))
                .font_semibold()
                .text_color(rgb(palette::TEXT))
                .child("还没有已完成的工作记录"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(palette::SUBTLE))
                .child("勾选待办完成后，会自动按时间顺序记录到这里并同步到 Markdown 文件"),
        )
        .into_any_element()
}

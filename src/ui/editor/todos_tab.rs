//! 编辑窗口「待办」页签：快速添加卡片、进行中分组与今日已完成可折叠分组。

use std::collections::HashMap;

use chrono::NaiveDate;
use gpui_kit::component::date_picker::DateRangePreset;
use gpui_kit::component::input::Input;
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::ui::theme::editor as palette;

use super::components::{checkable_tag_pill, count_badge, due_presets, render_due_picker};
use super::{COL_DUE, COL_TAGS, EditorWindow, PAD_X, RowEditors, RowSnapshot, Tab};

impl EditorWindow {
    pub(super) fn render_todos(
        &mut self,
        cx: &mut Context<Self>,
        snapshots: &HashMap<TodoId, RowSnapshot>,
        predefined_tags: &[String],
        today: NaiveDate,
    ) -> AnyElement {
        let mut active_ids = Vec::new();
        let mut done_ids = Vec::new();
        for row in &self.rows {
            let is_done = snapshots.get(&row.id).is_some_and(|s| s.done);
            if is_done {
                done_ids.push(row.id);
            } else {
                active_ids.push(row.id);
            }
        }

        let total_empty = self.rows.is_empty();
        let show_completed = self.show_completed;
        let presets = due_presets(today);

        // 顶部快速添加卡片固定在滚动列表上方（flex_shrink_0），待办过多时绝不被挤占
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(PAD_X))
                    .pt(px(16.0))
                    .pb(px(12.0))
                    .child(self.render_quick_add_card(&presets, predefined_tags, cx)),
            )
            .child(
                div()
                    .id("todos-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(PAD_X))
                    .pb(px(20.0))
                    .gap(px(16.0))
                    .when(total_empty, |body| body.child(render_empty_card()))
                    .when(!active_ids.is_empty(), |body| {
                        body.child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .px(px(4.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_semibold()
                                                        .text_color(rgb(palette::TEXT))
                                                        .child("进行中"),
                                                )
                                                .child(count_badge(active_ids.len(), true)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(rgb(palette::SUBTLE))
                                                .child("仅展示未完成与今日完成事项，历史完成自动归入日志"),
                                        ),
                                )
                                .child(self.render_todo_card(
                                    &active_ids,
                                    snapshots,
                                    predefined_tags,
                                    &presets,
                                    cx,
                                )),
                        )
                    })
                    .when(!done_ids.is_empty(), |body| {
                        let done_count = done_ids.len();
                        body.child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .px(px(4.0))
                                        .child(
                                            div()
                                                .id("toggle-completed")
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .cursor_pointer()
                                                .hover(|s| s.opacity(0.75))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.toggle_completed_group(cx);
                                                }))
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(rgb(palette::MUTED))
                                                        .child(if show_completed {
                                                            "▾"
                                                        } else {
                                                            "▸"
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_semibold()
                                                        .text_color(rgb(palette::MUTED))
                                                        .child("今日已完成"),
                                                )
                                                .child(count_badge(done_count, false)),
                                        )
                                        .child(
                                            div()
                                                .id("goto-log-link")
                                                .px(px(8.0))
                                                .py(px(3.0))
                                                .rounded(px(6.0))
                                                .text_size(px(11.5))
                                                .text_color(rgb(palette::ACCENT))
                                                .cursor_pointer()
                                                .hover(|s| s.bg(rgba(palette::ACCENT_SOFT)))
                                                .child("查看完整日志 →")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.set_tab(Tab::Log, cx);
                                                })),
                                        ),
                                )
                                .when(show_completed, |group| {
                                    group.child(self.render_todo_card(
                                        &done_ids,
                                        snapshots,
                                        predefined_tags,
                                        &presets,
                                        cx,
                                    ))
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_quick_add_card(
        &self,
        presets: &[DateRangePreset],
        predefined_tags: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_shrink_0()
            .bg(rgb(palette::SURFACE))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(palette::BORDER))
            .overflow_hidden()
            .flex()
            .flex_col()
            // 主输入行
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(8.0))
                    .child(
                        div()
                            .size(px(22.0))
                            .flex_none()
                            .rounded(px(6.0))
                            .bg(rgba(palette::ACCENT_SOFT))
                            .text_color(rgb(palette::ACCENT))
                            .text_size(px(14.0))
                            .font_semibold()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child("+"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(rgb(palette::TEXT))
                            .child(
                                Input::new(&self.quick_text)
                                    .appearance(false)
                                    .px(px(4.0))
                                    .py(px(0.0))
                                    .h(px(28.0))
                                    .text_size(px(13.5)),
                            ),
                    )
                    .child(div().w(px(1.0)).h(px(18.0)).bg(rgba(palette::BORDER)))
                    .child(
                        div()
                            .w(px(COL_DUE))
                            .flex_none()
                            .px(px(4.0))
                            .py(px(2.0))
                            .rounded(px(7.0))
                            .bg(rgb(palette::SURFACE_SUBTLE))
                            .border_1()
                            .border_color(rgba(palette::BORDER))
                            .text_color(rgb(palette::TEXT))
                            .child(render_due_picker(&self.quick_due, presets)),
                    )
                    .child(
                        div()
                            .id("quick-add-btn")
                            .flex_none()
                            .px(px(14.0))
                            .h(px(30.0))
                            .rounded(px(8.0))
                            .bg(rgb(palette::ACCENT))
                            .hover(|s| s.bg(rgb(palette::ACCENT_HOVER)))
                            .text_size(px(12.5))
                            .font_medium()
                            .text_color(rgb(palette::SURFACE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child("添加")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_quick_add(window, cx);
                            })),
                    ),
            )
            // 预定义标签多选勾选栏
            .child(
                div()
                    .px(px(14.0))
                    .py(px(7.0))
                    .bg(rgb(palette::SURFACE_SUBTLE))
                    .border_t_1()
                    .border_color(rgba(palette::BORDER))
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(palette::SUBTLE))
                            .mr(px(2.0))
                            .child("标签"),
                    )
                    .when(predefined_tags.is_empty(), |bar| {
                        bar.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(rgb(palette::SUBTLE))
                                .child("尚未预定义标签"),
                        )
                        .child(
                            div()
                                .id("quick-manage-tags-empty")
                                .px(px(7.0))
                                .py(px(1.5))
                                .rounded(px(5.0))
                                .text_size(px(11.5))
                                .text_color(rgb(palette::ACCENT))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgba(palette::ACCENT_SOFT)))
                                .child("去设置添加 →")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_tab(Tab::Settings, cx);
                                })),
                        )
                    })
                    .when(!predefined_tags.is_empty(), |bar| {
                        bar.children(predefined_tags.iter().enumerate().map(|(idx, tag)| {
                            let selected = self.quick_tags.iter().any(|t| t == tag);
                            let tag_owned = tag.clone();
                            checkable_tag_pill(
                                ("quick-tag", idx),
                                tag,
                                selected,
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_quick_tag(&tag_owned, cx);
                                }),
                            )
                        }))
                        .child(
                            div()
                                .id("quick-manage-tags")
                                .ml(px(4.0))
                                .px(px(6.0))
                                .py(px(1.5))
                                .rounded(px(5.0))
                                .text_size(px(11.0))
                                .text_color(rgb(palette::SUBTLE))
                                .cursor_pointer()
                                .hover(|s| {
                                    s.bg(rgba(palette::HOVER))
                                        .text_color(rgb(palette::TEXT))
                                })
                                .child("管理")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_tab(Tab::Settings, cx);
                                })),
                        )
                    }),
            )
            .when_some(self.quick_error.clone(), |card, message| {
                card.child(
                    div()
                        .px(px(14.0))
                        .py(px(6.0))
                        .bg(rgba(palette::DANGER_SOFT))
                        .border_t_1()
                        .border_color(rgba(palette::BORDER))
                        .text_size(px(11.5))
                        .text_color(rgb(palette::DANGER))
                        .child(message),
                )
            })
            .into_any_element()
    }

    fn render_todo_card(
        &self,
        ids: &[TodoId],
        snapshots: &HashMap<TodoId, RowSnapshot>,
        predefined_tags: &[String],
        presets: &[DateRangePreset],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut card = div()
            .flex_shrink_0()
            .bg(rgb(palette::SURFACE))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(palette::BORDER))
            .overflow_hidden()
            .flex()
            .flex_col();

        for (index, &id) in ids.iter().enumerate() {
            let Some(row) = self.rows.iter().find(|r| r.id == id) else {
                continue;
            };
            let snap = snapshots.get(&id).cloned().unwrap_or(RowSnapshot {
                id,
                done: false,
                overdue: false,
                tags: Vec::new(),
            });
            card = card.child(self.render_todo_row(
                row,
                snap,
                predefined_tags,
                presets,
                index > 0,
                cx,
            ));
        }

        card.into_any_element()
    }

    fn render_todo_row(
        &self,
        row: &RowEditors,
        snap: RowSnapshot,
        predefined_tags: &[String],
        presets: &[DateRangePreset],
        with_top_divider: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = snap.id;
        let done = snap.done;
        let overdue = snap.overdue;
        let has_due = row.due.read(cx).date_time().start().is_some();
        let picker_open = self.editing_tags_for == Some(id);
        let toggler = self.state.clone();

        let (due_bg, due_fg) = if overdue {
            (rgba(palette::DANGER_SOFT), rgb(palette::DANGER))
        } else if has_due {
            (
                rgba(palette::DUE_BG),
                if done {
                    rgb(palette::MUTED)
                } else {
                    rgb(palette::TEXT)
                },
            )
        } else {
            (rgba(0x00000000), rgb(palette::SUBTLE))
        };

        // 展开行内标签选择器时，合并预定义标签 + 该条已有标签（防止已被删出预设的旧标签无法取消）
        let mut picker_options: Vec<String> = predefined_tags.to_vec();
        for existing in &snap.tags {
            if !picker_options.contains(existing) {
                picker_options.push(existing.clone());
            }
        }

        div()
            .flex()
            .flex_col()
            .when(with_top_divider, |el| {
                el.border_t_1().border_color(rgba(palette::BORDER))
            })
            .child(
                div()
                    .id(("editor-row", id as usize))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(8.0))
                    .hover(|style| style.bg(rgba(palette::HOVER)))
                    // 18×18 圆形勾选框
                    .child(
                        div()
                            .id(("edit-done", id as usize))
                            .size(px(18.0))
                            .flex_none()
                            .rounded(px(9.0))
                            .border_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(done, |box_| {
                                box_.bg(rgb(palette::ACCENT))
                                    .border_color(rgb(palette::ACCENT))
                                    .hover(|s| s.bg(rgb(palette::ACCENT_HOVER)))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_bold()
                                            .text_color(rgb(palette::SURFACE))
                                            .child("✓"),
                                    )
                            })
                            .when(!done, |box_| {
                                box_.bg(rgb(palette::SURFACE))
                                    .border_color(rgba(palette::BORDER_STRONG))
                                    .hover(|s| {
                                        s.border_color(rgb(palette::ACCENT))
                                            .bg(rgba(palette::ACCENT_SOFT))
                                    })
                            })
                            .on_click(move |_, _, cx| {
                                toggler.update(cx, |app, cx| {
                                    let changed = app.set_done(id, !done);
                                    app.commit_click(changed, cx);
                                });
                            }),
                    )
                    // 内容输入
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(if done {
                                rgb(palette::MUTED)
                            } else {
                                rgb(palette::TEXT)
                            })
                            .child(
                                Input::new(&row.text)
                                    .appearance(false)
                                    .px(px(4.0))
                                    .py(px(0.0))
                                    .h(px(26.0))
                                    .text_size(px(13.5)),
                            ),
                    )
                    // 截止时间选择器胶囊
                    .child(
                        div()
                            .w(px(COL_DUE))
                            .flex_none()
                            .px(px(4.0))
                            .py(px(1.0))
                            .rounded(px(6.0))
                            .bg(due_bg)
                            .text_color(due_fg)
                            .hover(|s| {
                                if overdue {
                                    s.bg(rgba(palette::DANGER_SOFT))
                                } else {
                                    s.bg(rgba(palette::DUE_BG))
                                }
                            })
                            .child(render_due_picker(&row.due, presets)),
                    )
                    // 多标签展示与展开选择按钮（固定列宽，避免不同标签数导致左侧截止时间列错位）
                    .child(
                        div()
                            .id(("row-tags-trigger", id as usize))
                            .w(px(COL_TAGS))
                            .flex_none()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(4.0))
                            .px(px(6.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .when(picker_open, |el| el.bg(rgba(palette::ACCENT_SOFT)))
                            .when(!picker_open, |el| el.hover(|s| s.bg(rgba(palette::DUE_BG))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_row_tag_picker(id, cx);
                            }))
                            .when(snap.tags.is_empty(), |area| {
                                area.child(
                                    div()
                                        .px(px(6.0))
                                        .py(px(1.0))
                                        .rounded(px(5.0))
                                        .bg(rgba(0x00000007))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::SUBTLE))
                                        .child("+ 标签"),
                                )
                            })
                            .when(!snap.tags.is_empty(), |area| {
                                area.children(snap.tags.iter().map(|tag| {
                                    div()
                                        .px(px(6.0))
                                        .py(px(1.0))
                                        .rounded(px(5.0))
                                        .bg(rgba(palette::TAG_BG))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::TAG_TEXT))
                                        .child(format!("#{tag}"))
                                }))
                            }),
                    )
                    // 安静删除按钮
                    .child(
                        div()
                            .id(("editor-del", id as usize))
                            .size(px(26.0))
                            .flex_none()
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.0))
                            .text_color(rgb(palette::SUBTLE))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgba(palette::DANGER_SOFT))
                                    .text_color(rgb(palette::DANGER))
                            })
                            .child("×")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.delete_row(id, cx);
                            })),
                    ),
            )
            // 展开的行内多选标签抽屉
            .when(picker_open, |wrapper| {
                wrapper.child(
                    div()
                        .px(px(42.0))
                        .py(px(8.0))
                        .bg(rgb(palette::SURFACE_SUBTLE))
                        .border_t_1()
                        .border_color(rgba(palette::BORDER))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(rgb(palette::SUBTLE))
                                .child("勾选标签："),
                        )
                        .when(picker_options.is_empty(), |tray| {
                            tray.child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(palette::SUBTLE))
                                    .child("还没有预定义标签"),
                            )
                        })
                        .children(picker_options.into_iter().enumerate().map(|(idx, tag)| {
                            let checked = snap.tags.iter().any(|t| t == &tag);
                            let tag_owned = tag.clone();
                            checkable_tag_pill(
                                ("row-tag-opt", (id as usize) * 1024 + idx),
                                &tag,
                                checked,
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_row_tag(id, &tag_owned, cx);
                                }),
                            )
                        }))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id(("row-tag-manage", id as usize))
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(5.0))
                                .text_size(px(11.5))
                                .text_color(rgb(palette::ACCENT))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgba(palette::ACCENT_SOFT)))
                                .child("管理标签")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_tab(Tab::Settings, cx);
                                })),
                        )
                        .child(
                            div()
                                .id(("row-tag-done", id as usize))
                                .px(px(8.0))
                                .py(px(2.0))
                                .rounded(px(5.0))
                                .bg(rgb(palette::SURFACE))
                                .border_1()
                                .border_color(rgba(palette::BORDER_STRONG))
                                .text_size(px(11.5))
                                .text_color(rgb(palette::TEXT))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgba(palette::HOVER)))
                                .child("收起")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_row_tag_picker(id, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

fn render_empty_card() -> AnyElement {
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
                .child("还没有待办事项"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(palette::SUBTLE))
                .child("在上方输入框输入内容，按 Enter 立即添加"),
        )
        .into_any_element()
}

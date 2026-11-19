//! 编辑窗口：结构化行编辑（每行 = 勾选框 + 内容 + 截止时间 + 标签 + 删除）。
//!
//! 这个窗口走 `gpui_kit::open_window`（有 `Root`），所以可以用 gpui-component 的
//! 输入框/按钮；桌面层窗口则不行（见 `ui/mod.rs`）。
//!
//! 输入框是 entity（`InputState`），每一行的三个输入框在行创建时生成一次并常驻，
//! 行只在「新增/删除」时增删——不在 render 里创建 entity。

use chrono::Local;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::timeparse;
use crate::ui::state::AppState;
use crate::ui::theme::editor as palette;

/// 新建待办时填入的占位内容（`Store::add` 不接受空文本）。
const NEW_ITEM_TEXT: &str = "新待办";

pub struct EditorWindow {
    state: Entity<AppState>,
    rows: Vec<RowEditors>,
}

struct RowEditors {
    id: TodoId,
    text: Entity<InputState>,
    due: Entity<InputState>,
    tag: Entity<InputState>,
    /// 截止时间解析失败的提示，`None` = 正常。
    due_error: Option<String>,
}

impl EditorWindow {
    pub fn new(
        state: Entity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            state: state.clone(),
            rows: Vec::new(),
        };

        // 桌面层窗口里勾选/改标签时，编辑窗口也要跟着重画
        cx.observe(&state, |_, _, cx| cx.notify()).detach();

        this.sync_rows(window, cx);
        this
    }

    /// 让行与 store 对齐：新增缺失的行，丢掉已删除的行。
    fn sync_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<TodoId> = {
            let app = self.state.read(cx);
            let mut ids: Vec<TodoId> = app.store.items.iter().map(|item| item.id).collect();
            ids.sort_unstable();
            ids
        };

        self.rows.retain(|row| ids.contains(&row.id));

        for id in ids {
            if self.rows.iter().any(|row| row.id == id) {
                continue;
            }
            if let Some(row) = self.build_row(id, window, cx) {
                self.rows.push(row);
            }
        }
        self.rows.sort_by_key(|row| row.id);
    }

    fn build_row(
        &mut self,
        id: TodoId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<RowEditors> {
        let (text, due, tag) = {
            let app = self.state.read(cx);
            let item = app.store.item(id)?;
            (
                item.text.clone(),
                timeparse::format_due(item.due),
                item.tag.clone().unwrap_or_default(),
            )
        };

        let text_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("做什么")
                .default_value(text)
        });
        let due_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("2026-11-20 18:00")
                .default_value(due)
        });
        let tag_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("标签")
                .default_value(tag)
        });

        // 三个输入框的变化都直接写进 store（并落盘）
        cx.subscribe(&text_input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_text_changed(id, cx);
            }
        })
        .detach();

        cx.subscribe(&due_input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_due_changed(id, cx);
            }
        })
        .detach();

        cx.subscribe(&tag_input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_tag_changed(id, cx);
            }
        })
        .detach();

        Some(RowEditors {
            id,
            text: text_input,
            due: due_input,
            tag: tag_input,
            due_error: None,
        })
    }

    fn on_text_changed(&mut self, id: TodoId, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.id == id) else {
            return;
        };
        let text = row.text.read(cx).value().to_string();
        self.state.update(cx, |app, cx| {
            let changed = app.set_text(id, &text);
            app.commit_typing(changed, cx);
        });
    }

    fn on_tag_changed(&mut self, id: TodoId, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.id == id) else {
            return;
        };
        let tag = row.tag.read(cx).value().to_string();
        self.state.update(cx, |app, cx| {
            let changed = app.set_tag(id, Some(tag.as_str()));
            app.commit_typing(changed, cx);
        });
    }

    fn on_due_changed(&mut self, id: TodoId, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.id == id) else {
            return;
        };
        let raw = row.due.read(cx).value().to_string();
        let today = Local::now().date_naive();

        match timeparse::parse_due(&raw, today) {
            Ok(due) => {
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.due_error = None;
                }
                self.state.update(cx, |app, cx| {
                    let changed = app.set_due(id, due);
                    app.commit_typing(changed, cx);
                });
            }
            Err(message) => {
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.due_error = Some(message);
                }
            }
        }
        cx.notify();
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.state.update(cx, |app, cx| {
            let id = app.add_item(NEW_ITEM_TEXT);
            app.commit_click(id.is_some(), cx);
            id
        }) else {
            return;
        };
        self.sync_rows(window, cx);
        // 新行放在最后并滚动到可见区域
        if let Some(row) = self.rows.iter().find(|row| row.id == id) {
            let handle = row.text.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        cx.notify();
    }

    fn delete_row(&mut self, id: TodoId, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.remove_item(id);
            app.commit_click(changed, cx);
        });
        self.rows.retain(|row| row.id != id);
        cx.notify();
    }
}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 防御：外部新增/删除了条目时对齐一次
        self.sync_rows(window, cx);

        let done_map: Vec<(TodoId, bool)> = {
            let app = self.state.read(cx);
            app.store
                .items
                .iter()
                .map(|item| (item.id, item.done))
                .collect()
        };
        let is_empty = self.rows.is_empty();

        let mut form = div().flex().flex_col().gap_2();
        for row in &self.rows {
            let id = row.id;
            let done = done_map
                .iter()
                .find(|(item_id, _)| *item_id == id)
                .map(|(_, done)| *done)
                .unwrap_or(false);
            let due_error = row.due_error.clone();

            let toggler = self.state.clone();

            form = form.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(rgb(palette::SURFACE))
                    .border_1()
                    .border_color(rgba(palette::BORDER))
                    // 第一行：勾选框 + 内容 + 删除
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Checkbox::new(format!("edit-done-{id}"))
                                    .checked(done)
                                    .on_change(move |checked, _window, cx| {
                                        toggler.update(cx, |app, cx| {
                                            let changed = app.set_done(id, *checked);
                                            app.commit_click(changed, cx);
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .when(done, |input| input.opacity(0.6))
                                    .child(Input::new(&row.text).cleanable(true)),
                            )
                            .child(
                                Button::new(format!("edit-del-{id}"))
                                    .label("删除")
                                    .compact()
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.delete_row(id, cx);
                                    })),
                            ),
                    )
                    // 第二行：时间 + 标签 + 解析错误提示
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pl(px(26.0))
                            .child(
                                div()
                                    .w(px(176.0))
                                    .child(Input::new(&row.due).small()),
                            )
                            .child(
                                div()
                                    .w(px(132.0))
                                    .child(Input::new(&row.tag).small()),
                            )
                            .when_some(due_error, |line, message| {
                                line.child(
                                    div()
                                        .flex_1()
                                        .text_size(px(10.0))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            }),
                    ),
            );
        }

        div()
            .size_full()
            .bg(rgb(palette::BG))
            .flex()
            .flex_col()
            .text_size(px(13.0))
            .text_color(rgb(palette::TEXT))
            // 顶部标题
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(10.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_bold()
                            .child("待办事项"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(palette::MUTED))
                            .child("时间可留空 · 2026-11-20 18:00 / 11-20 18:00 / 18:00"),
                    ),
            )
            // 列表
            .child(
                div()
                    .id("editor-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px(px(16.0))
                    .pb(px(12.0))
                    .overflow_y_scroll()
                    .when(is_empty, |list| {
                        list.child(
                            div()
                                .py(px(24.0))
                                .text_color(rgb(palette::MUTED))
                                .child("还没有待办，点下面的「＋ 新增待办」"),
                        )
                    })
                    .child(form),
            )
            // 底部操作条
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(16.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(rgba(palette::BORDER))
                    .child(
                        Button::new("editor-add")
                            .label("＋ 新增待办")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_row(window, cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("editor-close")
                            .label("关闭")
                            .on_click(cx.listener(|_, _, window, _cx| {
                                window.remove_window();
                            })),
                    ),
            )
    }
}

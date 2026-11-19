//! 编辑窗口：结构化行编辑（每行 = 勾选框 + 内容 + 截止时间 + 标签 + 删除），
//! 外加全局快捷键的设置（录制 / 恢复默认）。
//!
//! 这个窗口走 `gpui_kit::open_window`（有 `Root`），所以可以用 gpui-component 的
//! 输入框/按钮；桌面层窗口则不行（见 `ui/mod.rs`）。
//!
//! 输入框是 entity（`InputState`），每一行的三个输入框在行创建时生成一次并常驻，
//! 行只在「新增/删除」时增删——不在 render 里创建 entity。

use chrono::Local;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::timeparse;
use crate::platform::win_thread::{self, PlatformCommand};
use crate::ui::state::AppState;
use crate::ui::theme::editor as palette;

/// 新建待办时填入的占位内容（`Store::add` 不接受空文本）。
const NEW_ITEM_TEXT: &str = "新待办";

pub struct EditorWindow {
    state: Entity<AppState>,
    rows: Vec<RowEditors>,
    /// 是否正在录制快捷键。
    recording: bool,
    recording_focus: FocusHandle,
    /// 快捷键操作的提示（校验失败/注册失败原因）。
    hotkey_note: Option<String>,
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
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            state: state.clone(),
            rows: Vec::new(),
            recording: false,
            recording_focus: cx.focus_handle(),
            hotkey_note: None,
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
                .placeholder("18:00")
                .default_value(due)
        });
        let tag_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("标签")
                .default_value(tag)
        });

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

    // ---- 快捷键设置 ----

    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = true;
        self.hotkey_note = None;
        self.recording_focus.focus(window, cx);
        cx.notify();
    }

    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = false;
        self.hotkey_note = None;
        cx.notify();
    }

    fn on_hotkey_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.recording {
            return;
        }

        let key = event.keystroke.key.to_ascii_lowercase();
        // 只按了修饰键：还没按主键，继续等
        if matches!(
            key.as_str(),
            "control" | "ctrl" | "alt" | "shift" | "meta" | "super" | "win" | "cmd" | "function"
        ) {
            return;
        }
        if key == "escape" {
            self.cancel_recording(cx);
            return;
        }

        let modifiers = event.keystroke.modifiers;
        let mut parts: Vec<&str> = Vec::new();
        if modifiers.control {
            parts.push("Ctrl");
        }
        if modifiers.alt {
            parts.push("Alt");
        }
        if modifiers.shift {
            parts.push("Shift");
        }
        if modifiers.platform {
            parts.push("Win");
        }
        parts.push(&key);

        self.apply_hotkey(&parts.join("+"), cx);
    }

    fn apply_hotkey(&mut self, raw: &str, cx: &mut Context<Self>) {
        let result = self.state.update(cx, |app, cx| {
            let result = app.set_hotkey(raw);
            if result.is_ok() {
                app.commit_click(true, cx);
            }
            result
        });

        match result {
            Ok(spec) => {
                win_thread::send_command(PlatformCommand::SetHotkey(spec));
                self.recording = false;
                self.hotkey_note = None;
            }
            Err(message) => {
                self.hotkey_note = Some(message);
            }
        }
        cx.notify();
    }

    fn reset_hotkey(&mut self, cx: &mut Context<Self>) {
        let spec = self.state.update(cx, |app, cx| {
            let spec = app.reset_hotkey();
            app.commit_click(true, cx);
            spec
        });
        win_thread::send_command(PlatformCommand::SetHotkey(spec));
        self.recording = false;
        self.hotkey_note = None;
        cx.notify();
    }
}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 防御：外部新增/删除了条目时对齐一次
        self.sync_rows(window, cx);

        let now = chrono::Local::now().naive_local();
        let (done_map, hotkey, hotkey_error, pending) = {
            let app = self.state.read(cx);
            // (id, 已完成, 已逾期)
            let done_map: Vec<(TodoId, bool, bool)> = app
                .store
                .items
                .iter()
                .map(|item| (item.id, item.done, item.is_overdue(now)))
                .collect();
            (
                done_map,
                app.hotkey().format(),
                app.hotkey_error.clone(),
                app.store.items.iter().filter(|item| !item.done).count(),
            )
        };
        let is_empty = self.rows.is_empty();
        let recording = self.recording;
        let note = self.hotkey_note.clone().or(hotkey_error);

        let mut list = div().flex().flex_col();
        for (index, row) in self.rows.iter().enumerate() {
            let id = row.id;
            let (done, overdue) = done_map
                .iter()
                .find(|(item_id, _, _)| *item_id == id)
                .map(|(_, done, overdue)| (*done, *overdue))
                .unwrap_or((false, false));
            let due_error = row.due_error.clone();
            let toggler = self.state.clone();
            let has_error = due_error.is_some();

            // 行之间用细线分隔，第一行前面不加
            if index > 0 {
                list = list.child(
                    div()
                        .h(px(1.0))
                        .w_full()
                        .bg(rgba(palette::BORDER)),
                );
            }

            list = list.child(
                div()
                    .id(("editor-row", id as usize))
                    .flex()
                    .items_start()
                    .gap_3()
                    .px(px(8.0))
                    .py(px(9.0))
                    .rounded(px(6.0))
                    .hover(|style| style.bg(rgba(palette::HOVER)))
                    .child(
                        div().pt(px(2.0)).child(
                            Checkbox::new(format!("edit-done-{id}"))
                                .checked(done)
                                .on_change(move |checked, _window, cx| {
                                    toggler.update(cx, |app, cx| {
                                        let changed = app.set_done(id, *checked);
                                        app.commit_click(changed, cx);
                                    });
                                }),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .when(done, |column| column.opacity(0.55))
                            // 内容：无边框输入，平时看起来就是一行文字
                            .child(Input::new(&row.text).appearance(false))
                            // 元信息：时间 + 标签 + 解析错误
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(148.0))
                                            .text_size(px(11.0))
                                            .text_color(if has_error || overdue {
                                                rgb(palette::DANGER)
                                            } else {
                                                rgb(palette::MUTED)
                                            })
                                            .child(Input::new(&row.due).appearance(false).small()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_0p5()
                                            .w(px(120.0))
                                            .text_size(px(11.0))
                                            .text_color(rgb(palette::MUTED))
                                            .child(div().child("#"))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .child(
                                                        Input::new(&row.tag)
                                                            .appearance(false)
                                                            .small(),
                                                    ),
                                            ),
                                    )
                                    .when_some(due_error, |line, message| {
                                        line.child(
                                            div()
                                                .flex_1()
                                                .text_size(px(11.0))
                                                .text_color(rgb(palette::DANGER))
                                                .child(message),
                                        )
                                    }),
                            ),
                    )
                    // 删除：安静的 ×，hover 才变红
                    .child(
                        div()
                            .id(("editor-del", id as usize))
                            .w(px(18.0))
                            .text_size(px(13.0))
                            .text_color(rgb(palette::MUTED))
                            .hover(|style| style.text_color(rgb(palette::DANGER)))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.delete_row(id, cx);
                            })),
                    ),
            );
        }

        let record_button = if recording {
            text_link(
                "hotkey-cancel",
                "取消",
                palette::MUTED,
                cx.listener(|this, _, _window, cx| this.cancel_recording(cx)),
            )
        } else {
            text_link(
                "hotkey-record",
                "修改",
                palette::ACCENT,
                cx.listener(|this, _, window, cx| this.start_recording(window, cx)),
            )
        };

        div()
            .id("editor-root")
            .track_focus(&self.recording_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.on_hotkey_key(event, window, cx);
            }))
            .size_full()
            .bg(rgb(palette::BG))
            .flex()
            .flex_col()
            .text_size(px(13.0))
            .line_height(px(21.0))
            .text_color(rgb(palette::TEXT))
            // 标题
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .px(px(20.0))
                    .pt(px(16.0))
                    .pb(px(12.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_semibold()
                            .child("待办事项"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(palette::MUTED))
                            .child(format!("{pending} 项未完成")),
                    ),
            )
            // 快捷键设置
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(px(20.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_b_1()
                    .border_color(rgba(palette::BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(44.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(palette::MUTED))
                                    .child("快捷键"),
                            )
                            .child(if recording {
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(rgb(palette::ACCENT))
                                    .child("按下新的组合键…（Esc 取消）")
                                    .into_any_element()
                            } else {
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded(px(4.0))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .bg(rgb(palette::SURFACE))
                                            .text_size(px(11.0))
                                            .text_color(rgb(palette::TEXT))
                                            .child(hotkey.clone()),
                                    )
                                    .into_any_element()
                            })
                            .child(record_button)
                            .child(text_link(
                                "hotkey-reset",
                                "恢复默认",
                                palette::MUTED,
                                cx.listener(|this, _, _window, cx| this.reset_hotkey(cx)),
                            )),
                    )
                    .when_some(note, |section, message| {
                        section.child(
                            div()
                                .pl(px(56.0))
                                .text_size(px(11.0))
                                .text_color(rgb(palette::DANGER))
                                .child(message),
                        )
                    }),
            )
            // 列表
            .child(
                div()
                    .id("editor-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px(px(12.0))
                    .py(px(4.0))
                    .overflow_y_scroll()
                    .when(is_empty, |area| {
                        area.child(
                            div()
                                .px(px(8.0))
                                .py(px(28.0))
                                .text_size(px(12.0))
                                .text_color(rgb(palette::MUTED))
                                .child("还没有待办 · 点下面的「＋ 新增」"),
                        )
                    })
                    .child(list),
            )
            // 底部
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px(px(20.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(rgba(palette::BORDER))
                    .child(text_link(
                        "editor-add",
                        "＋ 新增",
                        palette::ACCENT,
                        cx.listener(|this, _, window, cx| this.add_row(window, cx)),
                    ))
                    .child(div().flex_1())
                    .child(text_link(
                        "editor-close",
                        "关闭",
                        palette::MUTED,
                        cx.listener(|_, _, window, _cx| window.remove_window()),
                    )),
            )
    }
}

/// 文字按钮：没有框、没有底色，靠颜色和 hover 表达可点击。
fn text_link(
    id: &'static str,
    label: &'static str,
    color: u32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .text_size(px(12.0))
        .text_color(rgb(color))
        .hover(|style| style.opacity(0.7))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

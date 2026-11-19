//! 编辑窗口：顶部「待办 / 设置」两页。
//!
//! - 待办页：单行表格式清单（勾选框 · 内容 · 截止时间 · 标签 · 删除），列宽固定所以能对齐。
//! - 设置页：全局快捷键（录制 / 恢复默认），以后放主题、开机自启等。
//!
//! 这个窗口走 `gpui_kit::open_window`（有 `Root`），所以可以用 gpui-component 的
//! 输入框/按钮；桌面层窗口则不行（见 `ui/mod.rs`）。
//!
//! 输入框是 entity（`InputState`），每行的三个输入框在行创建时生成一次并常驻，
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

// 列宽（固定，保证每行对齐）
const COL_CHECK: f32 = 20.0;
const COL_DUE: f32 = 168.0;
const COL_TAG: f32 = 120.0;
const COL_DEL: f32 = 18.0;
const GAP: f32 = 12.0;
const PAD_X: f32 = 20.0;

/// 编辑窗口的两页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Todos,
    Settings,
}

pub struct EditorWindow {
    state: Entity<AppState>,
    tab: Tab,
    rows: Vec<RowEditors>,
    /// 是否正在录制快捷键。
    recording: bool,
    recording_focus: FocusHandle,
    /// 快捷键操作的提示（校验失败原因）。
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
    pub fn new(
        state: Entity<AppState>,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            state: state.clone(),
            tab,
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

    /// 切换页签（托盘「设置」入口也会调它）。
    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
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
                .placeholder("—")
                .default_value(due)
        });
        let tag_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("—")
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

    /// 当前第一条时间解析错误（显示在列表下方的状态条里，保持行内对齐）。
    fn due_error_message(&self) -> Option<String> {
        self.rows
            .iter()
            .find_map(|row| row.due_error.clone())
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

    fn on_hotkey_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
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

        let now = Local::now().naive_local();
        let (rows_state, hotkey, hotkey_error, pending) = {
            let app = self.state.read(cx);
            // (id, 已完成, 已逾期)
            let rows_state: Vec<(TodoId, bool, bool)> = app
                .store
                .items
                .iter()
                .map(|item| (item.id, item.done, item.is_overdue(now)))
                .collect();
            (
                rows_state,
                app.hotkey().format(),
                app.hotkey_error.clone(),
                app.store.items.iter().filter(|item| !item.done).count(),
            )
        };

        let tab = self.tab;
        let recording = self.recording;
        let note = self.hotkey_note.clone().or(hotkey_error);

        let content = match tab {
            Tab::Todos => self.render_todos(cx, &rows_state),
            Tab::Settings => self.render_settings(cx, &hotkey, recording, note),
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
            .line_height(px(20.0))
            .text_color(rgb(palette::TEXT))
            // 页签栏
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_5()
                    .px(px(PAD_X))
                    .pt(px(10.0))
                    .border_b_1()
                    .border_color(rgba(palette::BORDER))
                    .child(tab_button("tab-todos", "待办", tab == Tab::Todos, cx))
                    .child(tab_button(
                        "tab-settings",
                        "设置",
                        tab == Tab::Settings,
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(
                        div()
                            .pb(px(10.0))
                            .text_size(px(11.0))
                            .text_color(rgb(palette::MUTED))
                            .child(format!("{pending} 项未完成")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(content),
            )
    }
}

impl EditorWindow {
    fn render_todos(
        &mut self,
        cx: &mut Context<Self>,
        rows_state: &[(TodoId, bool, bool)],
    ) -> AnyElement {
        let is_empty = self.rows.is_empty();
        let due_error = self.due_error_message();

        // 表头：列宽与每行一致，所以能对齐
        let header = div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .px(px(PAD_X))
            .py(px(6.0))
            .border_b_1()
            .border_color(rgba(palette::BORDER))
            .child(div().w(px(COL_CHECK)))
            .child(
                div()
                    .flex_1()
                    .text_size(px(11.0))
                    .text_color(rgb(palette::MUTED))
                    .child("内容"),
            )
            .child(
                div()
                    .w(px(COL_DUE))
                    .text_size(px(11.0))
                    .text_color(rgb(palette::MUTED))
                    .child("截止时间"),
            )
            .child(
                div()
                    .w(px(COL_TAG))
                    .text_size(px(11.0))
                    .text_color(rgb(palette::MUTED))
                    .child("标签"),
            )
            .child(div().w(px(COL_DEL)));

        let mut list = div().flex().flex_col();
        for row in &self.rows {
            let id = row.id;
            let (done, overdue) = rows_state
                .iter()
                .find(|(item_id, _, _)| *item_id == id)
                .map(|(_, done, overdue)| (*done, *overdue))
                .unwrap_or((false, false));
            let row_has_error = row.due_error.is_some();
            let toggler = self.state.clone();

            list = list.child(
                div()
                    .id(("editor-row", id as usize))
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .px(px(PAD_X))
                    .py(px(5.0))
                    .hover(|style| style.bg(rgba(palette::HOVER)))
                    .child(
                        div()
                            .w(px(COL_CHECK))
                            .child(
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
                    // 内容：无边框输入，平时看起来就是一行文字
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(if done {
                                rgb(palette::MUTED)
                            } else {
                                rgb(palette::TEXT)
                            })
                            .child(Input::new(&row.text).appearance(false)),
                    )
                    // 截止时间：逾期或解析失败标红
                    .child(
                        div()
                            .w(px(COL_DUE))
                            .text_size(px(12.0))
                            .text_color(if row_has_error || overdue {
                                rgb(palette::DANGER)
                            } else {
                                rgb(palette::MUTED)
                            })
                            .child(Input::new(&row.due).appearance(false).small()),
                    )
                    // 标签：带 # 前缀
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .w(px(COL_TAG))
                            .text_size(px(12.0))
                            .text_color(rgb(palette::MUTED))
                            .child(div().child("#"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&row.tag).appearance(false).small()),
                            ),
                    )
                    // 删除：安静的 ×，hover 才变红
                    .child(
                        div()
                            .id(("editor-del", id as usize))
                            .w(px(COL_DEL))
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(
                div()
                    .id("editor-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(is_empty, |area| {
                        area.child(
                            div()
                                .px(px(PAD_X))
                                .py(px(28.0))
                                .text_size(px(12.0))
                                .text_color(rgb(palette::MUTED))
                                .child("还没有待办 · 点下面的「＋ 新增」"),
                        )
                    })
                    .child(list),
            )
            // 时间解析失败时在底部给提示，行内保持对齐
            .when_some(due_error, |area, message| {
                area.child(
                    div()
                        .px(px(PAD_X))
                        .py(px(6.0))
                        .border_t_1()
                        .border_color(rgba(palette::BORDER))
                        .text_size(px(11.0))
                        .text_color(rgb(palette::DANGER))
                        .child(message),
                )
            })
            // 底部操作
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px(px(PAD_X))
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
            .into_any_element()
    }

    fn render_settings(
        &mut self,
        cx: &mut Context<Self>,
        hotkey: &str,
        recording: bool,
        note: Option<String>,
    ) -> AnyElement {
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
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            // 设置项分组标题
            .child(
                div()
                    .px(px(PAD_X))
                    .pt(px(18.0))
                    .pb(px(8.0))
                    .text_size(px(12.0))
                    .font_semibold()
                    .child("快捷键"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px(px(PAD_X))
                    .py(px(10.0))
                    .border_t_1()
                    .border_b_1()
                    .border_color(rgba(palette::BORDER))
                    .child(
                        div()
                            .w(px(160.0))
                            .text_size(px(12.0))
                            .text_color(rgb(palette::MUTED))
                            .child("唤起 / 收回待办面板"),
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
                                    .items_center()
                                    .px(px(7.0))
                                    .py(px(3.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(rgba(palette::BORDER))
                                    .bg(rgb(palette::SURFACE))
                                    .text_size(px(12.0))
                                    .text_color(rgb(palette::TEXT))
                                    .child(hotkey.to_string()),
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
                        .px(px(PAD_X))
                        .py(px(6.0))
                        .text_size(px(11.0))
                        .text_color(rgb(palette::DANGER))
                        .child(message),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px(px(PAD_X))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(rgba(palette::BORDER))
                    .child(div().flex_1())
                    .child(text_link(
                        "settings-close",
                        "关闭",
                        palette::MUTED,
                        cx.listener(|_, _, window, _cx| window.remove_window()),
                    )),
            )
            .into_any_element()
    }
}

/// 顶部页签：选中态用正文色 + 底部 2px 强调色下划线。
fn tab_button(
    id: &'static str,
    label: &'static str,
    active: bool,
    cx: &mut Context<EditorWindow>,
) -> AnyElement {
    let tab = if label == "待办" { Tab::Todos } else { Tab::Settings };
    div()
        .id(id)
        .pb(px(8.0))
        .text_size(px(13.0))
        .when(active, |tab| tab.font_semibold())
        .text_color(if active {
            rgb(palette::TEXT)
        } else {
            rgb(palette::MUTED)
        })
        .border_b_2()
        .border_color(if active {
            rgb(palette::ACCENT)
        } else {
            rgba(0x00000000)
        })
        .child(label)
        .on_click(cx.listener(move |this, _, _window, cx| this.set_tab(tab, cx)))
        .into_any_element()
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

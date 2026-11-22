//! 编辑窗口：浅色精致卡片风，顶部「待办 / 设置」分段页签。
//!
//! - 待办页：顶部快速添加卡片（Enter 直接创建）+「进行中」卡片 +「已完成」可折叠卡片。
//! - 设置页：快捷键键帽与录制（录制期自动暂停旧热键）、开机自启开关、数据路径与时间格式。
//!
//! 所有 `InputState` 与 `DatePickerState` 在行创建/窗口初始化时生成一次并常驻，不在 `render` 里创建 entity。

use std::collections::HashMap;

use chrono::{Datelike as _, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use gpui_kit::component::date_picker::{
    DatePicker, DatePickerEvent, DatePickerState, DateRangePreset, DateTime,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::time_field::{HourCycle, TimePrecision};
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::platform::autostart;
use crate::platform::win_thread::{self, PlatformCommand};
use crate::ui::state::AppState;
use crate::ui::theme::editor as palette;

const COL_DUE: f32 = 180.0;
const COL_TAG: f32 = 114.0;
const PAD_X: f32 = 24.0;
const DUE_DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M";

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
    /// 快速添加栏的常驻输入与时间选择状态。
    quick_text: Entity<InputState>,
    quick_due: Entity<DatePickerState>,
    quick_tag: Entity<InputState>,
    quick_error: Option<String>,
    /// 已完成分组是否展开。
    show_completed: bool,
    /// 是否正在录制快捷键。
    recording: bool,
    recording_focus: FocusHandle,
    /// 快捷键操作的提示（校验失败原因）。
    hotkey_note: Option<String>,
    /// 开机自启当前状态与写注册表报错提示。
    autostart_enabled: bool,
    autostart_error: Option<String>,
}

struct RowEditors {
    id: TodoId,
    text: Entity<InputState>,
    due: Entity<DatePickerState>,
    tag: Entity<InputState>,
}

#[derive(Clone, Copy)]
struct RowSnapshot {
    id: TodoId,
    done: bool,
    overdue: bool,
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        if self.recording {
            win_thread::send_command(PlatformCommand::ResumeHotkey);
        }
    }
}

impl EditorWindow {
    pub fn new(
        state: Entity<AppState>,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let quick_text =
            cx.new(|cx| InputState::new(window, cx).placeholder("添加新待办… 按 Enter 创建"));
        let quick_due = build_due_picker(None, window, cx);
        let quick_tag = cx.new(|cx| InputState::new(window, cx).placeholder("标签"));

        for input in [&quick_text, &quick_tag] {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { .. } => this.submit_quick_add(window, cx),
                    InputEvent::Change if this.quick_error.is_some() => {
                        this.quick_error = None;
                        cx.notify();
                    }
                    _ => {}
                }
            })
            .detach();
        }

        let mut this = Self {
            state: state.clone(),
            tab,
            rows: Vec::new(),
            quick_text,
            quick_due,
            quick_tag,
            quick_error: None,
            show_completed: true,
            recording: false,
            recording_focus: cx.focus_handle(),
            hotkey_note: None,
            autostart_enabled: autostart::is_enabled(),
            autostart_error: None,
        };

        // 桌面层窗口勾选/改标签时同步重绘，并在切换至设置页时对齐状态
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        this.sync_rows(window, cx);
        this
    }

    /// 切换页签（托盘「设置」入口也会调它）。
    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            if self.recording {
                self.cancel_recording(cx);
            }
            if tab == Tab::Settings {
                self.autostart_enabled = autostart::is_enabled();
                self.autostart_error = None;
            }
            self.tab = tab;
            cx.notify();
        }
    }

    /// 让行与 store 对齐：新增缺失的行，丢掉已删除的行（按 id 降序，新条排在最上方）。
    fn sync_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<TodoId> = {
            let app = self.state.read(cx);
            app.store.items.iter().map(|item| item.id).collect()
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
        self.rows.sort_by_key(|row| std::cmp::Reverse(row.id));
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
                item.due,
                item.tag.clone().unwrap_or_default(),
            )
        };

        let text_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("待办内容")
                .default_value(text)
        });
        let due_picker = build_due_picker(due, window, cx);
        let tag_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("添加标签")
                .default_value(tag)
        });

        cx.subscribe_in(&text_input, window, move |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::Change => this.on_text_changed(id, cx),
                InputEvent::Blur => this.restore_blank_text(id, window, cx),
                _ => {}
            }
        })
        .detach();

        cx.subscribe(&due_picker, move |this, _, event: &DatePickerEvent, cx| {
            let DatePickerEvent::Change(value) = event;
            this.on_due_picked(id, value.start(), cx);
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
            due: due_picker,
            tag: tag_input,
        })
    }

    fn submit_quick_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.quick_text.read(cx).value().to_string();
        if text.trim().is_empty() {
            self.quick_error = Some("请输入待办内容".into());
            self.quick_text.update(cx, |s, cx| s.focus(window, cx));
            cx.notify();
            return;
        }

        let due = self.quick_due.read(cx).date_time().start();
        let tag_raw = self.quick_tag.read(cx).value().to_string();
        let created = self.state.update(cx, |app, cx| {
            let id = app.add_item_full(&text, due, Some(&tag_raw));
            app.commit_click(id.is_some(), cx);
            id
        });

        if created.is_some() {
            self.quick_text
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.quick_due
                .update(cx, |s, cx| s.set_date_time(DateTime::Single(None), window, cx));
            self.quick_tag
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.quick_error = None;
            self.sync_rows(window, cx);
            self.quick_text.update(cx, |s, cx| s.focus(window, cx));
            cx.notify();
        }
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

    /// `Store::set_text` 会拒绝空白文本；失去焦点时把空输入框恢复成原内容，避免界面与磁盘脱节。
    fn restore_blank_text(&mut self, id: TodoId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.id == id) else {
            return;
        };
        if !row.text.read(cx).value().trim().is_empty() {
            return;
        }
        let stored = {
            let app = self.state.read(cx);
            app.store.item(id).map(|item| item.text.clone())
        };
        if let Some(original) = stored {
            row.text
                .update(cx, |s, cx| s.set_value(original, window, cx));
        }
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
        cx.notify();
    }

    fn on_due_picked(&mut self, id: TodoId, due: Option<NaiveDateTime>, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.set_due(id, due);
            app.commit_click(changed, cx);
        });
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

    fn clear_completed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let removed = self.state.update(cx, |app, cx| {
            let count = app.clear_completed();
            app.commit_click(count > 0, cx);
            count
        });
        if removed > 0 {
            self.sync_rows(window, cx);
            cx.notify();
        }
    }

    fn toggle_completed_group(&mut self, cx: &mut Context<Self>) {
        self.show_completed = !self.show_completed;
        cx.notify();
    }

    // ---- 快捷键与系统集成 ----

    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            win_thread::send_command(PlatformCommand::PauseHotkey);
        }
        self.recording = true;
        self.hotkey_note = None;
        self.recording_focus.focus(window, cx);
        cx.notify();
    }

    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording {
            win_thread::send_command(PlatformCommand::ResumeHotkey);
        }
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

    fn toggle_autostart(&mut self, next: bool, cx: &mut Context<Self>) {
        match autostart::set_enabled(next) {
            Ok(()) => {
                self.autostart_enabled = next;
                self.autostart_error = None;
            }
            Err(message) => {
                self.autostart_enabled = autostart::is_enabled();
                self.autostart_error = Some(message);
            }
        }
        cx.notify();
    }
}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_rows(window, cx);

        let now = Local::now().naive_local();
        let (snapshots, hotkey, hotkey_error, pending, data_path) = {
            let app = self.state.read(cx);
            let mut map = HashMap::with_capacity(app.store.items.len());
            let mut pending = 0usize;
            for item in &app.store.items {
                if !item.done {
                    pending += 1;
                }
                map.insert(
                    item.id,
                    RowSnapshot {
                        id: item.id,
                        done: item.done,
                        overdue: item.is_overdue(now),
                    },
                );
            }
            (
                map,
                app.hotkey().format(),
                app.hotkey_error.clone(),
                pending,
                app.path().display().to_string(),
            )
        };

        let tab = self.tab;
        let recording = self.recording;
        let note = self.hotkey_note.clone().or(hotkey_error);

        let content = match tab {
            Tab::Todos => self.render_todos(cx, &snapshots),
            Tab::Settings => self.render_settings(cx, &hotkey, recording, note, &data_path),
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
            .child(render_header(tab, pending, cx))
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
        snapshots: &HashMap<TodoId, RowSnapshot>,
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
        let presets = due_presets(Local::now().date_naive());

        div()
            .id("todos-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(PAD_X))
            .py(px(18.0))
            .gap(px(18.0))
            .child(self.render_quick_add_card(&presets, cx))
            .when(total_empty, |body| body.child(render_empty_card()))
            .when(!active_ids.is_empty(), |body| {
                body.child(
                    div()
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
                                        .child("点击截止时间可打开日历与快捷预设"),
                                ),
                        )
                        .child(self.render_todo_card(&active_ids, snapshots, &presets, cx)),
                )
            })
            .when(!done_ids.is_empty(), |body| {
                let done_count = done_ids.len();
                body.child(
                    div()
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
                                                .child(if show_completed { "▾" } else { "▸" }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_semibold()
                                                .text_color(rgb(palette::MUTED))
                                                .child("已完成"),
                                        )
                                        .child(count_badge(done_count, false)),
                                )
                                .child(
                                    div()
                                        .id("clear-completed")
                                        .px(px(10.0))
                                        .py(px(3.0))
                                        .rounded(px(6.0))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::MUTED))
                                        .cursor_pointer()
                                        .hover(|s| {
                                            s.bg(rgba(palette::DANGER_SOFT))
                                                .text_color(rgb(palette::DANGER))
                                        })
                                        .child("清空已完成")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.clear_completed(window, cx);
                                        })),
                                ),
                        )
                        .when(show_completed, |group| {
                            group.child(self.render_todo_card(&done_ids, snapshots, &presets, cx))
                        }),
                )
            })
            .into_any_element()
    }

    fn render_quick_add_card(
        &self,
        presets: &[DateRangePreset],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .bg(rgb(palette::SURFACE))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(palette::BORDER))
            .overflow_hidden()
            .flex()
            .flex_col()
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
                            .w(px(108.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(7.0))
                            .bg(rgb(palette::SURFACE_SUBTLE))
                            .border_1()
                            .border_color(rgba(palette::BORDER))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(palette::SUBTLE))
                                    .child("#"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(rgb(palette::TEXT))
                                    .child(
                                        Input::new(&self.quick_tag)
                                            .appearance(false)
                                            .xsmall()
                                            .px(px(2.0))
                                            .h(px(22.0))
                                            .text_size(px(12.0)),
                                    ),
                            ),
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
        presets: &[DateRangePreset],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut card = div()
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
            let snap = snapshots.get(&id).copied().unwrap_or(RowSnapshot {
                id,
                done: false,
                overdue: false,
            });
            card = card.child(self.render_todo_row(row, snap, presets, index > 0, cx));
        }

        card.into_any_element()
    }

    fn render_todo_row(
        &self,
        row: &RowEditors,
        snap: RowSnapshot,
        presets: &[DateRangePreset],
        with_top_divider: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = snap.id;
        let done = snap.done;
        let overdue = snap.overdue;
        let has_due = row.due.read(cx).date_time().start().is_some();
        let has_tag = !row.tag.read(cx).value().trim().is_empty();
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

        let (tag_bg, tag_fg) = if has_tag {
            (rgba(palette::TAG_BG), rgb(palette::TAG_TEXT))
        } else {
            (rgba(0x00000007), rgb(palette::SUBTLE))
        };

        div()
            .id(("editor-row", id as usize))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(8.0))
            .when(with_top_divider, |el| {
                el.border_t_1().border_color(rgba(palette::BORDER))
            })
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
            // 标签胶囊（空时不渲染前缀 #）
            .child(
                div()
                    .w(px(COL_TAG))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(6.0))
                    .bg(tag_bg)
                    .hover(|s| {
                        if has_tag {
                            s.bg(rgba(palette::TAG_BG))
                        } else {
                            s.bg(rgba(palette::DUE_BG))
                        }
                    })
                    .when(has_tag, |pill| {
                        pill.child(
                            div()
                                .text_size(px(11.5))
                                .font_medium()
                                .text_color(tag_fg)
                                .child("#"),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(tag_fg)
                            .child(
                                Input::new(&row.tag)
                                    .appearance(false)
                                    .xsmall()
                                    .px(px(2.0))
                                    .h(px(22.0))
                                    .text_size(px(12.0)),
                            ),
                    ),
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
            )
            .into_any_element()
    }

    fn render_settings(
        &mut self,
        cx: &mut Context<Self>,
        hotkey: &str,
        recording: bool,
        note: Option<String>,
        data_path: &str,
    ) -> AnyElement {
        let hotkey_controls = if recording {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .bg(rgba(palette::ACCENT_SOFT))
                        .border_1()
                        .border_color(rgb(palette::ACCENT))
                        .text_size(px(12.0))
                        .text_color(rgb(palette::ACCENT_TEXT))
                        .child("请按下新组合键… (Esc 取消)"),
                )
                .child(secondary_button(
                    "hotkey-cancel",
                    "取消",
                    cx.listener(|this, _, _window, cx| this.cancel_recording(cx)),
                ))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(render_keycaps(hotkey))
                .child(secondary_button(
                    "hotkey-record",
                    "修改",
                    cx.listener(|this, _, window, cx| this.start_recording(window, cx)),
                ))
                .child(
                    div()
                        .id("hotkey-reset")
                        .px(px(8.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .text_size(px(12.0))
                        .text_color(rgb(palette::MUTED))
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(rgba(palette::HOVER))
                                .text_color(rgb(palette::TEXT))
                        })
                        .child("恢复默认")
                        .on_click(cx.listener(|this, _, _window, cx| this.reset_hotkey(cx))),
                )
                .into_any_element()
        };

        let autostart_enabled = self.autostart_enabled;
        let autostart_error = self.autostart_error.clone();

        div()
            .id("settings-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(PAD_X))
            .py(px(22.0))
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(680.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    // 卡片 1：快捷键与桌面面板
                    .child(settings_section(
                        "快捷键与桌面面板",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "唤起 / 收回桌面面板",
                                        "全局快捷键可随时将桌面待办置顶显示或落回底层",
                                    ))
                                    .child(hotkey_controls),
                            )
                            .when_some(note, |card, message| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(rgba(palette::DANGER_SOFT))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            })
                            .into_any_element(),
                    ))
                    // 卡片 2：系统集成
                    .child(settings_section(
                        "系统集成",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "开机自动启动",
                                        "登录 Windows 时自动在后台运行并展开桌面面板",
                                    ))
                                    .child(
                                        Switch::new("autostart-switch")
                                            .checked(autostart_enabled)
                                            .on_change(cx.listener(
                                                |this, next: &bool, _window, cx| {
                                                    this.toggle_autostart(*next, cx);
                                                },
                                            )),
                                    ),
                            )
                            .when_some(autostart_error, |card, message| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(rgba(palette::DANGER_SOFT))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            })
                            .into_any_element(),
                    ))
                    // 卡片 3：数据与时间选择说明
                    .child(settings_section(
                        "数据与时间选择",
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .pb(px(12.0))
                                    .border_b_1()
                                    .border_color(rgba(palette::BORDER))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "本地存储路径",
                                        "所有待办与配置均以原子写入保存在本地 JSON 中",
                                    ))
                                    .child(
                                        div()
                                            .max_w(px(320.0))
                                            .px(px(10.0))
                                            .py(px(4.0))
                                            .rounded(px(6.0))
                                            .bg(rgb(palette::SURFACE_SUBTLE))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .text_size(px(11.5))
                                            .text_color(rgb(palette::MUTED))
                                            .child(data_path.to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .pt(px(12.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "截止时间选择",
                                        "点击日历并搭配快捷预设或下方时间段微调，点击 × 一键清除",
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(format_chip("日历选日"))
                                            .child(format_chip("时分微调"))
                                            .child(format_chip("快捷预设")),
                                    ),
                            )
                            .into_any_element(),
                    )),
            )
            .into_any_element()
    }
}

fn render_header(tab: Tab, pending: usize, cx: &mut Context<EditorWindow>) -> AnyElement {
    div()
        .bg(rgb(palette::SURFACE))
        .border_b_1()
        .border_color(rgba(palette::BORDER))
        .px(px(PAD_X))
        .py(px(12.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .size(px(24.0))
                        .rounded(px(6.0))
                        .bg(rgb(palette::ACCENT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.0))
                        .font_bold()
                        .text_color(rgb(palette::SURFACE))
                        .child("✓"),
                )
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_semibold()
                        .text_color(rgb(palette::TEXT))
                        .child("Wasurenai"),
                ),
        )
        .child(
            div()
                .p(px(3.0))
                .rounded(px(8.0))
                .bg(rgb(palette::BG))
                .border_1()
                .border_color(rgba(palette::BORDER))
                .flex()
                .items_center()
                .gap(px(3.0))
                .child(segment_tab(
                    "tab-todos",
                    "待办",
                    Tab::Todos,
                    tab == Tab::Todos,
                    (pending > 0).then_some(pending),
                    cx,
                ))
                .child(segment_tab(
                    "tab-settings",
                    "设置",
                    Tab::Settings,
                    tab == Tab::Settings,
                    None,
                    cx,
                )),
        )
        .into_any_element()
}

fn segment_tab(
    id: &'static str,
    label: &'static str,
    target: Tab,
    active: bool,
    badge: Option<usize>,
    cx: &mut Context<EditorWindow>,
) -> AnyElement {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(px(12.5))
        .cursor_pointer()
        .when(active, |el| {
            el.bg(rgb(palette::SURFACE))
                .border_1()
                .border_color(rgba(palette::BORDER))
                .font_semibold()
                .text_color(rgb(palette::TEXT))
        })
        .when(!active, |el| {
            el.border_1()
                .border_color(rgba(0x00000000))
                .text_color(rgb(palette::MUTED))
                .hover(|s| s.text_color(rgb(palette::TEXT)))
        })
        .child(label)
        .when_some(badge, |el, count| {
            el.child(
                div()
                    .px(px(6.0))
                    .py(px(0.5))
                    .rounded(px(10.0))
                    .bg(if active {
                        rgba(palette::ACCENT_SOFT)
                    } else {
                        rgba(palette::DUE_BG)
                    })
                    .text_size(px(11.0))
                    .font_medium()
                    .text_color(if active {
                        rgb(palette::ACCENT_TEXT)
                    } else {
                        rgb(palette::MUTED)
                    })
                    .child(count.to_string()),
            )
        })
        .on_click(cx.listener(move |this, _, _window, cx| this.set_tab(target, cx)))
        .into_any_element()
}

fn render_empty_card() -> AnyElement {
    div()
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

fn count_badge(count: usize, accent: bool) -> AnyElement {
    div()
        .px(px(7.0))
        .py(px(1.0))
        .rounded(px(10.0))
        .bg(if accent {
            rgba(palette::ACCENT_SOFT)
        } else {
            rgba(palette::DUE_BG)
        })
        .text_size(px(11.0))
        .font_medium()
        .text_color(if accent {
            rgb(palette::ACCENT_TEXT)
        } else {
            rgb(palette::MUTED)
        })
        .child(count.to_string())
        .into_any_element()
}

fn render_keycaps(hotkey: &str) -> AnyElement {
    let mut row = div().flex().items_center().gap(px(4.0));
    for (idx, part) in hotkey.split('+').enumerate() {
        if idx > 0 {
            row = row.child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(palette::SUBTLE))
                    .child("+"),
            );
        }
        row = row.child(
            div()
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(6.0))
                .bg(rgb(palette::SURFACE_SUBTLE))
                .border_1()
                .border_b_2()
                .border_color(rgba(palette::BORDER_STRONG))
                .text_size(px(11.5))
                .font_medium()
                .text_color(rgb(palette::TEXT))
                .child(part.trim().to_string()),
        );
    }
    row.into_any_element()
}

fn secondary_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(5.0))
        .rounded(px(7.0))
        .bg(rgb(palette::SURFACE))
        .border_1()
        .border_color(rgba(palette::BORDER_STRONG))
        .text_size(px(12.0))
        .font_medium()
        .text_color(rgb(palette::TEXT))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(palette::SURFACE_SUBTLE)))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

fn settings_section(title: &'static str, body: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .px(px(4.0))
                .text_size(px(12.0))
                .font_semibold()
                .text_color(rgb(palette::MUTED))
                .child(title),
        )
        .child(
            div()
                .bg(rgb(palette::SURFACE))
                .rounded(px(12.0))
                .border_1()
                .border_color(rgba(palette::BORDER))
                .p(px(16.0))
                .child(body),
        )
        .into_any_element()
}

fn setting_label(title: &'static str, subtitle: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_medium()
                .text_color(rgb(palette::TEXT))
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(palette::SUBTLE))
                .child(subtitle),
        )
        .into_any_element()
}

fn format_chip(label: &'static str) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(rgb(palette::SURFACE_SUBTLE))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .text_size(px(11.5))
        .text_color(rgb(palette::MUTED))
        .child(label)
        .into_any_element()
}

fn build_due_picker(
    initial: Option<NaiveDateTime>,
    window: &mut Window,
    cx: &mut Context<EditorWindow>,
) -> Entity<DatePickerState> {
    cx.new(|cx| {
        let mut picker = DatePickerState::new(window, cx)
            .date_format(DUE_DISPLAY_FORMAT)
            .time_precision(TimePrecision::Minute)
            .hour_cycle(HourCycle::H23)
            .default_time(NaiveTime::from_hms_opt(18, 0, 0).expect("valid default time"))
            .first_day_of_week(Weekday::Mon);
        if let Some(dt) = initial {
            picker.set_date_time(DateTime::Single(Some(dt)), window, cx);
        }
        picker
    })
}

fn render_due_picker(
    state: &Entity<DatePickerState>,
    presets: &[DateRangePreset],
) -> DatePicker {
    DatePicker::new(state)
        .appearance(false)
        .small()
        .cleanable(true)
        .placeholder("截止时间")
        .presets(presets.to_vec())
}

fn due_presets(today: NaiveDate) -> Vec<DateRangePreset> {
    due_preset_specs(today)
        .into_iter()
        .map(|(label, dt)| DateRangePreset::date_time(label, dt))
        .collect()
}

fn due_preset_specs(today: NaiveDate) -> Vec<(&'static str, NaiveDateTime)> {
    let at = |date: NaiveDate, h: u32, m: u32| {
        date.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid preset time"))
    };
    let tomorrow = today + Duration::days(1);
    let days_until_next_monday = 7 - i64::from(today.weekday().num_days_from_monday());
    let next_monday = today + Duration::days(days_until_next_monday);

    vec![
        ("今天 18:00", at(today, 18, 0)),
        ("今天 21:00", at(today, 21, 0)),
        ("明天 09:00", at(tomorrow, 9, 0)),
        ("明天 18:00", at(tomorrow, 18, 0)),
        ("下周一 09:00", at(next_monday, 9, 0)),
    ]
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::due_preset_specs;

    #[test]
    fn due_preset_specs_compute_expected_dates_across_weekdays() {
        // 2026-11-22 是周日 -> 下周一就是 2026-11-23
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let specs = due_preset_specs(sunday);
        assert_eq!(specs.len(), 5);
        assert_eq!(
            specs[0],
            ("今天 18:00", sunday.and_hms_opt(18, 0, 0).unwrap())
        );
        assert_eq!(
            specs[4],
            (
                "下周一 09:00",
                NaiveDate::from_ymd_opt(2026, 11, 23)
                    .unwrap()
                    .and_hms_opt(9, 0, 0)
                    .unwrap()
            )
        );

        // 2026-11-23 是周一 -> 下周一是 7 天后的 2026-11-30
        let monday = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let monday_specs = due_preset_specs(monday);
        assert_eq!(
            monday_specs[4],
            (
                "下周一 09:00",
                NaiveDate::from_ymd_opt(2026, 11, 30)
                    .unwrap()
                    .and_hms_opt(9, 0, 0)
                    .unwrap()
            )
        );
    }
}

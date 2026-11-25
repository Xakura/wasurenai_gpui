//! 编辑窗口：浅色精致卡片风，顶部「待办 / 日志 / 设置」分段页签。
//!
//! - 待办页：顶部固定快速添加卡片（支持多选预定义标签，不随列表滚动被挤占）+「进行中」卡片 +「今日已完成」可折叠卡片。
//! - 日志页：按日期分组预览全部已完成工作，支持打开自动同步的 Markdown 日志、打开目录、导出副本。
//! - 设置页：工作标签管理、快捷键键帽与录制（录制期自动暂停旧热键）、开机自启开关、本地数据与日志路径。
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

use crate::core::model::{TodoId, normalize_tag};
use crate::core::{ordering, worklog};
use crate::platform::{autostart, shell};
use crate::platform::win_thread::{self, PlatformCommand};
use crate::ui::state::AppState;
use crate::ui::theme::editor as palette;

const COL_DUE: f32 = 176.0;
const COL_TAGS: f32 = 168.0;
const PAD_X: f32 = 24.0;
const DUE_DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M";

/// 编辑窗口的三页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Todos,
    Log,
    Settings,
}

pub struct EditorWindow {
    state: Entity<AppState>,
    tab: Tab,
    rows: Vec<RowEditors>,
    /// 快速添加栏的常驻输入、时间选择与已勾选标签。
    quick_text: Entity<InputState>,
    quick_due: Entity<DatePickerState>,
    quick_tags: Vec<String>,
    quick_error: Option<String>,
    /// 当前正在展开多选标签面板的行 id。
    editing_tags_for: Option<TodoId>,
    /// 今日已完成分组是否展开。
    show_completed: bool,
    /// 设置页新增预定义标签的输入框与提示。
    new_tag_input: Entity<InputState>,
    tag_note: Option<String>,
    /// 日志页操作反馈：`(is_error, message)`。
    log_notice: Option<(bool, String)>,
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
}

#[derive(Clone)]
struct RowSnapshot {
    id: TodoId,
    done: bool,
    overdue: bool,
    tags: Vec<String>,
}

struct LogItemSnapshot {
    text: String,
    time: Option<String>,
    tags: Vec<String>,
}

struct LogDaySnapshot {
    title: String,
    badge: Option<&'static str>,
    items: Vec<LogItemSnapshot>,
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
        let new_tag_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入标签名，按 Enter 添加…"));

        cx.subscribe_in(&quick_text, window, |this, _, event: &InputEvent, window, cx| {
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

        cx.subscribe_in(&new_tag_input, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.submit_new_predefined_tag(window, cx),
                InputEvent::Change if this.tag_note.is_some() => {
                    this.tag_note = None;
                    cx.notify();
                }
                _ => {}
            }
        })
        .detach();

        let mut this = Self {
            state: state.clone(),
            tab,
            rows: Vec::new(),
            quick_text,
            quick_due,
            quick_tags: Vec::new(),
            quick_error: None,
            editing_tags_for: None,
            show_completed: true,
            new_tag_input,
            tag_note: None,
            log_notice: None,
            recording: false,
            recording_focus: cx.focus_handle(),
            hotkey_note: None,
            autostart_enabled: autostart::is_enabled(),
            autostart_error: None,
        };

        // 桌面层窗口勾选/跨天刷新时同步重绘
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        this.sync_rows(Local::now().date_naive(), window, cx);
        this
    }

    /// 切换页签（托盘「设置」入口也会调它）。
    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            if self.recording {
                self.cancel_recording(cx);
            }
            self.editing_tags_for = None;
            if tab == Tab::Settings {
                self.autostart_enabled = autostart::is_enabled();
                self.autostart_error = None;
                self.tag_note = None;
            }
            if tab == Tab::Log {
                self.log_notice = None;
            }
            self.tab = tab;
            cx.notify();
        }
    }

    /// 让行与当日可见待办对齐：新增缺失的行，丢掉已删除或跨天隐藏的行（按 id 降序，新条排在最上方）。
    fn sync_rows(&mut self, today: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<TodoId> = {
            let app = self.state.read(cx);
            app.store
                .items
                .iter()
                .filter(|item| ordering::is_visible_on(item, today))
                .map(|item| item.id)
                .collect()
        };

        self.rows.retain(|row| ids.contains(&row.id));
        if let Some(editing) = self.editing_tags_for
            && !ids.contains(&editing)
        {
            self.editing_tags_for = None;
        }

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
        let (text, due) = {
            let app = self.state.read(cx);
            let item = app.store.item(id)?;
            (item.text.clone(), item.due)
        };

        let text_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("待办内容")
                .default_value(text)
        });
        let due_picker = build_due_picker(due, window, cx);

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

        Some(RowEditors {
            id,
            text: text_input,
            due: due_picker,
        })
    }

    fn toggle_quick_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.quick_tags.iter().position(|t| t == tag) {
            self.quick_tags.remove(pos);
        } else {
            self.quick_tags.push(tag.to_string());
        }
        cx.notify();
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
        let tags = self.quick_tags.clone();
        let created = self.state.update(cx, |app, cx| {
            let id = app.add_item_full(&text, due, &tags);
            app.commit_click(id.is_some(), cx);
            id
        });

        if created.is_some() {
            self.quick_text
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.quick_due
                .update(cx, |s, cx| s.set_date_time(DateTime::Single(None), window, cx));
            self.quick_tags.clear();
            self.quick_error = None;
            self.sync_rows(Local::now().date_naive(), window, cx);
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

    fn toggle_row_tag(&mut self, id: TodoId, tag: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.toggle_item_tag(id, tag);
            app.commit_click(changed, cx);
        });
        cx.notify();
    }

    fn toggle_row_tag_picker(&mut self, id: TodoId, cx: &mut Context<Self>) {
        self.editing_tags_for = if self.editing_tags_for == Some(id) {
            None
        } else {
            Some(id)
        };
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
        if self.editing_tags_for == Some(id) {
            self.editing_tags_for = None;
        }
        cx.notify();
    }

    fn toggle_completed_group(&mut self, cx: &mut Context<Self>) {
        self.show_completed = !self.show_completed;
        cx.notify();
    }

    // ---- 预定义标签管理 ----

    fn submit_new_predefined_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.new_tag_input.read(cx).value().to_string();
        let Some(tag) = normalize_tag(&raw) else {
            self.tag_note = Some("请输入有效的标签名称".into());
            self.new_tag_input.update(cx, |s, cx| s.focus(window, cx));
            cx.notify();
            return;
        };

        let added = self.state.update(cx, |app, cx| {
            let added = app.add_predefined_tag(&tag);
            app.commit_click(added, cx);
            added
        });

        if added {
            self.new_tag_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.tag_note = None;
            self.new_tag_input.update(cx, |s, cx| s.focus(window, cx));
        } else {
            self.tag_note = Some(format!("标签「{tag}」已存在"));
        }
        cx.notify();
    }

    fn remove_predefined_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.remove_predefined_tag(tag);
            app.commit_click(changed, cx);
        });
        self.quick_tags.retain(|t| t != tag);
        self.tag_note = None;
        cx.notify();
    }

    // ---- 日志打开与导出 ----

    fn open_log_file(&mut self, cx: &mut Context<Self>) {
        let log_path = {
            let app = self.state.read(cx);
            if let Err(err) = app.sync_worklog_now() {
                self.log_notice = Some((true, format!("同步日志文件失败：{err}")));
                cx.notify();
                return;
            }
            app.log_path().to_path_buf()
        };

        match shell::open_file(&log_path) {
            Ok(()) => self.log_notice = None,
            Err(err) => self.log_notice = Some((true, err)),
        }
        cx.notify();
    }

    fn reveal_log_dir(&mut self, cx: &mut Context<Self>) {
        let log_path = {
            let app = self.state.read(cx);
            let _ = app.sync_worklog_now();
            app.log_path().to_path_buf()
        };

        match shell::reveal_in_explorer(&log_path) {
            Ok(()) => self.log_notice = None,
            Err(err) => self.log_notice = Some((true, err)),
        }
        cx.notify();
    }

    fn export_log_as(&mut self, cx: &mut Context<Self>) {
        let default_dir = dirs::document_dir()
            .or_else(dirs::desktop_dir)
            .unwrap_or_else(|| {
                self.state
                    .read(cx)
                    .log_path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
            });
        let suggested = format!(
            "wasurenai_worklog_{}.md",
            Local::now().format("%Y%m%d")
        );
        let receiver = cx.prompt_for_new_path(&default_dir, Some(&suggested));
        let state = self.state.clone();

        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(dest))) = receiver.await else {
                return;
            };
            let result = state.read_with(cx, |app, _| app.export_worklog_to(&dest));
            this.update(cx, |view, cx| {
                view.log_notice = match result {
                    Ok(()) => Some((false, format!("已导出至 {}", dest.display()))),
                    Err(err) => Some((true, format!("导出失败：{err}"))),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        let now = Local::now().naive_local();
        let today = now.date();
        self.sync_rows(today, window, cx);

        let (
            snapshots,
            predefined_tags,
            hotkey,
            hotkey_error,
            pending,
            total_completed,
            log_days,
            data_path,
            log_path,
        ) = {
            let app = self.state.read(cx);
            let mut map = HashMap::with_capacity(app.store.items.len());
            let mut pending = 0usize;
            let mut total_completed = 0usize;
            for item in &app.store.items {
                if item.done {
                    total_completed += 1;
                } else {
                    pending += 1;
                }
                if ordering::is_visible_on(item, today) {
                    map.insert(
                        item.id,
                        RowSnapshot {
                            id: item.id,
                            done: item.done,
                            overdue: item.is_overdue(now),
                            tags: item.tags.clone(),
                        },
                    );
                }
            }

            let yesterday = today - Duration::days(1);
            let log_days: Vec<LogDaySnapshot> = if self.tab == Tab::Log {
                worklog::completed_groups(&app.store.items, true)
                    .into_iter()
                    .map(|group| {
                        let (title, badge) = match group.date {
                            Some(date) if date == today => {
                                (date.format("%Y-%m-%d").to_string(), Some("今天"))
                            }
                            Some(date) if date == yesterday => {
                                (date.format("%Y-%m-%d").to_string(), Some("昨天"))
                            }
                            Some(date) => (date.format("%Y-%m-%d").to_string(), None),
                            None => ("更早完成".to_string(), None),
                        };
                        let items = group
                            .items
                            .into_iter()
                            .map(|item| LogItemSnapshot {
                                text: item.text.clone(),
                                time: item
                                    .completed_at
                                    .map(|dt| dt.format("%H:%M").to_string()),
                                tags: item.tags.clone(),
                            })
                            .collect();
                        LogDaySnapshot {
                            title,
                            badge,
                            items,
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };

            (
                map,
                app.store.predefined_tags.clone(),
                app.hotkey().format(),
                app.hotkey_error.clone(),
                pending,
                total_completed,
                log_days,
                app.path().display().to_string(),
                app.log_path().display().to_string(),
            )
        };

        let tab = self.tab;
        let recording = self.recording;
        let note = self.hotkey_note.clone().or(hotkey_error);

        let content = match tab {
            Tab::Todos => self.render_todos(cx, &snapshots, &predefined_tags, today),
            Tab::Log => self.render_log(cx, total_completed, log_days, &log_path),
            Tab::Settings => self.render_settings(
                cx,
                &predefined_tags,
                &hotkey,
                recording,
                note,
                &data_path,
                &log_path,
            ),
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

    fn render_log(
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

    #[allow(clippy::too_many_arguments)]
    fn render_settings(
        &mut self,
        cx: &mut Context<Self>,
        predefined_tags: &[String],
        hotkey: &str,
        recording: bool,
        note: Option<String>,
        data_path: &str,
        log_path: &str,
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
        let tag_note = self.tag_note.clone();

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
                    .flex_shrink_0()
                    .w_full()
                    .max_w(px(680.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    // 卡片 1：工作标签管理
                    .child(settings_section(
                        "工作标签管理",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(setting_label(
                                "预定义工作标签",
                                "在此预设常用标签；新增或编辑待办时可直接多选勾选（删除预设不影响历史日志中的标签）",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px(px(10.0))
                                            .py(px(3.0))
                                            .rounded(px(7.0))
                                            .bg(rgb(palette::SURFACE_SUBTLE))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .text_color(rgb(palette::TEXT))
                                            .child(
                                                Input::new(&self.new_tag_input)
                                                    .appearance(false)
                                                    .px(px(2.0))
                                                    .py(px(0.0))
                                                    .h(px(26.0))
                                                    .text_size(px(12.5)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("add-predefined-tag-btn")
                                            .flex_none()
                                            .px(px(12.0))
                                            .h(px(32.0))
                                            .rounded(px(7.0))
                                            .bg(rgb(palette::ACCENT))
                                            .hover(|s| s.bg(rgb(palette::ACCENT_HOVER)))
                                            .text_size(px(12.0))
                                            .font_medium()
                                            .text_color(rgb(palette::SURFACE))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child("添加标签")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.submit_new_predefined_tag(window, cx);
                                            })),
                                    ),
                            )
                            .when_some(tag_note, |card, message| {
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
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap(px(8.0))
                                    .when(predefined_tags.is_empty(), |box_| {
                                        box_.child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(rgb(palette::SUBTLE))
                                                .child("暂无预定义标签，在上方输入名称后按 Enter 添加"),
                                        )
                                    })
                                    .children(predefined_tags.iter().enumerate().map(|(idx, tag)| {
                                        let tag_owned = tag.clone();
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.0))
                                            .pl(px(9.0))
                                            .pr(px(5.0))
                                            .py(px(3.0))
                                            .rounded(px(6.0))
                                            .bg(rgba(palette::TAG_BG))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_medium()
                                                    .text_color(rgb(palette::TAG_TEXT))
                                                    .child(format!("#{tag}")),
                                            )
                                            .child(
                                                div()
                                                    .id(("del-predefined-tag", idx))
                                                    .size(px(18.0))
                                                    .rounded(px(4.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(12.0))
                                                    .text_color(rgb(palette::MUTED))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(rgba(palette::DANGER_SOFT))
                                                            .text_color(rgb(palette::DANGER))
                                                    })
                                                    .child("×")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.remove_predefined_tag(
                                                                &tag_owned, cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                    })),
                            )
                            .into_any_element(),
                    ))
                    // 卡片 2：快捷键与桌面面板
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
                    // 卡片 3：系统集成
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
                    // 卡片 4：数据与日志路径
                    .child(settings_section(
                        "数据与日志",
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
                                    .child(path_badge(data_path)),
                            )
                            .child(
                                div()
                                    .py(px(12.0))
                                    .border_b_1()
                                    .border_color(rgba(palette::BORDER))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "自动日志路径",
                                        "已完成工作自动按时间顺序同步至 Markdown 文件",
                                    ))
                                    .child(path_badge(log_path)),
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

fn checkable_tag_pill(
    id: (&'static str, usize),
    tag: &str,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .border_1()
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(px(11.5))
        .cursor_pointer()
        .when(checked, |el| {
            el.bg(rgba(palette::ACCENT_SOFT))
                .border_color(rgb(palette::ACCENT))
                .font_medium()
                .text_color(rgb(palette::ACCENT_TEXT))
                .child("✓")
        })
        .when(!checked, |el| {
            el.bg(rgb(palette::SURFACE))
                .border_color(rgba(palette::BORDER_STRONG))
                .text_color(rgb(palette::MUTED))
                .hover(|s| {
                    s.border_color(rgb(palette::ACCENT))
                        .text_color(rgb(palette::TEXT))
                })
                .child("#")
        })
        .child(tag.to_string())
        .on_click(on_click)
        .into_any_element()
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

fn render_header(tab: Tab, pending: usize, cx: &mut Context<EditorWindow>) -> AnyElement {
    div()
        .flex_shrink_0()
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
                    "tab-log",
                    "日志",
                    Tab::Log,
                    tab == Tab::Log,
                    None,
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

fn path_badge(path: &str) -> AnyElement {
    div()
        .max_w(px(340.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgb(palette::SURFACE_SUBTLE))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .text_size(px(11.5))
        .text_color(rgb(palette::MUTED))
        .child(path.to_string())
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
    use chrono::{NaiveDate, NaiveTime};

    use super::due_preset_specs;

    #[test]
    fn due_presets_cover_today_tomorrow_and_next_monday() {
        // 2026-11-22 是周日，下周一应为 2026-11-23
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let specs = due_preset_specs(sunday);
        assert_eq!(specs.len(), 5);
        assert_eq!(
            specs[0],
            (
                "今天 18:00",
                sunday.and_time(NaiveTime::from_hms_opt(18, 0, 0).unwrap())
            )
        );
        assert_eq!(
            specs[4],
            (
                "下周一 09:00",
                NaiveDate::from_ymd_opt(2026, 11, 23)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
            )
        );

        // 2026-11-23 是周一，下周一应为 2026-11-30（+7 天）
        let monday = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let specs = due_preset_specs(monday);
        assert_eq!(
            specs[4].1.date(),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()
        );
    }
}

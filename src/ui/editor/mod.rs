//! 编辑窗口：浅色精致卡片风，顶部「待办 / 日志 / 设置」分段页签。
//!
//! - [`todos_tab`]：待办页（顶部固定快速添加卡片 +「进行中」+「今日已完成」可折叠卡片）。
//! - [`log_tab`]：日志页（按日期分组预览全部已完成工作、打开自动日志、导出副本）。
//! - [`settings_tab`]：设置页（工作标签管理、快捷键键帽与录制、开机自启、本地路径说明）。
//! - [`components`]：编辑器跨页签复用的小组件与 `DatePicker` 预设。
//!
//! 所有 `InputState` 与 `DatePickerState` 在行创建/窗口初始化时生成一次并常驻，不在 `render` 里创建 entity。

mod components;
mod log_tab;
mod settings_tab;
mod todos_tab;

use std::collections::HashMap;

use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use gpui_kit::component::date_picker::{DatePickerEvent, DatePickerState, DateTime};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::{TodoId, normalize_tag};
use crate::core::{ordering, worklog};
use crate::platform::{PlatformCommand, autostart, service, shell};
use crate::ui::state::AppState;
use crate::ui::theme::editor as palette;

use self::components::build_due_picker;
use self::log_tab::{LogDaySnapshot, LogItemSnapshot};

pub(super) const COL_DUE: f32 = 176.0;
pub(super) const COL_TAGS: f32 = 168.0;
pub(super) const PAD_X: f32 = 24.0;

/// 编辑窗口的三页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Todos,
    Log,
    Settings,
}

pub struct EditorWindow {
    pub(super) state: Entity<AppState>,
    tab: Tab,
    pub(super) rows: Vec<RowEditors>,
    /// 快速添加栏的常驻输入、时间选择与已勾选标签。
    pub(super) quick_text: Entity<InputState>,
    pub(super) quick_due: Entity<DatePickerState>,
    pub(super) quick_tags: Vec<String>,
    pub(super) quick_error: Option<String>,
    /// 当前正在展开多选标签面板的行 id。
    pub(super) editing_tags_for: Option<TodoId>,
    /// 今日已完成分组是否展开。
    pub(super) show_completed: bool,
    /// 设置页新增预定义标签的输入框与提示。
    pub(super) new_tag_input: Entity<InputState>,
    pub(super) tag_note: Option<String>,
    /// 日志页操作反馈：`(is_error, message)`。
    pub(super) log_notice: Option<(bool, String)>,
    /// 是否正在录制快捷键。
    recording: bool,
    recording_focus: FocusHandle,
    /// 快捷键操作的提示（校验失败原因）。
    hotkey_note: Option<String>,
    /// 开机自启当前状态与写配置报错提示。
    pub(super) autostart_enabled: bool,
    pub(super) autostart_error: Option<String>,
}

pub(super) struct RowEditors {
    pub(super) id: TodoId,
    pub(super) text: Entity<InputState>,
    pub(super) due: Entity<DatePickerState>,
}

#[derive(Clone)]
pub(super) struct RowSnapshot {
    pub(super) id: TodoId,
    pub(super) done: bool,
    pub(super) overdue: bool,
    pub(super) tags: Vec<String>,
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        if self.recording {
            service::send_command(PlatformCommand::ResumeHotkey);
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

    pub(super) fn toggle_quick_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.quick_tags.iter().position(|t| t == tag) {
            self.quick_tags.remove(pos);
        } else {
            self.quick_tags.push(tag.to_string());
        }
        cx.notify();
    }

    pub(super) fn submit_quick_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn toggle_row_tag(&mut self, id: TodoId, tag: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.toggle_item_tag(id, tag);
            app.commit_click(changed, cx);
        });
        cx.notify();
    }

    pub(super) fn toggle_row_tag_picker(&mut self, id: TodoId, cx: &mut Context<Self>) {
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

    pub(super) fn delete_row(&mut self, id: TodoId, cx: &mut Context<Self>) {
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

    pub(super) fn toggle_completed_group(&mut self, cx: &mut Context<Self>) {
        self.show_completed = !self.show_completed;
        cx.notify();
    }

    // ---- 预定义标签管理 ----

    pub(super) fn submit_new_predefined_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn remove_predefined_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |app, cx| {
            let changed = app.remove_predefined_tag(tag);
            app.commit_click(changed, cx);
        });
        self.quick_tags.retain(|t| t != tag);
        self.tag_note = None;
        cx.notify();
    }

    // ---- 日志打开与导出 ----

    pub(super) fn open_log_file(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn reveal_log_dir(&mut self, cx: &mut Context<Self>) {
        let log_path = {
            let app = self.state.read(cx);
            let _ = app.sync_worklog_now();
            app.log_path().to_path_buf()
        };

        match shell::reveal_in_file_manager(&log_path) {
            Ok(()) => self.log_notice = None,
            Err(err) => self.log_notice = Some((true, err)),
        }
        cx.notify();
    }

    pub(super) fn export_log_as(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            service::send_command(PlatformCommand::PauseHotkey);
        }
        self.recording = true;
        self.hotkey_note = None;
        self.recording_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording {
            service::send_command(PlatformCommand::ResumeHotkey);
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
                service::send_command(PlatformCommand::SetHotkey(spec));
                self.recording = false;
                self.hotkey_note = None;
            }
            Err(message) => {
                self.hotkey_note = Some(message);
            }
        }
        cx.notify();
    }

    pub(super) fn reset_hotkey(&mut self, cx: &mut Context<Self>) {
        let spec = self.state.update(cx, |app, cx| {
            let spec = app.reset_hotkey();
            app.commit_click(true, cx);
            spec
        });
        service::send_command(PlatformCommand::SetHotkey(spec));
        self.recording = false;
        self.hotkey_note = None;
        cx.notify();
    }

    pub(super) fn toggle_autostart(&mut self, next: bool, cx: &mut Context<Self>) {
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

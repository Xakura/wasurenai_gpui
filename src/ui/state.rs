//! 应用状态：待办数据、落盘、自动日志同步、运行期状态（提醒确认）。
//!
//! 这里**不持有窗口句柄、不做窗口操作**（见 `ui/mod.rs` 的硬约束）。
//!
//! 变更分两层：
//! - `set_*` / `add_item` / `remove_item`：纯变更，返回是否有变化，可单测。
//! - `commit_click` / `commit_typing`：由 UI 调用，负责通知重绘 + 防抖落盘。

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Local, NaiveDate, NaiveDateTime};

// 只引入需要的类型：`use gpui_kit::*` 会把 gpui 的 `test` 模块也带进来，
// 与下方测试模块里的 `#[test]` 属性宏打架（recursion limit reached）。
use gpui_kit::Context;

use crate::core::hotkey::HotkeySpec;
use crate::core::model::{Store, TodoId, TodoItem};
use crate::core::{reminder, store, worklog};

pub struct AppState {
    pub store: Store,
    path: PathBuf,
    log_path: PathBuf,
    /// 防抖落盘用：每次修改自增，只有最新一代的定时任务会真正写盘。
    save_generation: u64,
    /// 是否有尚未落盘的防抖修改（退出或关窗时用 `flush_save` 强制写回）。
    dirty: bool,
    /// 本次运行里已经点过「知道了」的提醒项（不持久化：重启后逾期项会再提醒一次）。
    acked: HashSet<TodoId>,
    /// 用户是否用快捷键把窗口浮到了最上层（不持久化）。
    pub user_floated: bool,
    /// 最近一次快捷键注册失败的提示（不持久化）。
    pub hotkey_error: Option<String>,
}

impl AppState {
    /// 从默认路径加载（文件不存在或损坏都不会失败），并初始化同步一次自动日志。
    pub fn load() -> Self {
        let state = Self::load_from(store::data_path());
        if let Err(err) = state.sync_worklog_now() {
            eprintln!(
                "[wasurenai] 初始化同步日志 {} 失败：{err}",
                state.log_path.display()
            );
        }
        state
    }

    /// 指定路径加载，便于测试与将来的"导入"功能。
    pub fn load_from(path: PathBuf) -> Self {
        let log_path = worklog::log_path_for(&path);
        Self {
            store: store::load_from(&path),
            path,
            log_path,
            save_generation: 0,
            dirty: false,
            acked: HashSet::new(),
            user_floated: false,
            hotkey_error: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    // ---- 纯变更（返回是否有变化；不写盘、不通知，便于单测）----

    pub fn set_done(&mut self, id: TodoId, done: bool) -> bool {
        self.set_done_at(id, done, Local::now().naive_local())
    }

    pub fn set_done_at(&mut self, id: TodoId, done: bool, now: NaiveDateTime) -> bool {
        if self.store.set_done_at(id, done, now) {
            if done {
                // 勾完就没必要再提醒了
                self.acked.insert(id);
            }
            println!("[wasurenai] set_done id={id} done={done}");
            true
        } else {
            if self.store.item(id).is_none() {
                eprintln!("[wasurenai] set_done 收到未知 id={id}（元素 id 接错了？）");
            }
            false
        }
    }

    pub fn set_text(&mut self, id: TodoId, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        match self.store.item_mut(id) {
            Some(item) if item.text != text => {
                item.text = text.to_string();
                true
            }
            _ => false,
        }
    }

    pub fn set_due(&mut self, id: TodoId, due: Option<NaiveDateTime>) -> bool {
        let changed = match self.store.item_mut(id) {
            Some(item) if item.due != due => {
                item.due = due;
                true
            }
            _ => false,
        };
        if changed {
            // 改了时间就重新开始提醒
            self.acked.remove(&id);
            println!("[wasurenai] set_due id={id} due={due:?}");
        }
        changed
    }

    pub fn toggle_item_tag(&mut self, id: TodoId, tag: &str) -> bool {
        let changed = self.store.toggle_item_tag(id, tag);
        if changed {
            self.store.prune_tag_filter();
        }
        changed
    }

    pub fn add_predefined_tag(&mut self, raw: &str) -> bool {
        let changed = self.store.add_predefined_tag(raw);
        if changed {
            println!("[wasurenai] add_predefined_tag -> {:?}", self.store.predefined_tags);
        }
        changed
    }

    pub fn remove_predefined_tag(&mut self, tag: &str) -> bool {
        let changed = self.store.remove_predefined_tag(tag);
        if changed {
            println!(
                "[wasurenai] remove_predefined_tag -> {:?}",
                self.store.predefined_tags
            );
        }
        changed
    }

    pub fn add_item(&mut self, text: &str) -> Option<TodoId> {
        let id = self.store.add(text)?;
        println!("[wasurenai] add_item id={id}");
        Some(id)
    }

    /// 一次性创建带截止时间和多标签的新待办。空白文本返回 `None`。
    pub fn add_item_full(
        &mut self,
        text: &str,
        due: Option<NaiveDateTime>,
        tags: &[String],
    ) -> Option<TodoId> {
        let id = self.store.add(text)?;
        self.store.set_due(id, due);
        self.store.set_tags(id, tags);
        println!("[wasurenai] add_item_full id={id} due={due:?} tags={tags:?}");
        Some(id)
    }

    pub fn remove_item(&mut self, id: TodoId) -> bool {
        if self.store.remove(id) {
            self.store.prune_tag_filter();
            self.acked.remove(&id);
            println!("[wasurenai] remove_item id={id}");
            true
        } else {
            false
        }
    }

    pub fn toggle_tag_filter(&mut self, tag: &str) -> bool {
        self.store.toggle_tag_filter(tag);
        println!("[wasurenai] tag_filter -> {:?}", self.store.tag_filter);
        true
    }

    pub fn clear_tag_filter(&mut self) -> bool {
        if self.store.tag_filter.is_some() {
            self.store.tag_filter = None;
            println!("[wasurenai] tag_filter -> None");
            true
        } else {
            false
        }
    }

    /// 跨过零点时清理失效的标签筛选，并通知视图隐藏昨日已完成项。
    pub fn on_day_changed(&mut self, today: NaiveDate) {
        self.store.prune_tag_filter_on(today);
        println!("[wasurenai] 日期切换 -> {today}");
    }

    // ---- 全局快捷键 ----

    /// 当前生效的快捷键（存盘值非法时回落到默认）。
    pub fn hotkey(&self) -> HotkeySpec {
        self.store
            .hotkey
            .as_deref()
            .and_then(|raw| HotkeySpec::parse(raw).ok())
            .unwrap_or_else(HotkeySpec::default_spec)
    }

    /// 设置快捷键（`None` = 恢复默认）。非法输入返回 false 并给出原因。
    pub fn set_hotkey(&mut self, raw: &str) -> Result<HotkeySpec, String> {
        let spec = HotkeySpec::parse(raw)?;
        let canonical = spec.format();
        if self.store.hotkey.as_deref() == Some(canonical.as_str()) {
            return Ok(spec);
        }
        self.store.hotkey = if canonical == crate::core::hotkey::DEFAULT {
            None
        } else {
            Some(canonical.clone())
        };
        println!("[wasurenai] hotkey -> {canonical}");
        self.hotkey_error = None;
        Ok(spec)
    }

    pub fn reset_hotkey(&mut self) -> HotkeySpec {
        self.store.hotkey = None;
        println!("[wasurenai] hotkey -> 默认");
        self.hotkey_error = None;
        HotkeySpec::default_spec()
    }

    // ---- 到期提醒 ----

    /// 当前需要提醒的条目（已到期、未完成、没点过「知道了」），按到期时间升序。
    pub fn due_items(&self, now: NaiveDateTime) -> Vec<&TodoItem> {
        reminder::due_items(&self.store, now, &self.acked)
    }

    pub fn ack(&mut self, id: TodoId) {
        self.acked.insert(id);
        println!("[wasurenai] 提醒已确认 id={id}");
    }

    /// 把所有当前到期的条目一次性确认掉。
    pub fn ack_all(&mut self, now: NaiveDateTime) {
        let ids: Vec<TodoId> = self.due_items(now).into_iter().map(|item| item.id).collect();
        for id in ids {
            self.acked.insert(id);
        }
        println!("[wasurenai] 提醒全部确认");
    }

    // ---- 日志同步与导出 ----

    pub fn sync_worklog_now(&self) -> io::Result<()> {
        worklog::sync_to(&self.store.items, &self.log_path)
    }

    pub fn export_worklog_to(&self, dest: &Path) -> io::Result<()> {
        worklog::sync_to(&self.store.items, dest)
    }

    // ---- UI 入口：通知 + 防抖落盘 ----

    /// 点击类操作：立刻重绘，落盘防抖。
    pub fn commit_click(&mut self, changed: bool, cx: &mut Context<Self>) {
        if !changed {
            return;
        }
        self.schedule_save(cx);
        cx.notify();
    }

    /// 输入类操作：不立刻重绘（避免每次按键都重排整窗），落盘后再通知一次。
    pub fn commit_typing(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            self.schedule_save(cx);
        }
    }

    /// 只通知重绘（提醒确认这类不落盘的状态变化）。
    pub fn commit_runtime(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            cx.notify();
        }
    }

    /// 防抖落盘：350ms 内的连续修改只写一次；写完再通知观察者刷新。
    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.save_generation += 1;
        let generation = self.save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            this.update(cx, |app, cx| {
                if app.save_generation == generation {
                    app.persist_now();
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// 立即刷盘未保存的防抖改动（关窗或退出前调用，避免最后 350ms 内的编辑丢失）。
    pub fn flush_save(&mut self) {
        if !self.dirty {
            return;
        }
        self.save_generation += 1;
        self.persist_now();
    }

    /// 立即写盘并同步自动日志文件。写失败只打印，不让 UI 崩（下次修改还会再试）。
    pub fn persist_now(&mut self) {
        match self.store.save_to(&self.path) {
            Ok(()) => {
                self.dirty = false;
                if let Err(err) = self.sync_worklog_now() {
                    eprintln!(
                        "[wasurenai] 同步日志 {} 失败：{err}",
                        self.log_path.display()
                    );
                }
                println!(
                    "[wasurenai] 已保存（tag_filter={:?} done_ids={:?}）",
                    self.store.tag_filter,
                    self.store
                        .items
                        .iter()
                        .filter(|item| item.done)
                        .map(|item| item.id)
                        .collect::<Vec<_>>()
                );
            }
            Err(err) => eprintln!("[wasurenai] 保存 {} 失败：{err}", self.path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> (Self, PathBuf) {
            let base = std::env::temp_dir().join(format!(
                "wasurenai_ui_state_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).expect("mkdir");
            let file = base.join("todos.json");
            (Self(base), file)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    #[test]
    fn set_done_reports_change_once_and_sets_completed_at() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();

        assert!(state.set_done_at(id, true, dt(25, 14)), "值变化应返回 true");
        assert!(!state.set_done_at(id, true, dt(25, 15)), "值没变应返回 false");
        let item = state.store.item(id).unwrap();
        assert!(item.done);
        assert_eq!(item.completed_at, Some(dt(25, 14)));
    }

    #[test]
    fn set_done_rejects_unknown_id() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        assert!(!state.set_done(999, true));
    }

    #[test]
    fn set_text_trims_and_rejects_blank() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();

        assert!(state.set_text(id, "  改过  "));
        assert_eq!(state.store.item(id).unwrap().text, "改过");
        assert!(!state.set_text(id, "改过"), "内容相同不算变化");
        assert!(!state.set_text(id, "   "), "空白不应清空文本");
        assert_eq!(state.store.item(id).unwrap().text, "改过");
    }

    #[test]
    fn set_due_and_clear() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();

        assert!(state.set_due(id, Some(dt(20, 18))));
        assert!(!state.set_due(id, Some(dt(20, 18))));
        assert!(state.set_due(id, None));
        assert_eq!(state.store.item(id).unwrap().due, None);
    }

    #[test]
    fn toggle_item_tag_and_prunes_filter() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();
        assert!(state.toggle_item_tag(id, "工作"));
        assert!(state.toggle_item_tag(id, "研发"));
        assert_eq!(state.store.item(id).unwrap().tags, vec!["工作", "研发"]);

        state.store.tag_filter = Some("工作".into());
        assert!(state.toggle_item_tag(id, "工作"));
        assert_eq!(state.store.tag_filter, None, "标签取消后筛选应被清理");
    }

    #[test]
    fn add_and_remove_items() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);

        let id = state.add_item("a").expect("应成功");
        assert!(state.remove_item(id));
        assert!(!state.remove_item(id), "重复删除应返回 false");
    }

    #[test]
    fn persist_now_writes_json_and_syncs_worklog() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.add_item("交周报").unwrap();
        state.toggle_item_tag(id, "工作");
        state.set_done_at(id, true, dt(25, 16));

        assert!(!path.exists(), "纯变更不应写盘");
        state.persist_now();

        let reloaded = store::load_from(&path);
        assert!(reloaded.item(id).unwrap().done);

        let log_text = fs::read_to_string(state.log_path()).expect("应自动生成日志文件");
        assert!(log_text.contains("## 2026-11-25"));
        assert!(log_text.contains("- [16:00] 交周报 `#工作`"));
    }

    #[test]
    fn due_items_respects_ack_and_done() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let overdue = state.add_item("逾期").unwrap();
        state.set_due(overdue, Some(dt(19, 9)));
        let finished = state.add_item("已完成").unwrap();
        state.set_due(finished, Some(dt(19, 9)));
        state.set_done(finished, true);

        let now = dt(19, 12);
        let ids: Vec<TodoId> = state.due_items(now).iter().map(|item| item.id).collect();
        assert_eq!(ids, vec![overdue], "已完成的条目不应再提醒");

        state.ack(overdue);
        assert!(state.due_items(now).is_empty(), "确认后不再提醒");

        state.set_due(overdue, Some(dt(19, 10)));
        assert_eq!(state.due_items(now).len(), 1, "改了时间应重新提醒");
    }

    #[test]
    fn completing_item_acks_it() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.add_item("逾期").unwrap();
        state.set_due(id, Some(dt(19, 9)));

        state.set_done(id, true);
        assert!(
            state.due_items(dt(19, 12)).is_empty(),
            "勾选完成等价于确认提醒"
        );
    }

    #[test]
    fn add_item_full_sets_due_and_tags() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let tags = vec!["#工作".to_string(), "研发".to_string()];
        let id = state
            .add_item_full("  提交季报 ", Some(dt(21, 18)), &tags)
            .expect("应成功创建");
        let item = state.store.item(id).unwrap();
        assert_eq!(item.text, "提交季报");
        assert_eq!(item.due, Some(dt(21, 18)));
        assert_eq!(item.tags, vec!["工作", "研发"]);
        assert!(item.created_at.is_some());

        assert_eq!(state.add_item_full("   ", Some(dt(21, 18)), &[]), None);
    }

    #[test]
    fn flush_save_writes_pending_dirty_state() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.add_item("刷盘测试").unwrap();

        state.flush_save();
        assert!(!path.exists(), "非 dirty 状态 flush 不应多余写盘");

        state.dirty = true;
        state.flush_save();
        assert!(!state.dirty);
        let reloaded = store::load_from(&path);
        assert_eq!(reloaded.item(id).unwrap().text, "刷盘测试");
    }
}

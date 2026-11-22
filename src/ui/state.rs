//! 应用状态：待办数据、落盘、运行期状态（提醒确认）。
//!
//! 这里**不持有窗口句柄、不做窗口操作**（见 `ui/mod.rs` 的硬约束）。
//!
//! 变更分两层：
//! - `set_*` / `add_item` / `remove_item`：纯变更，返回是否有变化，可单测。
//! - `commit_click` / `commit_typing`：由 UI 调用，负责通知重绘 + 防抖落盘。

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDateTime;

// 只引入需要的类型：`use gpui_kit::*` 会把 gpui 的 `test` 模块也带进来，
// 与下方测试模块里的 `#[test]` 属性宏打架（recursion limit reached）。
use gpui_kit::Context;

use crate::core::hotkey::HotkeySpec;
use crate::core::model::{Store, TodoId, TodoItem, normalize_tag};
use crate::core::{reminder, store};

pub struct AppState {
    pub store: Store,
    path: PathBuf,
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
    /// 从默认路径加载（文件不存在或损坏都不会失败）。
    pub fn load() -> Self {
        Self {
            store: store::load(),
            path: store::data_path(),
            save_generation: 0,
            dirty: false,
            acked: HashSet::new(),
            user_floated: false,
            hotkey_error: None,
        }
    }

    /// 指定路径加载，便于测试与将来的"导入"功能。
    pub fn load_from(path: PathBuf) -> Self {
        Self {
            store: store::load_from(&path),
            path,
            save_generation: 0,
            dirty: false,
            acked: HashSet::new(),
            user_floated: false,
            hotkey_error: None,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    // ---- 纯变更（返回是否有变化；不写盘、不通知，便于单测）----

    pub fn set_done(&mut self, id: TodoId, done: bool) -> bool {
        match self.store.item_mut(id) {
            Some(item) if item.done != done => {
                item.done = done;
                if done {
                    // 勾完就没必要再提醒了
                    self.acked.insert(id);
                }
                println!("[wasurenai] set_done id={id} done={done}");
                true
            }
            Some(_) => false,
            None => {
                eprintln!("[wasurenai] set_done 收到未知 id={id}（元素 id 接错了？）");
                false
            }
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

    pub fn set_tag(&mut self, id: TodoId, tag: Option<&str>) -> bool {
        let normalized = tag.and_then(normalize_tag);
        let changed = match self.store.item_mut(id) {
            Some(item) if item.tag != normalized => {
                item.tag = normalized;
                true
            }
            _ => false,
        };
        if changed {
            self.store.prune_tag_filter();
        }
        changed
    }

    pub fn add_item(&mut self, text: &str) -> Option<TodoId> {
        let id = self.store.add(text)?;
        println!("[wasurenai] add_item id={id}");
        Some(id)
    }

    /// 一次性创建带截止时间和标签的新待办。空白文本返回 `None`。
    pub fn add_item_full(
        &mut self,
        text: &str,
        due: Option<NaiveDateTime>,
        tag: Option<&str>,
    ) -> Option<TodoId> {
        let id = self.store.add(text)?;
        self.store.set_due(id, due);
        self.store.set_tag(id, tag);
        println!("[wasurenai] add_item_full id={id} due={due:?}");
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

    /// 清除所有已完成的待办，返回清理条数。
    pub fn clear_completed(&mut self) -> usize {
        let mut removed = 0;
        self.store.items.retain(|item| {
            if item.done {
                self.acked.remove(&item.id);
                removed += 1;
                false
            } else {
                true
            }
        });
        if removed > 0 {
            self.store.prune_tag_filter();
            println!("[wasurenai] clear_completed count={removed}");
        }
        removed
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

    /// 立即写盘。写失败只打印，不让 UI 崩（下次修改还会再试）。
    pub fn persist_now(&mut self) {
        match self.store.save_to(&self.path) {
            Ok(()) => {
                self.dirty = false;
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
    use chrono::NaiveDate;
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
    fn set_done_reports_change_once() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();

        assert!(state.set_done(id, true), "值变化应返回 true");
        assert!(!state.set_done(id, true), "值没变应返回 false");
        assert!(state.store.item(id).unwrap().done);
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
    fn set_tag_normalizes_and_prunes_filter() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state.store.add("a").unwrap();
        state.set_tag(id, Some("工作"));
        state.store.tag_filter = Some("工作".into());

        assert!(
            !state.set_tag(id, Some("#工作")),
            "规范化后与现值相同，不算变化"
        );
        assert!(state.set_tag(id, None));
        assert_eq!(state.store.tag_filter, None, "标签清掉后筛选应被清理");
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
    fn persist_now_writes_file() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.add_item("交周报").unwrap();
        state.set_done(id, true);

        assert!(!path.exists(), "纯变更不应写盘");
        state.persist_now();

        let reloaded = store::load_from(&path);
        assert!(reloaded.item(id).unwrap().done);
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
    fn add_item_full_sets_due_and_tag() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let id = state
            .add_item_full("  提交季报 ", Some(dt(21, 18)), Some("#工作"))
            .expect("应成功创建");
        let item = state.store.item(id).unwrap();
        assert_eq!(item.text, "提交季报");
        assert_eq!(item.due, Some(dt(21, 18)));
        assert_eq!(item.tag.as_deref(), Some("工作"));

        assert_eq!(state.add_item_full("   ", Some(dt(21, 18)), None), None);
    }

    #[test]
    fn clear_completed_removes_only_done() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);
        let keep = state.add_item("未完成").unwrap();
        let done_one = state.add_item("已完成1").unwrap();
        let done_two = state.add_item("已完成2").unwrap();
        state.set_tag(done_one, Some("工作"));
        state.store.tag_filter = Some("工作".into());
        state.set_done(done_one, true);
        state.set_done(done_two, true);

        let cleared = state.clear_completed();
        assert_eq!(cleared, 2);
        assert!(state.store.item(keep).is_some());
        assert!(state.store.item(done_one).is_none());
        assert!(state.store.item(done_two).is_none());
        assert_eq!(state.store.tag_filter, None, "被删标签使用为空后应清理筛选");
        assert_eq!(state.clear_completed(), 0);
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

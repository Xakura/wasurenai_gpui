//! 持久化与状态变更。
//!
//! 落盘策略：任何修改后立刻整体写回（结构化编辑频率低，不值得做增量/防抖）。
//! 写入用「临时文件 + rename」保证不会留下半截 JSON。
//! 读到损坏文件时**不 panic**：把原文件备份成 `.bak`，从空 store 重新开始。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, NaiveDateTime};

use crate::core::model::{Store, TodoId, TodoItem, normalize_tag, normalize_tags};
use crate::core::ordering;

/// 数据文件路径：`%APPDATA%\wasurenai_gpui\todos.json`。
///
/// `dirs::data_dir()` 在 Windows 上就是 `%APPDATA%`（Roaming）。
pub fn data_path() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("wasurenai_gpui").join("todos.json")
}

/// 从默认路径读取。文件不存在 → 空 store；文件损坏 → 备份后空 store。
pub fn load() -> Store {
    load_from(&data_path())
}

pub fn load_from(path: &Path) -> Store {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Store::default(),
        Err(err) => {
            eprintln!("[wasurenai] 读取 {} 失败：{err}", path.display());
            return Store::default();
        }
    };

    let has_predefined_key = text.contains("\"predefined_tags\"");
    match serde_json::from_str::<Store>(&text) {
        Ok(mut store) => {
            store.predefined_tags = normalize_tags(store.predefined_tags);
            // 旧版 JSON 没有 predefined_tags 字段：把已有待办上的标签自动吸收为预定义标签，避免升级后标签选项为空。
            if !has_predefined_key {
                for item in &store.items {
                    for tag in &item.tags {
                        if !store.predefined_tags.contains(tag) {
                            store.predefined_tags.push(tag.clone());
                        }
                    }
                }
            }
            store
        }
        Err(err) => {
            let backup = path.with_extension("json.bak");
            eprintln!(
                "[wasurenai] {} 解析失败（{err}），备份到 {} 并从空列表开始",
                path.display(),
                backup.display()
            );
            if let Err(err) = fs::copy(path, &backup) {
                eprintln!("[wasurenai] 备份失败：{err}");
            }
            Store::default()
        }
    }
}

impl Store {
    /// 立即写回默认路径。
    pub fn save(&self) -> io::Result<()> {
        self.save_to(&data_path())
    }

    /// 原子写：先写 `<path>.tmp`，再 rename 覆盖。
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, path)
    }

    pub fn item(&self, id: TodoId) -> Option<&TodoItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn item_mut(&mut self, id: TodoId) -> Option<&mut TodoItem> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    /// 新增一条待办（自动写入当前本地建立时间），返回新 id。空白文本会被拒绝（返回 None）。
    pub fn add(&mut self, text: impl AsRef<str>) -> Option<TodoId> {
        self.add_at(text, Local::now().naive_local())
    }

    /// 新增一条待办并指定建立时间（便于确定性单测）。
    pub fn add_at(&mut self, text: impl AsRef<str>, now: NaiveDateTime) -> Option<TodoId> {
        let text = text.as_ref().trim();
        if text.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(TodoItem::new_at(id, text, now));
        Some(id)
    }

    pub fn remove(&mut self, id: TodoId) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    /// 切换完成状态；勾选完成时记录完成时间并保留 due，取消完成时清空完成时间。
    pub fn toggle(&mut self, id: TodoId) {
        let now = Local::now().naive_local();
        if let Some(item) = self.item_mut(id) {
            item.done = !item.done;
            item.completed_at = if item.done { Some(now) } else { None };
        }
    }

    /// 显式设置完成状态与完成时间。返回值表示是否有变化。
    pub fn set_done_at(&mut self, id: TodoId, done: bool, now: NaiveDateTime) -> bool {
        match self.item_mut(id) {
            Some(item) if item.done != done => {
                item.done = done;
                item.completed_at = if done { Some(now) } else { None };
                true
            }
            _ => false,
        }
    }

    pub fn set_text(&mut self, id: TodoId, text: impl AsRef<str>) {
        let text = text.as_ref().trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(item) = self.item_mut(id) {
            item.text = text;
        }
    }

    pub fn set_due(&mut self, id: TodoId, due: Option<NaiveDateTime>) {
        if let Some(item) = self.item_mut(id) {
            item.due = due;
        }
    }

    /// 整组设置多标签（自动规范化与去重）。
    pub fn set_tags<I, S>(&mut self, id: TodoId, tags: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let normalized = normalize_tags(tags);
        match self.item_mut(id) {
            Some(item) if item.tags != normalized => {
                item.tags = normalized;
                true
            }
            _ => false,
        }
    }

    /// 勾选/取消单条待办上的某个标签。返回值表示是否有变化。
    pub fn toggle_item_tag(&mut self, id: TodoId, raw_tag: &str) -> bool {
        let Some(tag) = normalize_tag(raw_tag) else {
            return false;
        };
        let Some(item) = self.item_mut(id) else {
            return false;
        };
        if let Some(pos) = item.tags.iter().position(|t| t == &tag) {
            item.tags.remove(pos);
        } else {
            item.tags.push(tag);
        }
        true
    }

    /// 新增预定义工作标签（规范化并去重）。返回值表示是否真正新增。
    pub fn add_predefined_tag(&mut self, raw: &str) -> bool {
        let Some(tag) = normalize_tag(raw) else {
            return false;
        };
        if self.predefined_tags.contains(&tag) {
            return false;
        }
        self.predefined_tags.push(tag);
        true
    }

    /// 删除预定义工作标签（仅从预设列表移除，不修改历史待办上的已记录标签）。
    pub fn remove_predefined_tag(&mut self, tag: &str) -> bool {
        let before = self.predefined_tags.len();
        self.predefined_tags.retain(|t| t != tag);
        self.predefined_tags.len() != before
    }

    /// 切换标签筛选：同标签再点一次回到「全部」。
    pub fn toggle_tag_filter(&mut self, tag: &str) {
        self.tag_filter = match self.tag_filter.as_deref() {
            Some(current) if current == tag => None,
            _ => Some(tag.to_string()),
        };
    }

    /// 筛选的标签不再被任何当日可见条目使用时，清掉筛选，避免界面卡在空列表。
    pub fn prune_tag_filter_on(&mut self, today: NaiveDate) {
        if let Some(filter) = self.tag_filter.clone() {
            let still_used = self
                .items
                .iter()
                .filter(|item| ordering::is_visible_on(item, today))
                .any(|item| item.has_tag(&filter));
            if !still_used {
                self.tag_filter = None;
            }
        }
    }

    /// 以今天为基准清理失效标签筛选。
    pub fn prune_tag_filter(&mut self) {
        self.prune_tag_filter_on(Local::now().date_naive());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// 每个测试独立的临时目录，测试结束自动清理。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "wasurenai_test_{name}_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).expect("mkdir temp");
            Self(base)
        }

        fn file(&self) -> PathBuf {
            self.0.join("todos.json")
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
    fn data_path_points_to_appdata_dir() {
        let path = data_path();
        assert!(
            path.ends_with("wasurenai_gpui/todos.json")
                || path.ends_with("wasurenai_gpui\\todos.json")
        );
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = TempDir::new("missing");
        let store = load_from(&dir.file());
        assert_eq!(store, Store::default());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = TempDir::new("roundtrip");
        let mut store = Store::default();
        store.add_predefined_tag("工作");
        store.add_predefined_tag("研发");
        let id = store.add_at("交周报", dt(20, 9)).expect("id");
        store.set_due(id, Some(dt(20, 18)));
        store.set_tags(id, ["#工作", "研发"]);
        store.widget_visible = false;

        store.save_to(&dir.file()).expect("save");
        let loaded = load_from(&dir.file());

        assert_eq!(loaded, store);
        assert_eq!(
            loaded.item(id).unwrap().tags,
            vec!["工作".to_string(), "研发".to_string()]
        );
        assert_eq!(loaded.item(id).unwrap().created_at, Some(dt(20, 9)));
        assert!(
            !dir.file().with_extension("json.tmp").exists(),
            "临时文件应已 rename"
        );
    }

    #[test]
    fn legacy_file_absorbs_tags_into_predefined_once() {
        let dir = TempDir::new("legacy_absorb");
        fs::write(
            dir.file(),
            r#"{ "items": [ { "id": 1, "text": "旧任务", "tag": "工作" } ] }"#,
        )
        .unwrap();

        let mut store = load_from(&dir.file());
        assert_eq!(store.predefined_tags, vec!["工作".to_string()]);

        // 用户手动删除预定义标签并保存后，再次加载不应复活
        assert!(store.remove_predefined_tag("工作"));
        store.save_to(&dir.file()).unwrap();
        let reloaded = load_from(&dir.file());
        assert!(reloaded.predefined_tags.is_empty());
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reset() {
        let dir = TempDir::new("corrupt");
        fs::write(dir.file(), "{ this is not json").unwrap();

        let store = load_from(&dir.file());

        assert_eq!(store, Store::default(), "损坏文件应降级为空列表而不是 panic");
        let backup = dir.file().with_extension("json.bak");
        assert!(backup.exists(), "原文件应被备份");
        assert_eq!(fs::read_to_string(backup).unwrap(), "{ this is not json");
    }

    #[test]
    fn add_assigns_increasing_ids_and_created_at_and_rejects_blank() {
        let mut store = Store::default();
        let first = store.add_at("a", dt(25, 10)).unwrap();
        let second = store.add_at("b", dt(25, 11)).unwrap();

        assert_eq!((first, second), (1, 2));
        assert_eq!(store.next_id, 3);
        assert_eq!(store.item(first).unwrap().created_at, Some(dt(25, 10)));
        assert_eq!(store.add("   "), None, "空白文本不应生成条目");
        assert_eq!(store.items.len(), 2);
    }

    #[test]
    fn remove_reports_whether_something_was_removed() {
        let mut store = Store::default();
        let id = store.add("a").unwrap();

        assert!(store.remove(id));
        assert!(!store.remove(id));
        assert!(store.items.is_empty());
    }

    #[test]
    fn set_done_at_tracks_completed_timestamp_and_keeps_due() {
        let mut store = Store::default();
        let id = store.add_at("a", dt(20, 9)).unwrap();
        store.set_due(id, Some(dt(20, 18)));

        assert!(store.set_done_at(id, true, dt(20, 15)));
        let item = store.item(id).unwrap();
        assert!(item.done);
        assert_eq!(item.due, Some(dt(20, 18)));
        assert_eq!(item.completed_at, Some(dt(20, 15)));

        assert!(store.set_done_at(id, false, dt(20, 16)));
        let item = store.item(id).unwrap();
        assert!(!item.done);
        assert_eq!(item.completed_at, None);
    }

    #[test]
    fn set_text_ignores_blank_but_trims() {
        let mut store = Store::default();
        let id = store.add("a").unwrap();

        store.set_text(id, "  改过  ");
        assert_eq!(store.item(id).unwrap().text, "改过");

        store.set_text(id, "   ");
        assert_eq!(store.item(id).unwrap().text, "改过", "空白不应清空文本");
    }

    #[test]
    fn multi_tag_and_predefined_tag_management() {
        let mut store = Store::default();
        assert!(store.add_predefined_tag("#工作"));
        assert!(!store.add_predefined_tag("工作"), "重复标签不应再添加");
        assert!(!store.add_predefined_tag("   # "), "空白标签应拒绝");
        assert!(store.add_predefined_tag("研发"));
        assert_eq!(store.predefined_tags, vec!["工作", "研发"]);

        let id = store.add("a").unwrap();
        assert!(store.toggle_item_tag(id, "工作"));
        assert!(store.toggle_item_tag(id, "研发"));
        assert_eq!(store.item(id).unwrap().tags, vec!["工作", "研发"]);

        assert!(store.toggle_item_tag(id, "工作"));
        assert_eq!(store.item(id).unwrap().tags, vec!["研发"]);

        assert!(store.remove_predefined_tag("研发"));
        assert_eq!(store.predefined_tags, vec!["工作"]);
        assert_eq!(
            store.item(id).unwrap().tags,
            vec!["研发"],
            "删除预设标签不应抹掉已有待办上的标签"
        );
    }

    #[test]
    fn toggle_tag_filter_switches_between_all_and_tag() {
        let mut store = Store::default();
        store.toggle_tag_filter("工作");
        assert_eq!(store.tag_filter.as_deref(), Some("工作"));

        store.toggle_tag_filter("工作");
        assert_eq!(store.tag_filter, None, "再点同一个标签应回到全部");

        store.toggle_tag_filter("工作");
        store.toggle_tag_filter("生活");
        assert_eq!(store.tag_filter.as_deref(), Some("生活"), "换标签应直接切换");
    }

    #[test]
    fn prune_tag_filter_clears_when_tag_unused_on_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let mut store = Store::default();
        let id = store.add_at("a", dt(24, 9)).unwrap();
        store.set_tags(id, ["工作"]);
        store.tag_filter = Some("工作".into());

        store.prune_tag_filter_on(today);
        assert_eq!(store.tag_filter.as_deref(), Some("工作"), "标签还在时不应清除");

        // 设为昨日完成 → 今日不可见，筛选应清理
        store.set_done_at(id, true, dt(24, 18));
        store.prune_tag_filter_on(today);
        assert_eq!(store.tag_filter, None, "昨日完成项不可见后应清掉筛选");
    }
}

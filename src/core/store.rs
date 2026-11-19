//! 持久化与状态变更。
//!
//! 落盘策略：任何修改后立刻整体写回（结构化编辑频率低，不值得做增量/防抖）。
//! 写入用「临时文件 + rename」保证不会留下半截 JSON。
//! 读到损坏文件时**不 panic**：把原文件备份成 `.bak`，从空 store 重新开始。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

use crate::core::model::{Store, TodoId, TodoItem, normalize_tag};

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

    match serde_json::from_str::<Store>(&text) {
        Ok(store) => store,
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

    /// 新增一条待办，返回新 id。空白文本会被拒绝（返回 None）。
    pub fn add(&mut self, text: impl AsRef<str>) -> Option<TodoId> {
        let text = text.as_ref().trim();
        if text.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(TodoItem::new(id, text));
        Some(id)
    }

    pub fn remove(&mut self, id: TodoId) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    /// 切换完成状态；勾选完成时保留 due（历史信息）。
    pub fn toggle(&mut self, id: TodoId) {
        if let Some(item) = self.item_mut(id) {
            item.done = !item.done;
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

    /// 设置标签，空白或纯 `#` 视为清除标签。
    pub fn set_tag(&mut self, id: TodoId, tag: Option<&str>) {
        let normalized = tag.and_then(normalize_tag);
        if let Some(item) = self.item_mut(id) {
            item.tag = normalized;
        }
    }

    /// 切换标签筛选：同标签再点一次回到「全部」。筛选的标签已不存在时也回到全部。
    pub fn toggle_tag_filter(&mut self, tag: &str) {
        self.tag_filter = match self.tag_filter.as_deref() {
            Some(current) if current == tag => None,
            _ => Some(tag.to_string()),
        };
    }

    /// 筛选的标签不再被任何条目使用时，清掉筛选，避免界面卡在空列表。
    pub fn prune_tag_filter(&mut self) {
        if let Some(filter) = self.tag_filter.clone() {
            let still_used = self
                .items
                .iter()
                .any(|item| item.tag.as_deref() == Some(filter.as_str()));
            if !still_used {
                self.tag_filter = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
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
        assert!(path.ends_with("wasurenai_gpui/todos.json") || path.ends_with("wasurenai_gpui\\todos.json"));
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
        let id = store.add("交周报").expect("id");
        store.set_due(id, Some(dt(20, 18)));
        store.set_tag(id, Some("#工作"));
        store.widget_visible = false;

        store.save_to(&dir.file()).expect("save");
        let loaded = load_from(&dir.file());

        assert_eq!(loaded, store);
        assert_eq!(loaded.item(id).unwrap().tag.as_deref(), Some("工作"));
        assert!(!dir.file().with_extension("json.tmp").exists(), "临时文件应已 rename");
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
    fn add_assigns_increasing_ids_and_rejects_blank() {
        let mut store = Store::default();
        let first = store.add("a").unwrap();
        let second = store.add("b").unwrap();

        assert_eq!((first, second), (1, 2));
        assert_eq!(store.next_id, 3);
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
    fn toggle_flips_done_and_keeps_due() {
        let mut store = Store::default();
        let id = store.add("a").unwrap();
        store.set_due(id, Some(dt(20, 18)));

        store.toggle(id);
        let item = store.item(id).unwrap();
        assert!(item.done);
        assert_eq!(item.due, Some(dt(20, 18)));

        store.toggle(id);
        assert!(!store.item(id).unwrap().done);
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
    fn set_tag_normalizes_and_clears() {
        let mut store = Store::default();
        let id = store.add("a").unwrap();

        store.set_tag(id, Some("#工作"));
        assert_eq!(store.item(id).unwrap().tag.as_deref(), Some("工作"));

        store.set_tag(id, Some("  #  "));
        assert_eq!(store.item(id).unwrap().tag, None, "纯 # 应清除标签");

        store.set_tag(id, None);
        assert_eq!(store.item(id).unwrap().tag, None);
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
    fn prune_tag_filter_clears_when_tag_unused() {
        let mut store = Store::default();
        let id = store.add("a").unwrap();
        store.set_tag(id, Some("工作"));
        store.tag_filter = Some("工作".into());

        store.prune_tag_filter();
        assert_eq!(store.tag_filter.as_deref(), Some("工作"), "标签还在时不应清除");

        store.set_tag(id, None);
        store.prune_tag_filter();
        assert_eq!(store.tag_filter, None, "标签消失后应清掉筛选");
    }
}

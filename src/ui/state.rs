//! 应用状态：持有待办数据与落盘路径。
//!
//! 这里**不持有窗口句柄、不做窗口操作**（见 `ui/mod.rs` 的硬约束）。

use std::path::PathBuf;

use crate::core::model::{Store, TodoId};
use crate::core::store;

pub struct AppState {
    pub store: Store,
    path: PathBuf,
}

impl AppState {
    /// 从默认路径加载（文件不存在或损坏都不会失败）。
    pub fn load() -> Self {
        Self {
            store: store::load(),
            path: store::data_path(),
        }
    }

    /// 指定路径加载，便于测试与将来的"导入"功能。
    pub fn load_from(path: PathBuf) -> Self {
        Self {
            store: store::load_from(&path),
            path,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// 勾选/取消勾选。
    pub fn set_done(&mut self, id: TodoId, done: bool) {
        if let Some(item) = self.store.item_mut(id) {
            if item.done == done {
                return;
            }
            item.done = done;
        }
        self.persist();
    }

    /// 点筛选条上的标签（同一个标签再点一次回到「全部」）。
    pub fn toggle_tag_filter(&mut self, tag: &str) {
        self.store.toggle_tag_filter(tag);
        self.persist();
    }

    pub fn clear_tag_filter(&mut self) {
        if self.store.tag_filter.is_some() {
            self.store.tag_filter = None;
            self.persist();
        }
    }

    /// 立即写盘。写失败只打印，不让 UI 崩（下次修改还会再试）。
    fn persist(&self) {
        if let Err(err) = self.store.save_to(&self.path) {
            eprintln!("[wasurenai] 保存 {} 失败：{err}", self.path.display());
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

    #[test]
    fn set_done_writes_through_to_disk() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.store.add("交周报").unwrap();

        state.set_done(id, true);

        let reloaded = store::load_from(&path);
        assert!(reloaded.item(id).unwrap().done, "勾选后应立即落盘");
    }

    #[test]
    fn set_done_is_noop_when_value_unchanged() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.store.add("交周报").unwrap();

        state.set_done(id, false); // 本来就是未完成
        assert!(!path.exists(), "值没变就不该写盘");
    }

    #[test]
    fn tag_filter_toggles_and_persists() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path.clone());
        let id = state.store.add("交周报").unwrap();
        state.store.set_tag(id, Some("工作"));

        state.toggle_tag_filter("工作");
        assert_eq!(store::load_from(&path).tag_filter.as_deref(), Some("工作"));

        state.toggle_tag_filter("工作");
        assert_eq!(store::load_from(&path).tag_filter, None);
    }

    #[test]
    fn clear_tag_filter_is_noop_when_none() {
        let (_dir, path) = TempDir::new();
        let mut state = AppState::load_from(path);

        state.clear_tag_filter();
        assert_eq!(state.store.tag_filter, None);
    }
}

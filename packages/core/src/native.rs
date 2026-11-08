//! 原生目标的知识库服务实现。
//!
//! 仅在 `native` 特性（Desktop / Mobile）下编译。
//! 包装 [`crate::vault::Vault`]，向 UI 层暴露只读的 [`KnowledgeService`] 接口。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::error::Result;
use crate::service::KnowledgeService;
use crate::types::{LinkRecord, SearchHit};
use crate::vault::Vault;

fn slot() -> &'static Mutex<Option<Vault>> {
    static SLOT: OnceLock<Mutex<Option<Vault>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn revision_counter() -> &'static AtomicU64 {
    static REVISION: AtomicU64 = AtomicU64::new(0);
    &REVISION
}

/// 索引修订号：每批**成功落盘**的增量变更 +1。
///
/// Desktop UI 轮询此值触发自动刷新，而不是让后台监听线程直接写 Dioxus Signal
/// （Signal 必须在 UI runtime 内写入）。纯原子操作，不持有 Vault 锁。
pub fn revision() -> u64 {
    revision_counter().load(Ordering::SeqCst)
}

/// 手动标记一次索引变更（供测试与将来其它写入路径复用）。
pub fn bump_revision() {
    revision_counter().fetch_add(1, Ordering::SeqCst);
}

/// 打开并注册当前知识库（替换已有的）。
pub fn open(path: impl AsRef<std::path::Path>, init_git: bool) -> Result<()> {
    let vault = Vault::open(path, init_git)?;
    let mut guard = slot()
        .lock()
        .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
    *guard = Some(vault);
    Ok(())
}

/// 是否已打开知识库。
pub fn is_open() -> bool {
    slot().lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 重新建立索引。
pub fn reindex() -> Result<crate::vault::IndexReport> {
    let mut guard = slot()
        .lock()
        .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
    let vault = guard
        .as_mut()
        .ok_or_else(|| crate::Error::Config("知识库未打开".into()))?;
    vault.reindex(None)
}

/// 保存后增量更新单个文档。
pub fn update_document(rel_path: &str) -> Result<()> {
    let mut guard = slot()
        .lock()
        .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
    let vault = guard
        .as_mut()
        .ok_or_else(|| crate::Error::Config("知识库未打开".into()))?;
    vault.update_document(rel_path)
}

/// 应用一批文件监听变更（Desktop 后台线程调用）。
///
/// 与 watcher 共用全局 `Vault` 实例，因此不会与 UI 抢 `.kbindex/.lock`。
pub fn apply_changes(
    changes: &[crate::watcher::FileChange],
) -> Result<crate::watcher::ApplyReport> {
    let mut guard = slot()
        .lock()
        .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
    let vault = guard
        .as_mut()
        .ok_or_else(|| crate::Error::Config("知识库未打开".into()))?;
    let report = crate::watcher::apply_changes(vault, changes)?;
    if report.upserted + report.removed > 0 {
        bump_revision();
    }
    Ok(report)
}

/// 提交一次 Git 版本。
pub fn commit(message: &str, author: &str) -> Result<crate::git::CommitInfo> {
    let guard = slot()
        .lock()
        .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
    let vault = guard
        .as_ref()
        .ok_or_else(|| crate::Error::Config("知识库未打开".into()))?;
    vault.commit(message, author)
}

/// 原生知识库服务：通过全局注册表访问当前打开的知识库。
pub struct NativeService;

impl KnowledgeService for NativeService {
    fn root(&self) -> String {
        with(|v| {
            v.root()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default()
    }

    fn document_count(&self) -> usize {
        with(|v| v.documents().map(|d| d.len()).unwrap_or(0)).unwrap_or(0)
    }

    fn documents(&self) -> Vec<crate::types::DocumentRecord> {
        with(|v| v.documents().unwrap_or_default()).unwrap_or_default()
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        with(|v| v.search(query, limit).unwrap_or_default()).unwrap_or_default()
    }

    fn read_document(&self, rel_path: &str) -> Option<String> {
        with(|v| v.read_document(rel_path).ok()).flatten()
    }

    fn backlinks(&self, rel_path: &str) -> Vec<LinkRecord> {
        with(|v| v.backlinks(rel_path).unwrap_or_default()).unwrap_or_default()
    }

    fn outgoing_links(&self, rel_path: &str) -> Vec<LinkRecord> {
        with(|v| v.outgoing_links(rel_path).unwrap_or_default()).unwrap_or_default()
    }

    fn supports_editing(&self) -> bool {
        true
    }

    fn open_editor(&self, rel_path: &str) -> Option<crate::editor::EditSession> {
        with(|v| v.open_editor(rel_path).ok()).flatten()
    }

    fn save_editor(&self, session: &crate::editor::EditSession) -> crate::Result<()> {
        let mut guard = slot()
            .lock()
            .map_err(|_| crate::Error::Config("知识库注册表锁已损坏".into()))?;
        let vault = guard
            .as_mut()
            .ok_or_else(|| crate::Error::Config("知识库未打开".into()))?;
        vault.save_editor(session)?;
        // 内置编辑保存同样要触发 UI 刷新（即使没有外部文件事件）。
        bump_revision();
        Ok(())
    }

    fn supports_external_open(&self) -> bool {
        true
    }

    fn open_externally(&self, rel_path: &str) -> crate::Result<()> {
        match with(|v| v.open_externally(rel_path)) {
            Some(result) => result,
            None => Err(crate::Error::Config("知识库未打开".into())),
        }
    }
}

/// 以只读方式访问已注册的知识库；未打开时返回 None。
fn with<T>(f: impl FnOnce(&Vault) -> T) -> Option<T> {
    let guard = slot().lock().ok()?;
    let vault = guard.as_ref()?;
    Some(f(vault))
}

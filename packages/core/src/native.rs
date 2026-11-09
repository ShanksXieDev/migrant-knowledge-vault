//! 原生目标的知识库服务实现。
//!
//! 仅在 `native` 特性（Desktop / Mobile）下编译。
//! 包装 [`crate::vault::Vault`]，向 UI 层暴露只读的 [`KnowledgeService`] 接口。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::error::Result;
use crate::service::KnowledgeService;
use crate::types::{LinkRecord, ReferenceTarget, SearchHit};
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

    fn search_query(&self, query: &crate::query::SearchQuery) -> Vec<SearchHit> {
        with(|v| v.search_query(query).unwrap_or_default()).unwrap_or_default()
    }

    fn validate_query(&self, query: &crate::query::SearchQuery) -> Option<String> {
        if query.mode != crate::query::QueryMode::Regex {
            return None;
        }
        crate::regex::Regex::new(&query.text).err()
    }

    fn tags_of(&self, rel_path: &str) -> Vec<String> {
        with(|v| v.tags_of(rel_path)).unwrap_or_default()
    }

    fn all_tags(&self) -> Vec<(String, usize)> {
        with(|v| v.all_tags()).unwrap_or_default()
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

    /// 支持在原生 Office 应用内定位（F-06 进阶③）。
    ///
    /// Native 端有本地进程能力；Web 端是预构建只读索引，没有这条路径。
    ///
    /// 但「有本地进程能力」**不等于**「当前平台能唤起原生应用」——
    /// `office_locator::current_platform()` 只认 Windows / macOS / Linux，
    /// 移动端（Android / iOS）返回 `None`。
    /// 因此这里按**平台**判定而不是恒 `true`：
    /// 否则 Mobile 端会渲染出一个点了必然报错的按钮——那属于谎报能力。
    fn supports_office_locate(&self) -> bool {
        crate::office_locator::current_platform().is_some()
    }

    /// 在原生 Office 应用内定位到引用片段（F-06 进阶③）。
    ///
    /// 返回唤起方案（含定位能力与降级原因），供 UI 如实展示。
    fn open_in_office(
        &self,
        rel_path: &str,
        locator: &str,
    ) -> crate::Result<crate::office_locator::OfficeLaunchPlan> {
        match with(|v| v.open_in_office(rel_path, locator)) {
            Some(result) => result,
            None => Err(crate::Error::Config("知识库未打开".into())),
        }
    }

    fn locate(&self, rel_path: &str, query: &str) -> Option<crate::locate::HitLocation> {
        let text = self.read_document(rel_path)?;
        let format = crate::format::Format::from_path(std::path::Path::new(rel_path))
            .unwrap_or(crate::format::Format::Markdown);
        crate::locate::locate_in_segments(
            &crate::locate::segment_text(&text, format),
            query,
        )
    }

    fn graph_snapshot(&self) -> crate::locate::GraphSnapshot {
        let docs = self.documents();
        let nodes: Vec<(String, String)> = docs
            .iter()
            .map(|d| (d.path.clone(), d.title.clone()))
            .collect();
        // 先收集完文档再逐条取出链，避免嵌套持有全局 Vault 锁。
        let links: Vec<(String, String)> = docs
            .iter()
            .flat_map(|d| self.outgoing_links(&d.path))
            .filter(|l| !l.to_path.is_empty())
            .map(|l| (l.from_path, l.to_path))
            .collect();
        crate::locate::build_graph_snapshot(&nodes, &links)
    }

    fn references(&self, rel_path: &str) -> Vec<crate::refs::ReferenceView> {
        // F-06 + F-11 节点定位 + F-06 进阶源变更差异，统一由 Vault 负责：
        // 它同时掌握正文、流程图节点清单与快照基线。
        with(|v| v.references(rel_path).unwrap_or_default()).unwrap_or_default()
    }

    /// 可作为引用目标的文档及其选择器候选（F-06 插入引用入口）。
    ///
    /// **代价**：本方法会读取全部文档的原文并做片段切分，
    /// 因此只应在用户打开「插入引用」面板时按需调用，绝不能放进渲染热路径。
    ///
    /// 读取用 [`Vault::read_document`]（原始字节的宽松 UTF-8），
    /// 与 [`Vault::references`] 解析目标片段时用的是同一份文本，
    /// 这样 UI 里选到的候选序号与真正解析时命中的片段才对得上。
    fn reference_targets(&self) -> Vec<ReferenceTarget> {
        with(|v| {
            v.documents()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|doc| {
                    // 格式认不出来的文档（后缀未知）没有片段语义，跳过。
                    let format = crate::format::Format::from_path(std::path::Path::new(&doc.path))?;
                    // 读失败（文件已删 / 权限问题）只跳过这一篇，不影响其它文档。
                    let text = v.read_document(&doc.path).ok()?;
                    let segments = crate::locate::segment_text(&text, format);
                    let candidates = crate::refs::selector_candidates(&segments);
                    Some(ReferenceTarget {
                        path: doc.path.clone(),
                        title: doc.title.clone(),
                        candidates,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn editor_plugins(&self) -> Vec<crate::plugin::PluginDescriptor> {
        crate::plugin::registry()
            .read()
            .map(|r| r.descriptors())
            .unwrap_or_default()
    }

    fn supports_editor_updates(&self) -> bool {
        true
    }

    /// 启动时调用一次：读取知识库内的运行时状态，探测三个上游 HEAD 并生成计划。
    ///
    /// 探测走 `git ls-remote`，离线 / 超时只会得到空计划，不会阻断应用。
    fn editor_update_plan(&self) -> Option<crate::editor_updates::UpdatePlan> {
        let root = std::path::PathBuf::from(self.root());
        if root.as_os_str().is_empty() {
            return None;
        }
        let lock = crate::editor_updates::load_state(&root);
        let remote = crate::editor_updates::detect_remote_revisions(&lock);
        Some(crate::editor_updates::plan_updates(&lock, &remote))
    }

    /// 持久化用户的升级 / 跳过决定，写入 `.kbindex/editors.json`。
    fn decide_editor_update(
        &self,
        choice: &crate::editor_updates::UpdateChoice,
    ) -> Result<usize> {
        use crate::editor_updates::{apply_choice, load_state, plan_updates, save_state};
        let root = std::path::PathBuf::from(self.root());
        if root.as_os_str().is_empty() {
            return Err(crate::Error::Config("知识库未打开，无法保存插件升级决定".into()));
        }
        let mut lock = load_state(&root);
        let remote = crate::editor_updates::detect_remote_revisions(&lock);
        let plan = plan_updates(&lock, &remote);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("unix:{}", d.as_secs()))
            .unwrap_or_else(|_| "unix:0".to_string());
        let count = apply_choice(&mut lock, &plan, choice, &now);
        if count > 0 {
            save_state(&root, &lock)?;
        }
        Ok(count)
    }
}

/// 以只读方式访问已注册的知识库；未打开时返回 None。
fn with<T>(f: impl FnOnce(&Vault) -> T) -> Option<T> {
    let guard = slot().lock().ok()?;
    let vault = guard.as_ref()?;
    Some(f(vault))
}

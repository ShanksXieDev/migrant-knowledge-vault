//! 文件监听与增量索引（P0）。
//!
//! 目标（PRD 硬指标）：文档保存后**索引与双链 1 秒内更新**。
//!
//! 设计要点：
//! 1. **只处理受支持格式的文档**，过滤 `.kbindex/`、`.git/`、隐藏文件、
//!    Office 临时锁文件（`~$xxx`）—— 否则编辑保存产生的一连串事件会反复触发重建；
//! 2. **去抖合并**：`notify` 在 Windows 上一次保存常给出多个事件，
//!    同一路径在窗口内只保留最后一个动作；
//! 3. **纯内容修改走 `update_document`**（最快）；只有出现新路径或删除时才
//!    重算全库双链 `relink_all`（因为链接目标解析依赖「当前有哪些文档」）；
//! 4. **复用 Vault 的文件锁**：watcher 与 UI 共用同一个 `Vault` 实例，
//!    不另开一个去抢 `.kbindex/.lock`。
//!
//! 本模块只在 `native` 特性下编译（notify 依赖系统 API，wasm32 不可用）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::error::{Error, Result};
use crate::format::Format;
use crate::vault::Vault;

/// 变更动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// 新建或内容修改。
    Upsert,
    /// 删除。
    Remove,
}

/// 一个待处理的文件变更（路径为知识库内相对路径，`/` 分隔）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

/// 一批变更的处理结果。
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub upserted: usize,
    pub removed: usize,
    /// 是否因新路径 / 删除而重算了全库双链。
    pub relinked: bool,
    pub links_found: usize,
    /// 本批次耗时（毫秒），用于核对 PRD 的 1 秒指标。
    pub elapsed_ms: u128,
}

/// 监听选项。
#[derive(Debug, Clone)]
pub struct WatchOptions {
    /// 去抖窗口：一次保存常产生多个事件，窗口内按路径合并。
    pub debounce: Duration,
    /// 每批变更后自动 Git 提交（默认关闭，避免提交噪声）。
    pub auto_commit: bool,
    pub commit_author: String,
    /// 置为 true 时优雅退出监听循环（供测试与将来的「停止监听」按钮使用）。
    pub stop: Arc<AtomicBool>,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(250),
            auto_commit: false,
            commit_author: "mkv-watch".to_string(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// 去抖聚合器：同一路径只保留最后一个动作，窗口静默后才算「就绪」。
#[derive(Debug)]
pub struct Debouncer {
    pending: BTreeMap<String, ChangeKind>,
    last_event: Option<Instant>,
    debounce: Duration,
}

impl Debouncer {
    pub fn new(debounce: Duration) -> Self {
        Self {
            pending: BTreeMap::new(),
            last_event: None,
            debounce,
        }
    }

    /// 记录一个事件；`now` 由调用方传入，便于单元测试使用可控时钟。
    pub fn push(&mut self, path: String, kind: ChangeKind, now: Instant) {
        self.pending.insert(path, kind);
        self.last_event = Some(now);
    }

    pub fn ready(&self, now: Instant) -> bool {
        match self.last_event {
            Some(last) => now.saturating_duration_since(last) >= self.debounce,
            None => false,
        }
    }

    /// 取出并清空当前批次（按路径排序，保证处理顺序稳定）。
    pub fn drain(&mut self) -> Vec<FileChange> {
        self.last_event = None;
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(path, kind)| FileChange { path, kind })
            .collect()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

/// 判断一个相对路径是否值得处理。
///
/// 与 `FsStorage::is_ignored` 的忽略规则保持一致，额外排除 Office 临时锁文件。
pub fn is_watchable(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    for seg in rel.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == crate::KBINDEX_DIR || seg == ".git" {
            return false;
        }
        if seg.starts_with('.') || seg.starts_with("~$") {
            return false;
        }
    }
    match Format::from_path(Path::new(rel)) {
        Some(fmt) => fmt.is_supported(),
        None => false,
    }
}

/// 把绝对路径转成知识库内的、以 `/` 分隔的相对路径。
pub fn relative_path(root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// 把 notify 事件归类为「新建/修改」或「删除」；无关事件返回 None。
///
/// 改名事件（`Modify(Name(..))`）按 Upsert 处理，随后由
/// `apply_changes` 依据磁盘存在性把旧路径翻转为 Remove。
pub fn classify_event(kind: &EventKind) -> Option<ChangeKind> {
    match kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any => {
            Some(ChangeKind::Upsert)
        }
        EventKind::Remove(_) => Some(ChangeKind::Remove),
        EventKind::Access(_) | EventKind::Other => None,
    }
}

/// 把一批变更应用到 Vault。
///
/// - 同路径只保留最后一个动作；
/// - Upsert 时若文件已不在磁盘上（改名事件的旧路径），自动翻转为 Remove；
/// - 出现「新路径」或删除时调用 `relink_all`，修复指向旧路径的悬空双链。
///
/// 返回处理报告（含耗时，可用于核对 1 秒指标）。
pub fn apply_changes(vault: &mut Vault, changes: &[FileChange]) -> Result<ApplyReport> {
    let started = Instant::now();
    let mut report = ApplyReport::default();

    // 批次开始前的文档集合，用于判断「是否出现了新文档」
    let before: BTreeSet<String> = vault
        .documents()?
        .into_iter()
        .map(|d| d.path)
        .collect();

    // 去重：后到的事件覆盖先到的
    let mut merged: BTreeMap<String, ChangeKind> = BTreeMap::new();
    for change in changes {
        if !is_watchable(&change.path) {
            continue;
        }
        merged.insert(change.path.clone(), change.kind.clone());
    }

    let mut has_new_document = false;
    for (path, kind) in merged {
        let exists = vault.file_exists(&path);
        let effective = match kind {
            ChangeKind::Upsert if !exists => ChangeKind::Remove,
            other => other,
        };

        match effective {
            ChangeKind::Upsert => {
                if !before.contains(&path) {
                    has_new_document = true;
                }
                vault.update_document(&path)?;
                report.upserted += 1;
            }
            ChangeKind::Remove => {
                // 已经不在文档表里的删除事件（重复通知）直接跳过，
                // 避免每次都做一次无意义的 relink。
                if before.contains(&path) {
                    vault.remove_document(&path)?;
                    report.removed += 1;
                }
            }
        }
    }

    if has_new_document || report.removed > 0 {
        report.relinked = true;
        report.links_found = vault.relink_all()?;
    }

    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

/// 启动监听（通用版）：由调用方提供「如何应用变更」。
///
/// Desktop 端传入 `native::apply_changes` 复用全局 Vault；
/// CLI 可直接使用 [`run_with_vault`]。函数会一直阻塞到 channel 断开，
/// 适合放在后台线程里运行。
pub fn run_with<F, G>(
    root: &Path,
    options: &WatchOptions,
    mut apply: F,
    mut on_batch: G,
) -> Result<()>
where
    F: FnMut(&[FileChange]) -> Result<ApplyReport>,
    G: FnMut(&ApplyReport),
{
    let root = root.to_path_buf();
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )
    .map_err(|e| Error::Config(format!("创建文件监听器失败: {e}")))?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| Error::Config(format!("监听目录失败 {}: {e}", root.display())))?;

    // RecommendedWatcher 必须在整个监听周期内存活（Drop 会停止监听）
    let _keep_alive = watcher;

    let mut debouncer = Debouncer::new(options.debounce);
    let poll = std::cmp::min(options.debounce, Duration::from_millis(50));

    loop {
        if options.stop.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(poll) {
            Ok(Ok(event)) => {
                let Some(kind) = classify_event(&event.kind) else {
                    continue;
                };
                for abs in &event.paths {
                    let Some(rel) = relative_path(&root, abs) else {
                        continue;
                    };
                    if is_watchable(&rel) {
                        debouncer.push(rel, kind.clone(), Instant::now());
                    }
                }
            }
            Ok(Err(e)) => {
                // 单个事件错误不应终止监听
                eprintln!("[mkv-watch] 事件错误: {e}");
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if debouncer.ready(Instant::now()) {
            let batch = debouncer.drain();
            if batch.is_empty() {
                continue;
            }
            match apply(&batch) {
                Ok(report) => on_batch(&report),
                Err(e) => eprintln!("[mkv-watch] 增量更新失败: {e}"),
            }
        }
    }
    Ok(())
}

/// 启动监听（自带 Vault）：CLI `mkv watch` 使用。
///
/// 打开库时即持有了 `.kbindex/.lock`，与 Desktop 端互斥 —— 这是有意的，
/// 索引目录同一时刻只允许一个写入者（决策：文件锁）。
pub fn run_with_vault<G>(
    root: &Path,
    options: &WatchOptions,
    mut on_batch: G,
) -> Result<()>
where
    G: FnMut(&ApplyReport),
{
    let mut vault = Vault::open(root, false)?;
    let auto_commit = options.auto_commit;
    let author = options.commit_author.clone();

    run_with(
        root,
        options,
        |changes| {
            let report = apply_changes(&mut vault, changes)?;
            if auto_commit && report.upserted + report.removed > 0 {
                // 自动提交失败不影响索引正确性，只提示
                if let Err(e) = vault.commit(
                    &format!(
                        "auto: +{} ~{} -{}",
                        report.upserted, report.upserted, report.removed
                    ),
                    &author,
                ) {
                    eprintln!("[mkv-watch] 自动提交失败: {e}");
                }
            }
            Ok(report)
        },
        |report| on_batch(report),
    )
}

/// 供 CLI / Desktop 打印的一行摘要。
pub fn describe_report(report: &ApplyReport) -> String {
    format!(
        "+{} 新建/更新 · -{} 删除 · {} 条链接{} · {} ms",
        report.upserted,
        report.removed,
        report.links_found,
        if report.relinked { "（已重算双链）" } else { "" },
        report.elapsed_ms
    )
}

/// 监听根目录的规范化（用于 CLI 输出）。
pub fn normalize_root(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_index_git_hidden_and_temp_files() {
        assert!(is_watchable("笔记/模流分析.md"));
        assert!(is_watchable("台账.xlsx"));
        assert!(!is_watchable(".kbindex/meta.db"));
        assert!(!is_watchable(".git/index"));
        assert!(!is_watchable(".hidden.md"));
        assert!(!is_watchable("~$台账.xlsx"));
        assert!(!is_watchable("图片.png"));
        assert!(!is_watchable(""));
        assert!(!is_watchable("子目录/.git/config"));
    }

    #[test]
    fn debouncer_merges_same_path_by_last_action() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_millis(200));
        d.push("a.md".into(), ChangeKind::Upsert, t0);
        d.push("a.md".into(), ChangeKind::Remove, t0);
        d.push("b.md".into(), ChangeKind::Upsert, t0);

        assert!(!d.ready(t0 + Duration::from_millis(100)));
        assert!(d.ready(t0 + Duration::from_millis(250)));

        let batch = d.drain();
        assert_eq!(batch.len(), 2);
        assert!(batch.iter().any(|c| c.path == "a.md" && c.kind == ChangeKind::Remove));
        assert!(batch.iter().any(|c| c.path == "b.md" && c.kind == ChangeKind::Upsert));
        assert!(d.drain().is_empty(), "drain 后应清空");
    }

    #[test]
    fn classifies_event_kinds() {
        assert_eq!(classify_event(&EventKind::Any), Some(ChangeKind::Upsert));
        assert_eq!(
            classify_event(&EventKind::Create(notify::event::CreateKind::Any)),
            Some(ChangeKind::Upsert)
        );
        assert_eq!(
            classify_event(&EventKind::Remove(notify::event::RemoveKind::Any)),
            Some(ChangeKind::Remove)
        );
        assert_eq!(
            classify_event(&EventKind::Access(notify::event::AccessKind::Any)),
            None
        );
    }

    #[test]
    fn relative_path_normalizes_separators() {
        let root = Path::new("C:/vault");
        assert_eq!(
            relative_path(root, &root.join("子目录").join("a.md")),
            Some("子目录/a.md".to_string())
        );
        assert_eq!(relative_path(root, root), None);
    }
}

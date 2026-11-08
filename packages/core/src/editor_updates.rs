//! 上游编辑器版本检测与「用户确认后升级」的状态机。
//!
//! # 三层数据
//!
//! | 层 | 位置 | 作用 |
//! |---|---|---|
//! | 构建期基线 | 仓库根 `editors.lock`（`include_str!` 打包） | 三个上游 pin 的 commit SHA + 契约版本 |
//! | 运行时状态 | 知识库 `.kbindex/editors.json` | 用户确认 / 跳过 / 待重建标记，跨启动持久 |
//! | 远端事实 | 启动时 `git ls-remote` 探测 | 上游默认分支当前 HEAD |
//!
//! # 边界（重要）
//!
//! 插件是**编译期 crate**，运行时无法替换已链接进二进制的代码。因此用户确认
//! 升级后，本模块只负责「把目标 rev 写进运行时状态、标记待重建」；真正的
//! 源码同步与插件重建由 `scripts/sync-editors.sh --apply` / `mkv editors upgrade`
//! 完成，重建后重启应用生效。这样不会给出「点了就立刻换代码」的虚假承诺。
//!
//! 纯逻辑部分不依赖网络与 UI，全部可单测；网络探测只在 `native` 特性下编译。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::plugin::PLUGIN_CONTRACT_VERSION;

/// 打包进二进制的构建期基线。
pub const LOCK_TEXT: &str = include_str!("../../../editors.lock");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamPin {
    pub id: String,
    pub repo: String,
    pub branch: String,
    pub pinned_rev: String,
    pub contract_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingUpgrade {
    pub plugin_id: String,
    pub repo: String,
    pub from_rev: String,
    pub to_rev: String,
    pub confirmed_at: String,
    /// 编译期 crate 必须重建后重启才能生效。
    pub requires_rebuild: bool,
}

/// 某个插件的后端选择（架构闭环的配置载体）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendPin {
    /// 插件 ID（对应 [`UpstreamPin::id`]）。
    pub id: String,
    /// 后端类型：`builtin` / `external`。
    pub kind: String,
    /// 选择理由（写进锁文件，避免后人重复调研）。
    #[serde(default)]
    pub reason: String,
    /// `external` 时的上游可执行程序名（如 `deckcraft-cli`）。
    #[serde(default)]
    pub command: String,
    /// `external` 时的启动参数。
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorsLock {
    pub contract_version: u32,
    #[serde(default)]
    pub plugins: Vec<UpstreamPin>,
    /// 各插件的后端选择。缺省时全部视为 `builtin`。
    #[serde(default)]
    pub backends: Vec<BackendPin>,
    #[serde(default)]
    pub skipped_revisions: Vec<String>,
    #[serde(default)]
    pub pending: Vec<PendingUpgrade>,
}

impl EditorsLock {
    /// 校验契约版本与 pin 完整性。
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != PLUGIN_CONTRACT_VERSION {
            return Err(Error::Config(format!(
                "editors.lock 契约版本 {} 与宿主 {} 不匹配",
                self.contract_version, PLUGIN_CONTRACT_VERSION
            )));
        }
        for p in &self.plugins {
            if p.contract_version != PLUGIN_CONTRACT_VERSION {
                return Err(Error::Config(format!(
                    "插件 {} 的上游 pin 契约版本不匹配",
                    p.id
                )));
            }
            if p.pinned_rev.len() < 7 || p.repo.is_empty() {
                return Err(Error::Config(format!("插件 {} 的上游 pin 不完整", p.id)));
            }
        }
        for b in &self.backends {
            if b.kind != "builtin" && b.kind != "external" {
                return Err(Error::Config(format!(
                    "插件 {} 的后端类型 `{}` 非法（应为 builtin / external）",
                    b.id, b.kind
                )));
            }
            if b.kind == "external" && b.command.trim().is_empty() {
                return Err(Error::Config(format!(
                    "插件 {} 选择了 external 后端但未配置上游命令",
                    b.id
                )));
            }
        }
        Ok(())
    }

    /// 取某插件的后端配置；未配置时返回默认的 `builtin`。
    pub fn backend_of(&self, plugin_id: &str) -> BackendPin {
        self.backends
            .iter()
            .find(|b| b.id == plugin_id)
            .cloned()
            .unwrap_or_else(|| BackendPin {
                id: plugin_id.to_string(),
                kind: "builtin".to_string(),
                reason: String::new(),
                command: String::new(),
                args: Vec::new(),
            })
    }
}

/// 解析锁文件文本。
pub fn parse_lock(text: &str) -> Result<EditorsLock> {
    let lock: EditorsLock = serde_json::from_str(text)
        .map_err(|e| Error::Config(format!("editors.lock 解析失败: {e}")))?;
    lock.validate()?;
    Ok(lock)
}

/// 构建期基线（随应用分发，不需要外部文件）。
pub fn bundled_lock() -> Result<EditorsLock> {
    parse_lock(LOCK_TEXT)
}

/// 运行时状态文件路径。
pub fn state_path(root: &Path) -> PathBuf {
    root.join(crate::KBINDEX_DIR).join("editors.json")
}

/// 读取运行时状态；不存在 / 损坏时回落到构建期基线。
pub fn load_state(root: &Path) -> EditorsLock {
    std::fs::read_to_string(state_path(root))
        .ok()
        .and_then(|s| parse_lock(&s).ok())
        .unwrap_or_else(|| bundled_lock().expect("内置的 editors.lock 必须有效"))
}

/// 持久化运行时状态（父目录不存在时创建）。
pub fn save_state(root: &Path, lock: &EditorsLock) -> Result<()> {
    lock.validate()?;
    let path = state_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Config(format!("创建插件状态目录失败: {e}")))?;
    }
    let text = serde_json::to_string_pretty(lock)
        .map_err(|e| Error::Config(format!("序列化插件状态失败: {e}")))?;
    std::fs::write(&path, text)
        .map_err(|e| Error::Config(format!("写入 {} 失败: {e}", path.display())))
}

// ---------------------------------------------------------------- 检测

/// 远端探测结果（一次启动检测得到一组）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRevision {
    pub id: String,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateItem {
    pub id: String,
    pub repo: String,
    pub from_rev: String,
    pub to_rev: String,
    /// 该 rev 曾被用户明确跳过。
    pub skipped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePlan {
    pub items: Vec<UpdateItem>,
}

impl UpdatePlan {
    /// 是否存在需要提示用户的更新（跳过的版本不算）。
    pub fn has_updates(&self) -> bool {
        self.items.iter().any(|i| !i.skipped)
    }

    pub fn actionable_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| !i.skipped)
            .map(|i| i.id.clone())
            .collect()
    }
}

/// 把远端事实与本地 pin 对比，生成更新计划。
///
/// - 远端探测失败（返回空）→ 空计划，应用静默降级，不打断主流程；
/// - 相同 rev → 不列入；
/// - 被跳过的 rev 仍列出但标记 `skipped = true`，供 UI 显示「已跳过」。
pub fn plan_updates(lock: &EditorsLock, remote: &[RemoteRevision]) -> UpdatePlan {
    let mut items = Vec::new();
    for pin in &lock.plugins {
        let Some(r) = remote.iter().find(|r| r.id == pin.id) else {
            continue;
        };
        if r.rev == pin.pinned_rev || r.rev.is_empty() {
            continue;
        }
        items.push(UpdateItem {
            id: pin.id.clone(),
            repo: pin.repo.clone(),
            from_rev: pin.pinned_rev.clone(),
            to_rev: r.rev.clone(),
            skipped: lock.skipped_revisions.contains(&r.rev),
        });
    }
    UpdatePlan { items }
}

// ---------------------------------------------------------------- 用户决定

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateChoice {
    /// 确认升级这些插件（写入目标 rev + 待重建标记）。
    Upgrade { ids: Vec<String> },
    /// 跳过这些版本（同一 rev 不再提示）。
    Skip { ids: Vec<String> },
    /// 稍后再说（不改变任何状态）。
    Defer,
}

/// 应用用户决定，返回受影响的插件数。
pub fn apply_choice(
    lock: &mut EditorsLock,
    plan: &UpdatePlan,
    choice: &UpdateChoice,
    confirmed_at: &str,
) -> usize {
    match choice {
        UpdateChoice::Defer => 0,
        UpdateChoice::Skip { ids } => {
            let mut n = 0;
            for item in plan.items.iter().filter(|i| ids.contains(&i.id)) {
                if !lock.skipped_revisions.contains(&item.to_rev) {
                    lock.skipped_revisions.push(item.to_rev.clone());
                    n += 1;
                }
            }
            n
        }
        UpdateChoice::Upgrade { ids } => {
            let mut n = 0;
            for item in plan.items.iter().filter(|i| ids.contains(&i.id) && !i.skipped) {
                if let Some(pin) = lock.plugins.iter_mut().find(|p| p.id == item.id) {
                    pin.pinned_rev = item.to_rev.clone();
                    lock.pending.retain(|p| p.plugin_id != item.id);
                    lock.pending.push(PendingUpgrade {
                        plugin_id: item.id.clone(),
                        repo: item.repo.clone(),
                        from_rev: item.from_rev.clone(),
                        to_rev: item.to_rev.clone(),
                        confirmed_at: confirmed_at.to_string(),
                        requires_rebuild: true,
                    });
                    n += 1;
                }
            }
            n
        }
    }
}

/// 重建完成后清除待重建标记（由 `mkv editors upgrade` 调用）。
pub fn clear_pending(lock: &mut EditorsLock, ids: &[String]) -> usize {
    let before = lock.pending.len();
    lock.pending
        .retain(|p| !ids.is_empty() && !ids.contains(&p.plugin_id));
    before - lock.pending.len()
}

// ---------------------------------------------------------------- 远端探测

/// 通道一：`git ls-remote`（走 git 智能 HTTP，支持用户已有的 git 代理 / 凭据）。
#[cfg(feature = "native")]
fn probe_via_git(repo: &str, branch: &str) -> Option<String> {
    let url = format!("https://github.com/{repo}.git");
    let spec = format!("refs/heads/{branch}");
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "http.lowSpeedLimit=1000",
            "-c",
            "http.lowSpeedTime=15",
            "ls-remote",
            &url,
            &spec,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let rev = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()?
        .trim()
        .to_string();
    (rev.len() >= 7).then_some(rev)
}

/// 通道二：GitHub commits API。
///
/// 某些网络环境（实测本机代理）会对 git 的 CONNECT 隧道返回 502，
/// 但普通 HTTPS GET 正常；此时用系统 `curl` 读 REST API 作为回退。
/// 刻意不引入 HTTP / TLS crate，避免把 ring / rustls 带进依赖树。
#[cfg(feature = "native")]
fn probe_via_api(repo: &str, branch: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{repo}/commits/{branch}");
    let out = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "15",
            "-H",
            "Accept: application/vnd.github+json",
            &url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let rev = value.get("sha")?.as_str()?.to_string();
    (rev.len() >= 7).then_some(rev)
}

/// 启动时的远端探测：git 优先、API 回退。
///
/// 任一仓库两个通道都失败时静默跳过（离线 / 代理异常），不影响其它仓库，
/// 也不影响应用启动；此时得到空计划，升级横幅不出现。
#[cfg(feature = "native")]
pub fn detect_remote_revisions(lock: &EditorsLock) -> Vec<RemoteRevision> {
    lock.plugins
        .iter()
        .filter_map(|pin| {
            let rev = probe_via_git(&pin.repo, &pin.branch)
                .or_else(|| probe_via_api(&pin.repo, &pin.branch))?;
            Some(RemoteRevision {
                id: pin.id.clone(),
                rev,
            })
        })
        .collect()
}

/// 非 native 目标（Web 只读端）不检测。
#[cfg(not(feature = "native"))]
pub fn detect_remote_revisions(_lock: &EditorsLock) -> Vec<RemoteRevision> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> EditorsLock {
        bundled_lock().unwrap()
    }

    #[test]
    fn bundled_lock_is_valid_and_has_three_real_pins() {
        let l = lock();
        assert_eq!(l.contract_version, PLUGIN_CONTRACT_VERSION);
        assert_eq!(l.plugins.len(), 3);
        let mut repos: Vec<&str> = l.plugins.iter().map(|p| p.repo.as_str()).collect();
        repos.sort();
        assert_eq!(
            repos,
            vec![
                "storytold/deckcraft",
                "storytold/gridcraft",
                "storytold/wordcraft"
            ]
        );
        assert!(l.plugins.iter().all(|p| p.pinned_rev.len() == 40));
    }

    #[test]
    fn bundled_lock_declares_builtin_backends_with_reason() {
        let l = lock();
        assert_eq!(l.backends.len(), 3, "三个插件都应有后端配置");
        for b in &l.backends {
            assert_eq!(b.kind, "builtin", "{} 默认应为 builtin", b.id);
            assert!(!b.reason.is_empty(), "{} 应记录选择理由", b.id);
        }
        // 未配置的插件回落到 builtin
        let fallback = l.backend_of("not-configured");
        assert_eq!(fallback.kind, "builtin");
        assert!(fallback.command.is_empty());
    }

    #[test]
    fn external_backend_without_command_is_rejected() {
        let mut l = lock();
        l.backends.push(BackendPin {
            id: "gridcraft".into(),
            kind: "external".into(),
            reason: String::new(),
            command: String::new(),
            args: vec![],
        });
        assert!(l.validate().is_err(), "external 后端缺命令应被拒绝");
    }

    #[test]
    fn unknown_backend_kind_is_rejected() {
        let mut l = lock();
        l.backends.push(BackendPin {
            id: "gridcraft".into(),
            kind: "quantum".into(),
            reason: String::new(),
            command: "x".into(),
            args: vec![],
        });
        assert!(l.validate().is_err(), "未知后端类型应被拒绝");
    }

    #[test]
    fn plan_ignores_same_rev_and_failed_probes() {
        let l = lock();
        let same: Vec<RemoteRevision> = l
            .plugins
            .iter()
            .map(|p| RemoteRevision {
                id: p.id.clone(),
                rev: p.pinned_rev.clone(),
            })
            .collect();
        assert!(!plan_updates(&l, &same).has_updates());
        assert!(!plan_updates(&l, &[]).has_updates());

        let mut changed = same.clone();
        changed[0].rev = "a".repeat(40);
        let plan = plan_updates(&l, &changed);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].id, "gridcraft");
        assert_eq!(plan.actionable_ids(), vec!["gridcraft"]);
    }

    #[test]
    fn confirm_upgrade_writes_target_rev_and_pending_rebuild() {
        let mut l = lock();
        let mut remote: Vec<RemoteRevision> = l
            .plugins
            .iter()
            .map(|p| RemoteRevision {
                id: p.id.clone(),
                rev: p.pinned_rev.clone(),
            })
            .collect();
        remote[1].rev = "b".repeat(40);
        let plan = plan_updates(&l, &remote);
        let n = apply_choice(
            &mut l,
            &plan,
            &UpdateChoice::Upgrade {
                ids: plan.actionable_ids(),
            },
            "2026-11-08T14:00:00Z",
        );
        assert_eq!(n, 1);
        assert_eq!(l.plugins[1].pinned_rev, "b".repeat(40));
        assert_eq!(l.pending.len(), 1);
        assert!(l.pending[0].requires_rebuild);
        // 已确认后不再提示
        let after = plan_updates(&l, &remote);
        assert!(!after.has_updates());
    }

    #[test]
    fn skip_is_persisted_and_blocks_future_prompts() {
        let mut l = lock();
        let mut remote: Vec<RemoteRevision> = l
            .plugins
            .iter()
            .map(|p| RemoteRevision {
                id: p.id.clone(),
                rev: p.pinned_rev.clone(),
            })
            .collect();
        remote[0].rev = "c".repeat(40);
        let plan = plan_updates(&l, &remote);
        let n = apply_choice(
            &mut l,
            &plan,
            &UpdateChoice::Skip {
                ids: plan.actionable_ids(),
            },
            "now",
        );
        assert_eq!(n, 1);
        let plan2 = plan_updates(&l, &remote);
        assert!(plan2.items[0].skipped);
        assert!(!plan2.has_updates());
        assert!(l.pending.is_empty());
    }

    #[test]
    fn defer_changes_nothing_and_clear_pending_removes_marks() {
        let mut l = lock();
        let n = apply_choice(&mut l, &UpdatePlan::default(), &UpdateChoice::Defer, "now");
        assert_eq!(n, 0);
        l.pending.push(PendingUpgrade {
            plugin_id: "gridcraft".into(),
            repo: "storytold/gridcraft".into(),
            from_rev: "x".into(),
            to_rev: "y".into(),
            confirmed_at: "now".into(),
            requires_rebuild: true,
        });
        assert_eq!(clear_pending(&mut l, &["gridcraft".into()]), 1);
        assert!(l.pending.is_empty());
    }

    #[test]
    fn state_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("mkv_editors_state_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load_state(&dir).plugins.len(), 3);
        let mut l = load_state(&dir);
        l.skipped_revisions.push("f".repeat(40));
        save_state(&dir, &l).unwrap();
        assert_eq!(load_state(&dir), l);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

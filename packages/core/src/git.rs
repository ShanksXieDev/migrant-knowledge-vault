//! Git 版本管理：基于 git2。
//!
//! 见技术方案设计 §5.5：知识库根目录即 Git 仓库；`.kbindex/` 不入库。

use crate::error::{Error, Result};
use git2::{Repository, StatusOptions};
use std::path::Path;

/// 一次提交的信息。
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

/// Git 操作封装。
pub struct GitRepo {
    repo: Repository,
}

impl GitRepo {
    /// 打开已存在的仓库。
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::open(path).map_err(|e| Error::Config(format!("打开 Git 仓库失败: {e}")))?;
        Ok(Self { repo })
    }

    /// 初始化仓库（幂等：已存在则直接打开）。
    pub fn init_or_open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
        if let Ok(repo) = Repository::open(path) {
            return Ok(Self { repo });
        }
        let repo = Repository::init(path)
            .map_err(|e| Error::Config(format!("初始化 Git 仓库失败: {e}")))?;
        Ok(Self { repo })
    }

    /// 写入 `.gitignore`，排除索引目录与临时文件。
    pub fn ensure_gitignore(&self) -> Result<()> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| Error::Config("不是工作区仓库".into()))?;
        let gi = workdir.join(".gitignore");
        let content = format!(
            "# Migrant Knowledge Vault\n\
             # 派生索引目录，不入库（可一键重建）\n\
             {}/\n\
             \n\
             # 系统与编辑器临时文件\n\
             .DS_Store\n\
             Thumbs.db\n\
             *~\n\
             .*.swp\n",
            crate::KBINDEX_DIR
        );
        if !gi.exists() {
            std::fs::write(&gi, content).map_err(|e| Error::io(gi, e))?;
        }
        Ok(())
    }

    /// 暂存全部改动。
    pub fn stage_all(&self) -> Result<()> {
        let mut index = self.repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        Ok(())
    }

    /// 提交改动。
    pub fn commit(&self, message: &str, author: &str) -> Result<CommitInfo> {
        self.stage_all()?;
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        let sig = git2::Signature::now(author, author)?;

        // 仓库可能尚无任何提交（unborn branch），此时 head() 会失败，
        // 按「无父提交」处理即可创建首个提交。
        let parent = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok());
        let parent_refs: Vec<&git2::Commit<'_>> = parent.iter().collect();

        // 显式指定要更新的引用。
        // update_ref 传 None 时，libgit2 只创建 commit 对象、不动任何引用，
        // HEAD 会一直停在「未出生分支」状态（首次提交后 git log 读不到历史）；
        // 传具体分支名则由 libgit2 负责创建/更新该引用。
        let ref_name = self.head_ref_name();
        let oid = self
            .repo
            .commit(Some(&ref_name), &sig, &sig, message, &tree, &parent_refs)?;

        let commit = self.repo.find_commit(oid)?;
        Ok(CommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string().chars().take(8).collect(),
            message: message.to_string(),
            author: author.to_string(),
            timestamp: commit.time().seconds(),
        })
    }

    /// 当前分支的完整引用名（如 `refs/heads/master`）。
    ///
    /// 已有提交时直接取 HEAD 指向的分支；仓库尚无提交（unborn branch）时
    /// HEAD 是符号引用且目标分支尚不存在，`repo.head()` 会失败，需要
    /// 读取 HEAD 的符号引用目标。
    fn head_ref_name(&self) -> String {
        if let Ok(head) = self.repo.head() {
            if let Ok(name) = head.name() {
                return name.to_string();
            }
        }
        // unborn branch：HEAD 为符号引用，目标即分支名
        if let Ok(head_ref) = self.repo.find_reference("HEAD") {
            if let Ok(Some(target)) = head_ref.symbolic_target() {
                if !target.is_empty() {
                    return target.to_string();
                }
            }
        }
        "refs/heads/master".to_string()
    }

    /// 提交历史（最新在前）。
    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut out = Vec::new();
        let mut walk = self.repo.revwalk()?;
        walk.set_sorting(git2::Sort::TIME)?;
        walk.push_head()?;
        for oid in walk.flatten().take(limit) {
            let commit = self.repo.find_commit(oid)?;
            out.push(CommitInfo {
                id: oid.to_string(),
                short_id: oid.to_string().chars().take(8).collect(),
                message: commit.message().unwrap_or_default().to_string(),
                author: commit.author().name().unwrap_or_default().to_string(),
                timestamp: commit.time().seconds(),
            });
        }
        Ok(out)
    }

    /// 工作区是否干净（无未提交改动）。
    pub fn is_clean(&self) -> Result<bool> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        let statuses = self.repo.statuses(Some(&mut opts))?;
        Ok(statuses.is_empty())
    }

    /// 未提交改动的文件列表。
    pub fn changed_files(&self) -> Result<Vec<String>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        let statuses = self.repo.statuses(Some(&mut opts))?;
        let mut out = Vec::new();
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or_default().to_string();
            if path.starts_with(crate::KBINDEX_DIR) {
                continue;
            }
            out.push(path);
        }
        Ok(out)
    }

    /// 回滚到指定提交（保留工作区文件为该提交内容）。
    pub fn checkout_commit(&self, commit_id: &str) -> Result<()> {
        let obj = self
            .repo
            .revparse_single(commit_id)
            .map_err(|e| Error::Config(format!("无效的提交: {e}")))?;
        let commit = obj
            .peel_to_commit()
            .map_err(|e| Error::Config(format!("无效的提交: {e}")))?;
        let tree = commit.tree()?;
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| Error::Config("不是工作区仓库".into()))?;

        // 清空索引并按该提交重建
        let mut index = git2::Index::new()?;
        index.read_tree(&tree)?;
        let mut checkout_opts = git2::build::CheckoutBuilder::new();
        checkout_opts.force().remove_untracked(false);
        self.repo
            .checkout_index(Some(&mut index), Some(&mut checkout_opts))?;

        // 更新 HEAD 指向该提交
        self.repo
            .set_head_detached(commit.id())
            .map_err(|e| Error::Config(format!("切换 HEAD 失败: {e}")))?;
        let _ = workdir;
        Ok(())
    }

    /// 两个提交之间的差异摘要。
    pub fn diff_summary(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let from_tree = self
            .repo
            .revparse_single(from)
            .and_then(|o| o.peel_to_commit())
            .map_err(|e| Error::Config(format!("无效的提交 {from}: {e}")))?
            .tree()?;
        let to_tree = self
            .repo
            .revparse_single(to)
            .and_then(|o| o.peel_to_commit())
            .map_err(|e| Error::Config(format!("无效的提交 {to}: {e}")))?
            .tree()?;
        let diff = self
            .repo
            .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;
        let mut out = Vec::new();
        for delta in diff.deltas() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let status = match delta.status() {
                git2::Delta::Added => "新增",
                git2::Delta::Modified => "修改",
                git2::Delta::Deleted => "删除",
                git2::Delta::Renamed => "重命名",
                _ => "变更",
            };
            out.push(format!("{status}: {path}"));
        }
        Ok(out)
    }
}

//! 存储抽象：隔离「本地文件系统」与「Web 存储」的差异。
//!
//! 见技术方案设计 §4：
//! - `FsStorage`（Desktop / Mobile）：直接读写文件系统 + `.kbindex/`。
//! - Web 端使用 `WebStorage`（OPFS / IndexedDB），实现放在 Web crate 内。

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// 知识库存储后端。
///
/// 路径一律使用**相对知识库根目录**的 `/` 分隔字符串，保证跨端一致。
pub trait Storage: Send + Sync {
    /// 读取相对路径下的字节内容。
    fn read(&self, rel_path: &str) -> Result<Vec<u8>>;

    /// 写入相对路径下的字节内容（自动创建父目录）。
    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()>;

    /// 判断相对路径是否存在。
    fn exists(&self, rel_path: &str) -> bool;

    /// 删除相对路径（文件或目录）。
    fn remove(&self, rel_path: &str) -> Result<()>;

    /// 列出目录下的一级条目（相对路径）。
    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>>;

    /// 递归遍历，产出全部文件（相对路径）。
    fn walk_files(&self) -> Result<Vec<String>>;

    /// 文件最后修改时间（Unix 秒）；不存在返回 None。
    fn modified_at(&self, rel_path: &str) -> Result<Option<u64>>;

    /// 文件大小（字节）；不存在返回 None。
    fn file_size(&self, rel_path: &str) -> Result<Option<u64>>;

    /// 取得绝对路径（仅 Native 后端支持；Web 后端返回 None）。
    fn absolute_path(&self, rel_path: &str) -> Option<PathBuf>;
}

/// 本地文件系统后端。
#[derive(Debug, Clone)]
pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 将相对路径解析为绝对路径，并阻止路径逃逸出知识库根目录。
    pub fn resolve(&self, rel_path: &str) -> Result<PathBuf> {
        let cleaned = rel_path.replace('\\', "/");
        let mut abs = self.root.clone();
        for seg in cleaned.split('/') {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                return Err(Error::Config(format!(
                    "非法路径（试图逃逸出知识库根目录）: {rel_path}"
                )));
            }
            abs.push(seg);
        }
        Ok(abs)
    }
}

impl Storage for FsStorage {
    fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        let abs = self.resolve(rel_path)?;
        std::fs::read(&abs).map_err(|e| Error::io(abs, e))
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        let abs = self.resolve(rel_path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
        }
        std::fs::write(&abs, data).map_err(|e| Error::io(abs, e))
    }

    fn exists(&self, rel_path: &str) -> bool {
        self.resolve(rel_path)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        let abs = self.resolve(rel_path)?;
        if abs.is_dir() {
            std::fs::remove_dir_all(&abs).map_err(|e| Error::io(abs, e))
        } else {
            std::fs::remove_file(&abs).map_err(|e| Error::io(abs, e))
        }
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let abs = self.resolve(rel_path)?;
        let mut out = Vec::new();
        if !abs.is_dir() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(&abs).map_err(|e| Error::io(abs.clone(), e))? {
            let entry = entry.map_err(|e| Error::io(abs.clone(), e))?;
            let name = entry.file_name().to_string_lossy().to_string();
            out.push(if rel_path.is_empty() || rel_path == "." {
                name
            } else {
                format!("{}/{}", rel_path.trim_end_matches('/'), name)
            });
        }
        out.sort();
        Ok(out)
    }

    fn walk_files(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored(e.path(), &self.root))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(rel) = entry.path().strip_prefix(&self.root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        out.sort();
        Ok(out)
    }

    fn modified_at(&self, rel_path: &str) -> Result<Option<u64>> {
        let abs = self.resolve(rel_path)?;
        match std::fs::metadata(&abs) {
            Ok(meta) => Ok(meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())),
            Err(_) => Ok(None),
        }
    }

    fn file_size(&self, rel_path: &str) -> Result<Option<u64>> {
        let abs = self.resolve(rel_path)?;
        match std::fs::metadata(&abs) {
            Ok(meta) => Ok(Some(meta.len())),
            Err(_) => Ok(None),
        }
    }

    fn absolute_path(&self, rel_path: &str) -> Option<PathBuf> {
        self.resolve(rel_path).ok()
    }
}

/// 目录扫描时需要跳过的路径（索引目录、Git 目录、隐藏文件）。
fn is_ignored(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return true;
    };
    for seg in rel.components() {
        let name = seg.as_os_str().to_string_lossy();
        if name == crate::KBINDEX_DIR || name == ".git" {
            return true;
        }
        // 跳过以点开头的隐藏文件/目录（但排除根目录自身）
        if name.starts_with('.') && name != "." {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_escape() {
        let s = FsStorage::new("/tmp/vault");
        assert!(s.resolve("../outside").is_err());
        assert!(s.resolve("a/../b").is_err());
        assert!(s.resolve("a/b").is_ok());
    }

    #[test]
    fn ignores_kbindex_and_git() {
        let root = Path::new("/tmp/vault");
        assert!(is_ignored(&root.join(".kbindex"), root));
        assert!(is_ignored(&root.join(".git"), root));
        assert!(is_ignored(&root.join(".hidden"), root));
        assert!(!is_ignored(&root.join("notes.md"), root));
        assert!(!is_ignored(&root.join("sub/notes.md"), root));
    }
}

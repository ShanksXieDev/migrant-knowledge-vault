//! 知识库扫描：发现受支持的文档，产出扫描结果。

use crate::format::Format;
use crate::storage::Storage;
use serde::{Deserialize, Serialize};

/// 扫描到的一个文档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedDoc {
    /// 相对知识库根目录的路径。
    pub path: String,
    /// 文件名（含扩展名）。
    pub file_name: String,
    /// 所属目录（相对路径，根目录为 ""）。
    pub dir: String,
    /// 格式。
    pub format: Format,
    /// 文件大小（字节）。
    pub size: u64,
    /// 最后修改时间（Unix 秒）。
    pub modified_at: u64,
}

impl ScannedDoc {
    /// 文档标题：默认取文件名（不含扩展名）。
    pub fn title(&self) -> &str {
        let stem = &self.file_name;
        match stem.rfind('.') {
            Some(idx) => &stem[..idx],
            None => stem,
        }
    }
}

/// 扫描结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub docs: Vec<ScannedDoc>,
    /// 被跳过的文件数（不支持的格式等）。
    pub skipped: usize,
}

impl ScanResult {
    pub fn total_bytes(&self) -> u64 {
        self.docs.iter().map(|d| d.size).sum()
    }
}

/// 扫描知识库，返回所有受支持的文档。
pub fn scan(storage: &dyn Storage) -> crate::Result<ScanResult> {
    let mut result = ScanResult::default();

    for rel in storage.walk_files()? {
        let Some(format) = Format::from_path(std::path::Path::new(&rel)) else {
            result.skipped += 1;
            continue;
        };
        if !format.is_supported() {
            result.skipped += 1;
            continue;
        }

        let size = storage.file_size(&rel)?.unwrap_or(0);
        let modified_at = storage.modified_at(&rel)?.unwrap_or(0);
        let file_name = rel
            .rsplit('/')
            .next()
            .map(|s| s.to_string())
            .unwrap_or_else(|| rel.clone());
        let dir = match rel.rfind('/') {
            Some(idx) => rel[..idx].to_string(),
            None => String::new(),
        };

        result.docs.push(ScannedDoc {
            path: rel,
            file_name,
            dir,
            format,
            size,
            modified_at,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_strips_extension() {
        let doc = ScannedDoc {
            path: "a/b/note.md".into(),
            file_name: "note.md".into(),
            dir: "a/b".into(),
            format: Format::Markdown,
            size: 10,
            modified_at: 0,
        };
        assert_eq!(doc.title(), "note");
    }
}

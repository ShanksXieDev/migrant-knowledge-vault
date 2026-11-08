//! 跨端共享的数据传输对象（DTO）。
//!
//! 本模块**不依赖** tantivy / rusqlite / git2 等原生依赖，
//! 因此 Web（wasm32）目标也可以编译，供 `mkv-ui` 三端复用。

use serde::{Deserialize, Serialize};

/// 一条检索命中。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// 文档相对路径。
    pub path: String,
    /// 文档标题。
    pub title: String,
    /// 格式标签。
    pub format: String,
    /// 相关度得分。
    pub score: f32,
    /// 命中摘要。
    pub snippet: String,
}

/// 一条双链边（from 引用 to）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkRecord {
    pub from_path: String,
    pub to_path: String,
    /// 原始链接文本。
    pub target: String,
    /// 引用粒度。
    pub granularity: String,
}

/// 一条文档记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub path: String,
    pub title: String,
    pub format: String,
    pub size: u64,
    pub modified_at: u64,
    pub content_hash: String,
}

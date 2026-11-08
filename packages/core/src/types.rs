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
    /// 最后修改时间（Unix 秒）。
    ///
    /// F-07 多维过滤需要它做时间窗判定，因此放进命中本身：
    /// 过滤发生在**检索结果**上（而不是索引查询里），这样 Web 预构建索引
    /// 与 Native 索引的过滤语义完全一致，无需在两条链路上各实现一遍。
    ///
    /// `#[serde(default)]`：旧产物没有该字段时为 0（表示「未知时间」，
    /// 会被时间窗过滤排除——宁可少显示，也不谎报时间）。
    #[serde(default)]
    pub modified_at: u64,
    /// 文档标签（F-07 标签过滤）。
    #[serde(default)]
    pub tags: Vec<String>,
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

/// 一个可作为引用目标的文档及其选择器候选（F-06 插入引用入口）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceTarget {
    /// 目标文档相对路径（引用 token 里用的就是这个）。
    pub path: String,
    /// 目标文档标题（展示用）。
    pub title: String,
    /// 选择器候选：(1-based 序号, 标签文本, 类型)。
    pub candidates: Vec<(usize, String, String)>,
}

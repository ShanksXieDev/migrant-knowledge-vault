//! Migrant Knowledge Vault 核心库
//!
//! 分层约定（见技术方案设计 §1）：
//! - 本 crate **零 UI 依赖**，全部业务逻辑可被 Web / Desktop / Mobile 三端复用。
//! - 平台差异通过 [`storage::Storage`]、`[`index::SearchIndex`] 与
//!   [`service::KnowledgeService`] 三个 trait 抽象。
//!
//! ## 特性（features）
//!
//! | 特性 | 目标 | 启用内容 |
//! |------|------|----------|
//! | `native`（默认） | Desktop / Mobile | Tantivy 全文索引 + SQLite 元数据 + Git 版本 + jieba 分词 |
//! | `web` | Web (wasm32) | 仅纯 Rust 能力；索引采用**预构建只读**（技术方案设计 §4） |
//!
//! 纯 Rust 部分（`types` / `service` / `format` / `storage` / `parse` / `scan` /
//! `editor` / `demo`）在两种特性下均可编译。
//!
//! `tokenizer` 仅在 `native` 下提供：jieba 经 `include-flate` 引入 `zstd`，
//! 而 `zstd-sys` 需要 C 编译器，wasm32 目标不可用。Web 端采用预构建只读索引，
//! 运行期不需要分词器。
//!
//! `graph_layout`（力导向布局引擎）同样只在 `native` 下提供：它是纯 Rust 但体积不小，
//! 且 Web 端的图谱坐标已改为**构建期预计算**后随预构建索引产物发布
//! （见 [`prebuilt::PrebuiltIndex::graph_positions`]），
//! 运行期不再需要迭代引擎，也不必把它带进 wasm 产物。

// ---- 纯 Rust：跨端可用 ----
// 注意：三个编辑器已从 core 拆到 packages/plugins/ 独立 crate，
// core 只保留插件契约（plugin）与路由（editor），不链接任何编辑器实现。
pub mod backend;
#[cfg(not(target_arch = "wasm32"))]
pub mod demo;
pub mod diagram;
pub mod editor;
pub mod editor_updates;
pub mod error;
pub mod format;
pub mod locate;
pub mod ooxml;
pub mod office_locator;
pub mod parse;
pub mod plugin;
pub mod prebuilt;
pub mod query;
pub mod refs;
pub mod regex;
pub mod scan;
pub mod service;
pub mod storage;
pub mod tags;
pub mod types;
pub mod xml;

// ---- 原生能力：仅 native 目标 ----
#[cfg(feature = "native")]
pub mod git;
#[cfg(feature = "native")]
pub mod graph;
#[cfg(feature = "native")]
pub mod graph_layout;
#[cfg(feature = "native")]
pub mod index;
#[cfg(feature = "native")]
pub mod meta;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "native")]
pub mod prebuilt_export;
#[cfg(feature = "native")]
pub mod rpc;
#[cfg(feature = "native")]
pub mod tokenizer;
#[cfg(feature = "native")]
pub mod vault;
#[cfg(feature = "native")]
pub mod watcher;

pub use error::{Error, Result};
pub use format::Format;
pub use query::{QueryFilter, QueryMode, SearchQuery};
pub use service::KnowledgeService;
pub use types::{DocumentRecord, LinkRecord, SearchHit};

/// 知识库索引目录名（见技术方案设计 §3 `.kbindex/` 目录规范）。
pub const KBINDEX_DIR: &str = ".kbindex";

/// 索引 schema 版本；变更时需触发索引重建（见 `state.json`）。
pub const SCHEMA_VERSION: u32 = 1;

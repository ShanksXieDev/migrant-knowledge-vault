//! 知识库服务抽象：UI 层与核心实现之间的契约。
//!
//! UI 层（`mkv-ui`）只依赖本 trait，不直接依赖 Tantivy / SQLite / Git，
//! 因此可在 wasm32（Web）与原生（Desktop / Mobile）两端编译。
//!
//! - Desktop / Mobile：由 `native` 特性下的 [`crate::native::NativeService`] 实现。
//! - Web：采用预构建只读索引（见技术方案设计 §4），由 Web 端自行实现。

use crate::types::{DocumentRecord, LinkRecord, SearchHit};

/// 知识库查询服务。
pub trait KnowledgeService: Send + Sync {
    /// 知识库根目录路径。
    fn root(&self) -> String;

    /// 文档总数。
    fn document_count(&self) -> usize;

    /// 列出全部文档（用于侧栏浏览）。
    ///
    /// 默认返回空列表：Web 端的预构建索引可提供该能力。
    fn documents(&self) -> Vec<DocumentRecord> {
        Vec::new()
    }

    /// 全文检索。
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit>;

    /// 读取文档原文。
    fn read_document(&self, rel_path: &str) -> Option<String>;

    /// 反链（谁引用了我）。
    fn backlinks(&self, rel_path: &str) -> Vec<LinkRecord>;

    /// 出链（我引用了谁）。
    fn outgoing_links(&self, rel_path: &str) -> Vec<LinkRecord>;

    /// 当前状态描述。
    fn status(&self) -> String {
        "就绪".to_string()
    }

    /// 是否支持用系统默认应用打开原始文档（F-08）。
    ///
    /// Web 端只有内嵌索引、没有本地文件，返回 false；
    /// Desktop / Mobile 的本地知识库返回 true。
    fn supports_external_open(&self) -> bool {
        false
    }

    /// 用系统默认应用打开原始文档（Windows `start` / macOS `open` / Linux `xdg-open`）。
    ///
    /// 默认不支持：Web 只读后端应给出明确错误，而不是静默失败。
    fn open_externally(&self, _rel_path: &str) -> crate::Result<()> {
        Err(crate::Error::Config("当前后端不支持外部唤起".into()))
    }

    /// 是否支持内置编辑（GridCraft / DeckCraft / WordCraft）。
    ///
    /// Web 端为预构建只读索引，返回 false。
    fn supports_editing(&self) -> bool {
        false
    }

    /// 打开文档进行内置编辑。
    ///
    /// 默认不支持（Web 端为只读索引），返回 `None` 表示该后端不可编辑。
    fn open_editor(&self, _rel_path: &str) -> Option<crate::editor::EditSession> {
        None
    }

    /// 保存内置编辑结果。
    ///
    /// 默认不支持；返回 `Err` 表示保存失败。
    fn save_editor(&self, _session: &crate::editor::EditSession) -> crate::Result<()> {
        Err(crate::Error::Config("当前后端不支持编辑保存".into()))
    }
}

/// 空实现（Web 端未接入数据源时的占位）。
#[derive(Debug, Default, Clone, Copy)]
pub struct NullService;

impl KnowledgeService for NullService {
    fn root(&self) -> String {
        String::new()
    }

    fn document_count(&self) -> usize {
        0
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        None
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }
}

//! 全文索引：Tantivy 实现 + `SearchIndex` trait 抽象。
//!
//! 见技术方案设计 §2（决策：A = Tantivy 全文 + SQLite 元数据）。
//! schema 字段：
//! - `path`    : 文档相对路径（raw 分词，精确匹配用）
//! - `title`   : 标题（mkv_jieba 分词，权重更高）
//! - `content` : 正文（mkv_jieba 分词）
//! - `format`  : 格式（raw）

use crate::error::Result;
use crate::tokenizer::{JiebaTokenizer, TOKENIZER_NAME};
use crate::types::SearchHit;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::{Index, IndexWriter, ReloadPolicy, TantivyDocument, Term, doc};

/// 一条待索引的文档。
#[derive(Debug, Clone)]
pub struct IndexDocument {
    pub path: String,
    pub title: String,
    pub content: String,
    pub format: String,
}

/// 检索后端抽象（Web 端可换用预构建只读索引实现）。
pub trait SearchIndex: Send + Sync {
    fn upsert(&mut self, doc: &IndexDocument) -> Result<()>;
    fn delete(&mut self, path: &str) -> Result<()>;
    /// 清空全部索引文档（全量重建前调用，避免留下已删除文档的幽灵条目）。
    fn clear(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>>;
    fn doc_count(&self) -> Result<u64>;
}

/// Tantivy 索引实现。
pub struct TantivyIndex {
    index: Index,
    writer: Option<IndexWriter>,
    path_field: Field,
    title_field: Field,
    content_field: Field,
    format_field: Field,
    /// 索引目录（`.kbindex/`），用于持久化领域词典。
    kbindex_dir: std::path::PathBuf,
}

/// 领域词典文件名（位于 `.kbindex/` 下）。
pub const DOMAIN_DICT_FILE: &str = "dict.txt";

impl TantivyIndex {
    /// 在知识库索引目录下创建或打开索引。
    ///
    /// 目录结构：
    /// - `<kbindex>/search/`  Tantivy 索引段文件
    /// - `<kbindex>/dict.txt`  自定义领域词典（可选）
    ///
    /// **领域词典必须随索引一同加载**：索引与查询使用同一个分词器，
    /// 否则同一段文本在两侧被切成不同的词，检索结果会失真
    /// （tantivy 对多词查询生成短语查询，词序不一致将直接导致零命中）。
    pub fn open(kbindex: &std::path::Path) -> Result<Self> {
        let dir = kbindex.join("search");
        std::fs::create_dir_all(&dir).map_err(|e| crate::Error::io(dir.clone(), e))?;

        let mut builder = Schema::builder();
        // 注意：所有字段都必须 .set_stored()，否则检索结果取不回字段值。
        let raw = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            )
            .set_stored();
        let jieba_opts = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(TOKENIZER_NAME)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        let path_field = builder.add_text_field("path", raw.clone());
        let title_field = builder.add_text_field("title", jieba_opts.clone());
        let content_field = builder.add_text_field("content", jieba_opts);
        let format_field = builder.add_text_field("format", raw);
        let schema = builder.build();

        let index = match Index::open_in_dir(&dir) {
            Ok(idx) => idx,
            Err(_) => Index::create_in_dir(&dir, schema)?,
        };

        // 注册自定义分词器（jieba + 领域词典）
        index.tokenizers().register(TOKENIZER_NAME, JiebaTokenizer::new());

        let writer = index.writer(50_000_000)?;

        let mut this = Self {
            index,
            writer: Some(writer),
            path_field,
            title_field,
            content_field,
            format_field,
            kbindex_dir: kbindex.to_path_buf(),
        };

        // 载入已持久化的领域词典（若存在）
        this.load_persisted_dict()?;
        Ok(this)
    }

    /// 载入 `.kbindex/dict.txt` 中的领域词典。
    fn load_persisted_dict(&mut self) -> Result<usize> {
        let path = self.kbindex_dir.join(DOMAIN_DICT_FILE);
        if !path.exists() {
            return Ok(0);
        }
        let text = std::fs::read_to_string(&path).map_err(|e| crate::Error::io(path.clone(), e))?;
        let mut tokenizer = JiebaTokenizer::new();
        let added = tokenizer.load_domain_dict(&text)?;
        self.index.tokenizers().register(TOKENIZER_NAME, tokenizer);
        Ok(added)
    }

    /// 当前领域词典文本（未设置时为空串）。
    pub fn domain_dict_text(&self) -> String {
        let path = self.kbindex_dir.join(DOMAIN_DICT_FILE);
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// 领域词典文件的哈希（用于判断索引是否过期）。
    pub fn domain_dict_hash(&self) -> String {
        hash_dict(&self.domain_dict_text())
    }

    /// 用领域词典重建分词器（需重开 writer）。
    ///
    /// 词典会同步持久化到 `.kbindex/dict.txt`，保证下次打开索引时
    /// 索引与查询使用同一套分词规则。
    pub fn apply_domain_dict(&mut self, dict_text: &str) -> Result<usize> {
        let mut tokenizer = JiebaTokenizer::new();
        let added = tokenizer.load_domain_dict(dict_text)?;

        // 先持久化，再切换分词器：持久化失败时不改变当前分词行为
        let path = self.kbindex_dir.join(DOMAIN_DICT_FILE);
        std::fs::write(&path, dict_text).map_err(|e| crate::Error::io(path.clone(), e))?;

        self.index.tokenizers().register(TOKENIZER_NAME, tokenizer);
        // 词典变化需要重建索引才能生效
        self.writer = None;
        Ok(added)
    }

    fn writer(&mut self) -> Result<&mut IndexWriter> {
        if self.writer.is_none() {
            self.writer = Some(self.index.writer(50_000_000)?);
        }
        Ok(self.writer.as_mut().unwrap())
    }

    /// 删除某路径的所有文档（增量更新用）。
    fn delete_by_path_term(&mut self, path: &str) -> Result<()> {
        // 先构造 term 再取 writer，避免同时持有 &mut self 的两个借用
        let term = Term::from_field_text(self.path_field, path);
        let writer = self.writer()?;
        writer.delete_term(term);
        Ok(())
    }
}

impl SearchIndex for TantivyIndex {
    fn upsert(&mut self, doc: &IndexDocument) -> Result<()> {
        // 先删后加，保证同一 path 只有一份文档
        self.delete_by_path_term(&doc.path)?;

        let tantivy_doc = doc!(
            self.path_field => doc.path.clone(),
            self.title_field => doc.title.clone(),
            self.content_field => doc.content.clone(),
            self.format_field => doc.format.clone(),
        );
        let writer = self.writer()?;
        writer.add_document(tantivy_doc)?;
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<()> {
        self.delete_by_path_term(path)
    }

    fn clear(&mut self) -> Result<()> {
        let writer = self.writer()?;
        writer.delete_all_documents()?;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            writer.commit()?;
        }
        Ok(())
    }

    fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let query_str = query_str.trim();
        if query_str.is_empty() {
            return Ok(Vec::new());
        }

        let reader = self.index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into()?;
        let searcher = reader.searcher();

        let mut parser = QueryParser::for_index(&self.index, vec![self.title_field, self.content_field]);
        parser.set_field_boost(self.title_field, 3.0);
        // 采用 AND 语义：查询中的每个词都必须出现。
        // tantivy 默认对连续多词生成短语查询（要求词序相邻），
        // 对中文检索过严 —— 例如「模流分析」被切成「模流+分析」后，
        // 文档中两词不相邻就会零命中。
        parser.set_conjunction_by_default();
        let query = match parser.parse_query(query_str) {
            Ok(q) => q,
            Err(_) => {
                // 用户输入可能不是合法查询语法，退化为全部词项匹配
                let fallback = parser.parse_query_lenient(query_str).0;
                fallback
            }
        };

        let top = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;

        let mut hits = Vec::new();
        for (score, addr) in top {
            let tantivy_doc: TantivyDocument = searcher.doc(addr)?;
            let path = tantivy_doc
                .get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let title = tantivy_doc
                .get_first(self.title_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let format = tantivy_doc
                .get_first(self.format_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let content = tantivy_doc
                .get_first(self.content_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            hits.push(SearchHit {
                path,
                title,
                format,
                score,
                snippet: make_snippet(&content, 160),
            });
        }
        Ok(hits)
    }

    fn doc_count(&self) -> Result<u64> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        Ok(searcher.num_docs())
    }
}

/// 生成摘要片段。
fn make_snippet(content: &str, max_chars: usize) -> String {
    let flat: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let truncated: String = flat.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// 领域词典内容哈希（用于判断索引是否过期）。
pub fn hash_dict(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 供 schema 构建使用的字段名常量。
pub const FIELD_PATH: &str = "path";
pub const FIELD_TITLE: &str = "title";
pub const FIELD_CONTENT: &str = "content";
pub const FIELD_FORMAT: &str = "format";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_and_searches_chinese() {
        let dir = std::env::temp_dir().join(format!("mkv_idx_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut idx = TantivyIndex::open(&dir.join("kbindex")).unwrap();
        idx.upsert(&IndexDocument {
            path: "notes/a.md".into(),
            title: "模具分析".into(),
            content: "本文档介绍 Moldflow 模流分析的基本流程与参数设置。".into(),
            format: "Markdown".into(),
        })
        .unwrap();
        idx.commit().unwrap();

        let hits = idx.search("模流分析", 10).unwrap();
        assert!(!hits.is_empty(), "should find Chinese content");
        assert_eq!(hits[0].path, "notes/a.md");
        assert_eq!(hits[0].title, "模具分析");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_same_path() {
        let dir = std::env::temp_dir().join(format!("mkv_idx_upd_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut idx = TantivyIndex::open(&dir.join("kbindex")).unwrap();
        idx.upsert(&IndexDocument {
            path: "a.md".into(),
            title: "旧标题".into(),
            content: "旧内容".into(),
            format: "Markdown".into(),
        })
        .unwrap();
        idx.upsert(&IndexDocument {
            path: "a.md".into(),
            title: "新标题".into(),
            content: "新内容".into(),
            format: "Markdown".into(),
        })
        .unwrap();
        idx.commit().unwrap();

        assert_eq!(idx.doc_count().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}


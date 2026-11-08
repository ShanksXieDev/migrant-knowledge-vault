//! 预构建索引的**生成**端（仅 native）。
//!
//! 位于 native 侧的原因：建索引需要 jieba 分词（含领域词典），
//! 而 jieba 经 `include-flate` 引入 zstd，需要 C 编译器，无法在 wasm32 编译。
//!
//! 生成流程：
//! 1. 扫描知识库 → 抽取各文档正文与双链；
//! 2. 用**与运行时索引完全相同**的分词器（jieba + 领域词典）切词；
//! 3. 汇总倒排表 → 写入 JSON 产物。
//!
//! 第 2 步的一致性至关重要：Web 端查询时用产物自带的词表做正向最大匹配，
//! 若建索引与查询切分不一致，会出现「零命中」缺陷（v0.1 排查过的真实问题）。

use std::collections::HashMap;

use crate::prebuilt::{Posting, PrebuiltDoc, PrebuiltIndex, PREBUILT_SCHEMA_VERSION};
use crate::tokenizer::JiebaTokenizer;
use crate::types::LinkRecord;

/// 从已建立的 Vault 生成预构建索引产物。
pub fn build_index(
    docs: &[crate::types::DocumentRecord],
    texts: &HashMap<String, String>,
    links: &[LinkRecord],
    dict_text: Option<&str>,
    root: &str,
) -> crate::Result<PrebuiltIndex> {
    // 分词器：与运行时索引一致（含领域词典）
    let mut tokenizer = JiebaTokenizer::new();
    if let Some(dict) = dict_text {
        tokenizer
            .load_domain_dict(dict)
            .map_err(|e| crate::Error::Config(format!("领域词典加载失败: {e}")))?;
    }

    // 词 → (文档下标 → 词频)
    let mut postings: HashMap<String, HashMap<u32, u32>> = HashMap::new();

    for (di, doc) in docs.iter().enumerate() {
        let Some(text) = texts.get(&doc.path) else {
            continue;
        };
        for raw in tokenizer.cut(text) {
            let term = raw.trim().to_lowercase();
            if term.is_empty() {
                continue;
            }
            *postings
                .entry(term)
                .or_default()
                .entry(di as u32)
                .or_insert(0) += 1;
        }
    }

    // 汇总为倒排表（按词条排序，保证产物可复现）
    let mut posting_list: Vec<Posting> = postings
        .into_iter()
        .map(|(token, doc_tf)| {
            let mut items: Vec<(u32, u32)> = doc_tf.into_iter().collect();
            items.sort_by_key(|(d, _)| *d);
            Posting {
                token,
                docs: items.iter().map(|(d, _)| *d).collect(),
                tf: items.iter().map(|(_, tf)| *tf).collect(),
            }
        })
        .collect();
    posting_list.sort_by(|a, b| a.token.cmp(&b.token));

    let prebuilt_docs: Vec<PrebuiltDoc> = docs
        .iter()
        .map(|d| PrebuiltDoc {
            path: d.path.clone(),
            title: d.title.clone(),
            format: d.format.clone(),
            size: d.size,
            modified_at: d.modified_at,
            text: texts.get(&d.path).cloned().unwrap_or_default(),
        })
        .collect();

    Ok(PrebuiltIndex {
        schema_version: PREBUILT_SCHEMA_VERSION,
        generated_at: crate::meta::now_secs(),
        root: root.to_string(),
        docs: prebuilt_docs,
        links: links.to_vec(),
        postings: posting_list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_index_with_matching_segmentation() {
        let docs = vec![crate::types::DocumentRecord {
            path: "a.md".into(),
            title: "模流分析".into(),
            format: "Markdown".into(),
            size: 10,
            modified_at: 1,
            content_hash: String::new(),
        }];
        let mut texts = HashMap::new();
        texts.insert("a.md".to_string(), "模流分析入门 浇口位置".to_string());

        let idx = build_index(&docs, &texts, &[], None, "测试库").unwrap();
        assert_eq!(idx.doc_count(), 1);

        // 词表里应包含「模流分析」或「模流」
        let tokens: Vec<&str> = idx.postings.iter().map(|p| p.token.as_str()).collect();
        assert!(
            tokens.contains(&"模流分析") || tokens.contains(&"模流"),
            "tokens = {tokens:?}"
        );

        // 用同一份索引检索，必须能命中
        let hits = idx.search("模流分析", 5);
        assert!(!hits.is_empty(), "建索引与查询切分不一致");
        assert_eq!(hits[0].path, "a.md");
    }

    #[test]
    fn domain_dict_affects_segmentation_consistently() {
        let docs = vec![crate::types::DocumentRecord {
            path: "a.md".into(),
            title: "t".into(),
            format: "Markdown".into(),
            size: 1,
            modified_at: 1,
            content_hash: String::new(),
        }];
        let mut texts = HashMap::new();
        texts.insert("a.md".to_string(), "冷却水路布局".to_string());

        // 加载领域词典后，「冷却水路」应成为完整词条
        let idx = build_index(&docs, &texts, &[], Some("冷却水路\n"), "测试库").unwrap();
        let tokens: Vec<&str> = idx.postings.iter().map(|p| p.token.as_str()).collect();
        assert!(tokens.contains(&"冷却水路"), "tokens = {tokens:?}");

        // 查询侧同样能命中（词表驱动的正向最大匹配）
        let hits = idx.search("冷却水路", 5);
        assert!(!hits.is_empty(), "词典场景下查询应命中");
    }
}

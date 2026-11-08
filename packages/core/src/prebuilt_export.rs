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

/// 取路径中的目录部分（根目录文档返回空串）。
fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}

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

    // F-06：先汇总全库 (路径, 正文, 格式)，引用解析需要跨文档取源片段。
    let all_docs: Vec<(String, String, crate::format::Format)> = docs
        .iter()
        .filter_map(|d| {
            let text = texts.get(&d.path)?.clone();
            let format = crate::format::Format::from_path(std::path::Path::new(&d.path))
                .unwrap_or(crate::format::Format::Markdown);
            Some((d.path.clone(), text, format))
        })
        .collect();

    // F-11：抽取流程图节点清单。节点命中优先于片段命中，
    // 因此 `@流程.drawio#node-a` 能定位到具体图形节点而不是「第 N 个节点」。
    // 需要读原始字节（.drawio 的 XML / .vsdx 的 zip），因此走本地存储。
    let storage = crate::storage::FsStorage::new(root);
    let node_targets: Vec<(String, crate::format::Format)> = all_docs
        .iter()
        .map(|(p, _, f)| (p.clone(), *f))
        .collect();
    let node_map = crate::diagram::build_node_map(&storage, &node_targets);

    let prebuilt_docs: Vec<PrebuiltDoc> = docs
        .iter()
        .map(|d| {
            let text = texts.get(&d.path).cloned().unwrap_or_default();
            let format = crate::format::Format::from_path(std::path::Path::new(&d.path))
                .unwrap_or(crate::format::Format::Markdown);
            PrebuiltDoc {
                path: d.path.clone(),
                title: d.title.clone(),
                format: d.format.clone(),
                size: d.size,
                modified_at: d.modified_at,
                // 精确定位：按格式把原文切成带标签的片段，随索引一起打包。
                segments: crate::locate::segment_text(&text, format),
                // F-06：构建期把每条引用的源片段快照一起打包，Web 只读端零网络即可显示。
                refs: crate::refs::build_reference_views_with_nodes(&text, &all_docs, &node_map),
                // F-07：标签随索引一起打包，Web 端过滤面板无需额外请求。
                tags: crate::tags::extract_tags(&text, &dir_of(&d.path), format),
                // F-11：流程图节点清单随索引一起打包，Web 端节点级引用可用。
                nodes: node_map.get(&d.path).cloned().unwrap_or_default(),
                text,
            }
        })
        .collect();

    let mut index = PrebuiltIndex {
        schema_version: PREBUILT_SCHEMA_VERSION,
        generated_at: crate::meta::now_secs(),
        root: root.to_string(),
        docs: prebuilt_docs,
        links: links.to_vec(),
        postings: posting_list,
        graph_positions: Vec::new(),
    };
    // F-10：图谱坐标在构建期算好，随产物发布；Web 端运行期只查表。
    attach_graph_positions(&mut index);
    Ok(index)
}

/// 计算图谱坐标并写回索引产物（仅 native，构建期）。
///
/// 力导向布局引擎只在 native 侧参与编译，因此坐标必须**在构建期**算好：
/// 随产物一起发布的坐标让 Web（wasm32）端运行期零迭代渲染图谱。
///
/// 坐标按文档路径索引，与节点下标无关：
/// 这样即使产物里的文档集合发生变化（远端热替换），
/// 也只是个别节点缺坐标、单独退回环形布局，不会整体错位。
fn attach_graph_positions(index: &mut PrebuiltIndex) {
    let nodes: Vec<(String, String)> = index
        .docs
        .iter()
        .map(|d| (d.path.clone(), d.title.clone()))
        .collect();
    let links: Vec<(String, String)> = index
        .links
        .iter()
        .filter(|l| !l.to_path.is_empty())
        .map(|l| (l.from_path.clone(), l.to_path.clone()))
        .collect();
    let snapshot = crate::locate::build_graph_snapshot(&nodes, &links);
    index.graph_positions = snapshot
        .nodes
        .iter()
        .map(|n| crate::locate::GraphPosition {
            path: n.path.clone(),
            x: n.x,
            y: n.y,
        })
        .collect();
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

    #[test]
    fn exports_position_segments_per_format() {
        let docs = vec![crate::types::DocumentRecord {
            path: "deck.pptx".into(),
            title: "工艺方案".into(),
            format: "PPT".into(),
            size: 10,
            modified_at: 1,
            content_hash: String::new(),
        }];
        let text = "--- ppt/slides/slide1.xml ---\n第一页浇口设计\n--- ppt/slides/slide2.xml ---\n第二页冷却\n";
        let mut texts = HashMap::new();
        texts.insert("deck.pptx".to_string(), text.to_string());

        let idx = build_index(&docs, &texts, &[], None, "测试库").unwrap();
        let segs = &idx.docs[0].segments;
        assert_eq!(segs.len(), 2, "PPT 应按页切分，实际 {segs:?}");
        assert!(segs[0].label.contains("1") && segs[1].label.contains("2"));
        let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, text, "导出片段拼接必须无损");
    }

    #[test]
    fn exports_precomputed_graph_positions() {
        // F-10：图谱坐标在构建期算好并随产物发布，
        // Web（wasm32）端运行期只查表、不再跑 240 轮迭代。
        let docs = vec![
            crate::types::DocumentRecord {
                path: "a.md".into(),
                title: "甲".into(),
                format: "Markdown".into(),
                size: 10,
                modified_at: 1,
                content_hash: String::new(),
            },
            crate::types::DocumentRecord {
                path: "b.md".into(),
                title: "乙".into(),
                format: "Markdown".into(),
                size: 10,
                modified_at: 1,
                content_hash: String::new(),
            },
        ];
        let mut texts = HashMap::new();
        texts.insert("a.md".to_string(), "模流分析入门".to_string());
        texts.insert("b.md".to_string(), "工艺参数基线".to_string());
        let links = vec![crate::types::LinkRecord {
            from_path: "a.md".into(),
            to_path: "b.md".into(),
            target: "基线".into(),
            granularity: "块级".into(),
        }];

        let idx = build_index(&docs, &texts, &links, None, "测试库").unwrap();
        assert_eq!(
            idx.graph_positions.len(),
            2,
            "每个文档节点都应带一条预计算坐标，实际 {:?}",
            idx.graph_positions
        );
        for p in &idx.graph_positions {
            assert!(p.x.is_finite() && p.y.is_finite(), "{} 坐标非法", p.path);
            assert!((0.0..=crate::locate::GRAPH_CANVAS_WIDTH).contains(&p.x));
            assert!((0.0..=crate::locate::GRAPH_CANVAS_HEIGHT).contains(&p.y));
        }

        // 产物序列化 → 重新解析后坐标仍在（远端热替换拿到同一份产物也一致）。
        let back = crate::prebuilt::PrebuiltIndex::from_json(&idx.to_json().unwrap()).unwrap();
        assert_eq!(back.graph_positions, idx.graph_positions);
        let g = back.graph_snapshot();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }
}

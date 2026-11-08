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
    build_index_with_snapshots(docs, texts, links, dict_text, root, &HashMap::new())
}

/// 从已建立的 Vault 生成预构建索引产物，并带上**引用源片段快照**。
///
/// # 快照的作用
///
/// F-06 进阶②要在引用卡片上显示「源片段相对上次看到的内容变了什么」。
/// Native 端有持久化的快照基线（`meta` 里的引用快照表），
/// 但 Web 端只有**构建期产物**，运行时无从得知「上次是什么样」。
///
/// 因此构建端接受一份「上次看到的源片段」快照（键见
/// [`crate::refs::snapshot_key`]），据此算出差异并写进产物的 `refs[].diff`。
/// Web 端随后**零运行时开销**即可显示差异高亮。
///
/// 快照只参与构建、不进产物，因此不增加产物体积。
pub fn build_index_with_snapshots(
    docs: &[crate::types::DocumentRecord],
    texts: &HashMap<String, String>,
    links: &[LinkRecord],
    dict_text: Option<&str>,
    root: &str,
    snapshots: &HashMap<String, String>,
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

    // F-06 进阶②：采集**当前**的引用源片段全文，作为下一次构建的基线。
    //
    // 必须在算 diff **之前**采集，但**不能**用它当本次的基线——
    // 那等于「拿现在和现在比」，永远没有差异。
    // 本次的基线由调用方传入（上一份产物的 `ref_snapshots`）。
    let mut current_snapshots: HashMap<String, String> = HashMap::new();
    for (_path, text, _format) in &all_docs {
        for (target, selector, current) in
            crate::refs::resolve_reference_sources(text, &all_docs, &node_map)
        {
            current_snapshots.insert(crate::refs::snapshot_key(&target, &selector), current);
        }
    }

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
                // 带快照版本：源片段相对「上次看到的内容」有变化时，差异摘要一并打包，
                // Web 端据此显示「源已变更」的高亮块（F-06 进阶②）。
                refs: crate::refs::build_reference_views_with_snapshots(
                    &text,
                    &all_docs,
                    &node_map,
                    snapshots,
                ),
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
        // 本次采集到的当前基线随产物一起发布，供下一次构建做差异比对。
        ref_snapshots: current_snapshots,
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

    /// F-06 进阶②：产物带出**当前**的引用源片段全文基线。
    #[test]
    fn exports_full_text_reference_snapshot_baseline() {
        let docs = vec![
            crate::types::DocumentRecord {
                path: "台账.xlsx".into(),
                title: "台账".into(),
                format: "Excel".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
            crate::types::DocumentRecord {
                path: "note.md".into(),
                title: "笔记".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
        ];
        let mut texts = HashMap::new();
        texts.insert(
            "台账.xlsx".to_string(),
            "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n".to_string(),
        );
        texts.insert(
            "note.md".to_string(),
            "引用：@台账.xlsx#第2行\n".to_string(),
        );

        let idx = build_index(&docs, &texts, &[], None, "测试库").unwrap();
        let key = crate::refs::snapshot_key("台账.xlsx", "第2行");
        let baseline = idx.ref_snapshots.get(&key).map(String::as_str).unwrap_or("");
        assert!(
            baseline.contains("台账: PN-1024 | MDL-08"),
            "基线必须是**完整**源片段文本，而不是截断版 snippet，实际 {baseline:?}"
        );
        // 片段原文带换行，snippet 则是 trim 过的展示文本——基线必须用前者。
        assert!(
            baseline.ends_with('\n'),
            "基线应保留片段原文（含换行），实际 {baseline:?}"
        );
        assert!(
            idx.docs[1].refs[0].snippet != baseline,
            "基线不应等同于展示用的 snippet"
        );
        // 产物序列化 → 解析后基线仍在（下一轮构建据此算差异）。
        let back = crate::prebuilt::PrebuiltIndex::from_json(&idx.to_json().unwrap()).unwrap();
        assert_eq!(back.ref_snapshots, idx.ref_snapshots);
    }

    /// F-06 进阶②：**长段落不得产生假差异**。
    ///
    /// 这是「基线必须存全文」的直接理由：若基线取的是 120 字符的
    /// `snippet`，长段落每次构建都会被判成「截断处之后全变了」。
    #[test]
    fn long_paragraph_against_full_text_baseline_produces_no_false_diff() {
        let long = format!("首句。{}尾句。", "中间内容".repeat(40));
        assert!(long.chars().count() > 120, "测试样本必须超过 120 字符截断线");
        let docs = vec![
            crate::types::DocumentRecord {
                path: "长文.md".into(),
                title: "长文".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
            crate::types::DocumentRecord {
                path: "note.md".into(),
                title: "笔记".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
        ];
        let mut texts = HashMap::new();
        texts.insert("长文.md".to_string(), long.clone());
        texts.insert("note.md".to_string(), "引用：@长文.md#1\n".to_string());

        // 第一轮：建立基线。
        let first = build_index(&docs, &texts, &[], None, "测试库").unwrap();
        // 第二轮：用第一轮的基线比对，内容没变 → 不应有差异。
        let second =
            build_index_with_snapshots(&docs, &texts, &[], None, "测试库", &first.ref_snapshots)
                .unwrap();
        let refs = &second.docs[1].refs;
        assert_eq!(refs.len(), 1);
        assert!(
            refs[0].diff.is_none(),
            "内容未变时不得报差异（长段落也不该因截断而误报）：{:?}",
            refs[0].diff
        );
    }

    /// F-06 进阶②：源片段真的改了 → 产出差异，且新增 / 删除都能看出来。
    #[test]
    fn changed_source_produces_diff_against_baseline() {
        let docs = vec![
            crate::types::DocumentRecord {
                path: "台账.xlsx".into(),
                title: "台账".into(),
                format: "Excel".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
            crate::types::DocumentRecord {
                path: "note.md".into(),
                title: "笔记".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 1,
                content_hash: String::new(),
            },
        ];
        let mut texts = HashMap::new();
        texts.insert(
            "台账.xlsx".to_string(),
            "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n".to_string(),
        );
        texts.insert(
            "note.md".to_string(),
            "引用：@台账.xlsx#第2行\n".to_string(),
        );

        let first = build_index(&docs, &texts, &[], None, "测试库").unwrap();
        assert!(first.ref_snapshots.contains_key(&crate::refs::snapshot_key("台账.xlsx", "第2行")));

        // 源文件被改：模具编号从 MDL-08 变成 MDL-09。
        let mut changed = texts.clone();
        changed.insert(
            "台账.xlsx".to_string(),
            "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-09\n".to_string(),
        );
        let second =
            build_index_with_snapshots(&docs, &changed, &[], None, "测试库", &first.ref_snapshots)
                .unwrap();
        let diff = second.docs[1].refs[0]
            .diff
            .as_ref()
            .expect("源片段变更应产出差异");
        assert!(diff.has_changes());
        assert!(
            diff.lines
                .iter()
                .any(|l| l.kind == crate::refs::DiffLineKind::Added),
            "差异里应能看到新增行：{:?}",
            diff.lines
        );
        assert!(
            diff.lines
                .iter()
                .any(|l| l.kind == crate::refs::DiffLineKind::Removed),
            "差异里应能看到删除行：{:?}",
            diff.lines
        );
    }
}

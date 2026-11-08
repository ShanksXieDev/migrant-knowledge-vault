//! 预构建只读索引（Web 端检索数据源）。
//!
//! # 为什么需要它
//!
//! Web 端（wasm32）无法运行 Tantivy（依赖 tantivy → zstd → C 编译器），
//! 也无法在浏览器里实时建索引（性能不可接受）。
//! 因此按技术方案设计 §4 / 决策②，采用**预构建只读索引**：
//! 构建期由 native 端生成索引产物并随站打包，运行时只读加载。
//!
//! # 关键设计：查询与索引共用同一套分词
//!
//! 上一轮排查过一个隐蔽缺陷：若**建索引**与**查查询**用了不同的分词器，
//! 会出现「进程内正常、跨进程零命中」。本模块因此：
//!
//! - 建索引侧（native）用 jieba + 领域词典切词，产物里保存**倒排表**；
//! - 查询侧（Web）用**正向最大匹配**在产物自带的词表上切词。
//!
//! 词表来自索引产物本身（只收录语料中真实出现过的词），
//! 因此两侧切分结果一致，且 Web 端**不需要**引入 jieba（省掉 zstd 依赖）。
//!
//! # 产物格式
//!
//! 产物是一个 JSON 文件，结构见 [`PrebuiltIndex`]。

use serde::{Deserialize, Serialize};

use crate::types::{LinkRecord, SearchHit};

/// 预构建索引 schema 版本。
pub const PREBUILT_SCHEMA_VERSION: u32 = 1;

/// 一篇文档在预构建索引中的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrebuiltDoc {
    /// 相对路径。
    pub path: String,
    /// 标题。
    pub title: String,
    /// 格式标签。
    pub format: String,
    /// 文件大小（字节）。
    pub size: u64,
    /// 最后修改时间（Unix 秒）。
    pub modified_at: u64,
    /// 抽取出的纯文本（用于预览与摘要）。
    pub text: String,
    /// 带位置标签的片段（PRD §7.1 精确定位）。
    ///
    /// `#[serde(default)]`：旧产物没有该字段时解析为空数组，
    /// 运行期由 [`PrebuiltIndex::segments`] 用同一套切分规则补算。
    #[serde(default)]
    pub segments: Vec<crate::locate::TextSegment>,
    /// 跨格式引用视图（F-06）。
    ///
    /// 同样是可选字段：旧产物没有时由 [`PrebuiltIndex::references`] 运行期补算，
    /// schema 版本保持不变（v1）。
    #[serde(default)]
    pub refs: Vec<crate::refs::ReferenceView>,
    /// 文档标签（F-07 标签过滤）。
    ///
    /// `#[serde(default)]`：旧产物没有该字段时解析为空数组，
    /// 标签过滤面板因此不显示候选（而不是显示错误的结果）。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 流程图节点清单（F-11：Visio / Drawio 节点级引用）。
    ///
    /// 只有流程图格式才有内容，其余格式为空数组。
    ///
    /// `#[serde(default)]`：旧产物没有该字段时解析为空数组，
    /// 节点级引用退回到「页 / 段」级匹配，语义不丢。
    #[serde(default)]
    pub nodes: Vec<crate::diagram::DiagramNode>,
}

/// 一个词条及其倒排表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Posting {
    /// 词条（已按分词切分）。
    pub token: String,
    /// 命中的文档下标（对应 [`PrebuiltIndex::docs`] 的顺序）。
    pub docs: Vec<u32>,
    /// 各文档中该词的词频。
    pub tf: Vec<u32>,
}

impl Posting {
    /// 文档频次（命中文档数）。
    pub fn df(&self) -> usize {
        self.docs.len()
    }
}

/// 预构建索引产物。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrebuiltIndex {
    /// schema 版本。
    pub schema_version: u32,
    /// 生成时间（Unix 秒）。
    pub generated_at: u64,
    /// 知识库根目录名（仅展示用）。
    pub root: String,
    /// 文档列表。
    pub docs: Vec<PrebuiltDoc>,
    /// 双链边（from → to）。
    pub links: Vec<LinkRecord>,
    /// 倒排表。
    pub postings: Vec<Posting>,
    /// 图谱节点坐标（F-10，**构建期预计算**）。
    ///
    /// 由 native 侧的力导向布局在导出索引时算好，随产物一起发布；
    /// Web（wasm32）端运行期只查表，不再执行 240 轮浮点迭代，
    /// 也不必把布局引擎带进 wasm 产物。
    ///
    /// `#[serde(default)]`：schema v1 旧产物没有该字段时解析为空数组，
    /// 运行期退回确定性环形布局（图谱面板照常渲染，不白屏）。
    #[serde(default)]
    pub graph_positions: Vec<crate::locate::GraphPosition>,
}

impl PrebuiltIndex {
    /// 创建一个空索引。
    pub fn empty() -> Self {
        Self {
            schema_version: PREBUILT_SCHEMA_VERSION,
            generated_at: 0,
            root: String::new(),
            docs: Vec::new(),
            links: Vec::new(),
            postings: Vec::new(),
            graph_positions: Vec::new(),
        }
    }

    /// 从 JSON 文本解析。
    pub fn from_json(text: &str) -> crate::Result<Self> {
        serde_json::from_str(text).map_err(|e| {
            crate::Error::Config(format!("预构建索引解析失败: {e}"))
        })
    }

    /// 序列化为 JSON 文本。
    pub fn to_json(&self) -> crate::Result<String> {
        serde_json::to_string(self).map_err(|e| {
            crate::Error::Config(format!("预构建索引序列化失败: {e}"))
        })
    }

    /// 文档总数。
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }

    /// 文档列表（供 UI 侧栏浏览）。
    pub fn documents(&self) -> Vec<crate::types::DocumentRecord> {
        self.docs
            .iter()
            .map(|d| crate::types::DocumentRecord {
                path: d.path.clone(),
                title: d.title.clone(),
                format: d.format.clone(),
                size: d.size,
                modified_at: d.modified_at,
                content_hash: String::new(),
            })
            .collect()
    }

    /// 构建查询词表：词 → 倒排下标。
    fn vocabulary(&self) -> std::collections::HashMap<&str, usize> {
        self.postings
            .iter()
            .enumerate()
            .map(|(i, p)| (p.token.as_str(), i))
            .collect()
    }

    /// 查询切词：正向最大匹配。
    ///
    /// 只在索引自带的词表上匹配，保证与建索引侧切分一致。
    /// 词表未收录的连续中文按单字切分，连续拉丁字母/数字按整词切分。
    fn segment(&self, query: &str, vocab: &std::collections::HashMap<&str, usize>) -> Vec<String> {
        let chars: Vec<char> = query.chars().collect();
        let max_len = self
            .postings
            .iter()
            .map(|p| p.token.chars().count())
            .max()
            .unwrap_or(1)
            .clamp(1, 16);

        let mut out = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            // 优先尝试最长匹配
            let mut matched = false;
            let upper = max_len.min(chars.len() - i);
            for len in (1..=upper).rev() {
                let candidate: String = chars[i..i + len].iter().collect();
                if vocab.contains_key(candidate.as_str()) {
                    out.push(candidate);
                    i += len;
                    matched = true;
                    break;
                }
            }
            if matched {
                continue;
            }

            // 拉丁字母 / 数字：整段作为一个词
            if chars[i].is_ascii_alphanumeric() {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let lower = word.to_lowercase();
                // 词表里没有就退回原样（可能命中未收录的 ASCII 词）
                out.push(if vocab.contains_key(lower.as_str()) {
                    lower
                } else {
                    word
                });
                continue;
            }

            // 单字兜底
            out.push(chars[i].to_string());
            i += 1;
        }
        out
    }

    /// 检索。
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let query = query.trim();
        if query.is_empty() || self.docs.is_empty() {
            return Vec::new();
        }
        let vocab = self.vocabulary();
        let terms = self.segment(query, &vocab);
        if terms.is_empty() {
            return Vec::new();
        }

        let n = self.docs.len() as f64;
        // 文档 → (得分, 命中词数)
        let mut scores: std::collections::HashMap<u32, (f64, usize)> =
            std::collections::HashMap::new();

        for term in &terms {
            let Some(&pi) = vocab.get(term.as_str()) else {
                continue;
            };
            let posting = &self.postings[pi];
            let df = posting.df() as f64;
            // 平滑的逆文档频率
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for (di, &doc) in posting.docs.iter().enumerate() {
                let tf = posting.tf.get(di).copied().unwrap_or(1) as f64;
                let entry = scores.entry(doc).or_insert((0.0, 0));
                entry.0 += idf * (1.0 + tf.ln());
                entry.1 += 1;
            }
        }

        let term_count = terms.len() as f64;
        let mut ranked: Vec<(u32, f64, usize)> = scores
            .into_iter()
            .map(|(doc, (score, hits))| {
                // 命中词覆盖越多越相关（近似 AND 语义）
                let coverage = hits as f64 / term_count;
                (doc, score * (0.4 + 0.6 * coverage), hits)
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        ranked
            .into_iter()
            .take(limit)
            .filter_map(|(di, score, _)| {
                let doc = self.docs.get(di as usize)?;
                Some(SearchHit {
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    format: doc.format.clone(),
                    score: score as f32,
                    snippet: make_snippet(&doc.text, query),
                    modified_at: doc.modified_at,
                    tags: doc.tags.clone(),
                })
            })
            .collect()
    }

    /// 按查询对象检索（F-12 正则模式 + F-07 多维过滤）。
    ///
    /// 与 Native 端语义一致：
    /// - 普通模式走倒排表；
    /// - 正则模式在**原文**上匹配（预构建索引本就存了 `text`，可直接扫）；
    /// - 过滤在结果上做，三端判定函数共用。
    pub fn search_query(&self, query: &crate::query::SearchQuery) -> Vec<SearchHit> {
        if query.is_blank() {
            return Vec::new();
        }
        let hits = match query.mode {
            crate::query::QueryMode::Terms => self.search(&query.text, query.limit),
            crate::query::QueryMode::Regex => {
                let Ok(re) = crate::regex::Regex::new(&query.text) else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for doc in &self.docs {
                    let Some(m) = re.find_at(&doc.text, 0) else {
                        continue;
                    };
                    out.push(SearchHit {
                        path: doc.path.clone(),
                        title: doc.title.clone(),
                        format: doc.format.clone(),
                        // 正则模式没有词频统计，用命中数当分数（越高越靠前）
                        score: re.find_all(&doc.text, 0).len() as f32,
                        snippet: regex_snippet(&doc.text, m.start, m.end),
                        modified_at: doc.modified_at,
                        tags: doc.tags.clone(),
                    });
                    if out.len() >= query.limit {
                        break;
                    }
                }
                out
            }
        };

        if query.filter.is_empty() {
            return hits;
        }
        hits.into_iter()
            .filter(|h| query.filter.matches(h, h.modified_at, |p| self.tags_of(p)))
            .collect()
    }

    /// 某文档的标签（F-07）。
    pub fn tags_of(&self, rel_path: &str) -> Vec<String> {
        self.docs
            .iter()
            .find(|d| d.path == rel_path)
            .map(|d| d.tags.clone())
            .unwrap_or_default()
    }

    /// 全部标签及文档数（F-07 过滤面板候选项）。
    pub fn all_tags(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for doc in &self.docs {
            for tag in &doc.tags {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// 读取文档正文。
    pub fn read_document(&self, rel_path: &str) -> Option<String> {
        self.docs
            .iter()
            .find(|d| d.path == rel_path)
            .map(|d| d.text.clone())
    }

    /// 带位置标签的片段（兼容不带 `segments` 的旧产物）。
    pub fn segments(&self, rel_path: &str) -> Vec<crate::locate::TextSegment> {
        let Some(doc) = self.docs.iter().find(|d| d.path == rel_path) else {
            return Vec::new();
        };
        if !doc.segments.is_empty() {
            return doc.segments.clone();
        }
        let format = crate::format::Format::from_path(std::path::Path::new(rel_path))
            .unwrap_or(crate::format::Format::Markdown);
        crate::locate::segment_text(&doc.text, format)
    }

    /// 跨格式引用视图（F-06，兼容不带 `refs` 的旧产物）。
    pub fn references(&self, rel_path: &str) -> Vec<crate::refs::ReferenceView> {
        let Some(doc) = self.docs.iter().find(|d| d.path == rel_path) else {
            return Vec::new();
        };
        // 没有引用语法的文档 refs 为空是正常的，不必每次都扫全库。
        if doc.refs.is_empty() && crate::refs::parse_references(&doc.text).is_empty() {
            return Vec::new();
        }
        if !doc.refs.is_empty() {
            return doc.refs.clone();
        }
        let all: Vec<(String, String, crate::format::Format)> = self
            .docs
            .iter()
            .map(|d| {
                let format = crate::format::Format::from_path(std::path::Path::new(&d.path))
                    .unwrap_or(crate::format::Format::Markdown);
                (d.path.clone(), d.text.clone(), format)
            })
            .collect();
        // F-11：节点清单随产物打包，旧产物没有该字段时退回到页 / 段级匹配。
        let node_map = self.node_map();
        crate::refs::build_reference_views_with_nodes(&doc.text, &all, &node_map)
    }

    /// 全部文档的流程图节点清单（F-11）。
    pub fn node_map(&self) -> std::collections::HashMap<String, Vec<crate::diagram::DiagramNode>> {
        let mut map = std::collections::HashMap::new();
        for d in &self.docs {
            if !d.nodes.is_empty() {
                map.insert(d.path.clone(), d.nodes.clone());
            }
        }
        map
    }

    /// 检索命中的精确位置（PRD §7.1）。
    pub fn locate(&self, rel_path: &str, query: &str) -> Option<crate::locate::HitLocation> {
        crate::locate::locate_in_segments(&self.segments(rel_path), query)
    }

    /// 双链图谱快照（F-10）。
    ///
    /// 坐标优先用**构建期预计算**的结果（[`Self::graph_positions`]）；
    /// 旧产物（schema v1）没有坐标时退回确定性环形布局。
    /// 因此 Web（wasm32）端运行期不做任何浮点布局迭代。
    pub fn graph_snapshot(&self) -> crate::locate::GraphSnapshot {
        let nodes: Vec<(String, String)> = self
            .docs
            .iter()
            .map(|d| (d.path.clone(), d.title.clone()))
            .collect();
        let links: Vec<(String, String)> = self
            .links
            .iter()
            .filter(|l| !l.to_path.is_empty())
            .map(|l| (l.from_path.clone(), l.to_path.clone()))
            .collect();
        crate::locate::build_graph_snapshot_with_positions(&nodes, &links, &self.graph_positions)
    }

    /// 反链。
    pub fn backlinks(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.links
            .iter()
            .filter(|l| l.to_path == rel_path)
            .cloned()
            .collect()
    }

    /// 出链。
    pub fn outgoing_links(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.links
            .iter()
            .filter(|l| l.from_path == rel_path)
            .cloned()
            .collect()
    }
}

/// 生成正则命中处的上下文摘要。
///
/// 与 Native 端 `vault::regex_snippet` 同语义：命中前后各取一定字符数，
/// 换行压成空格，保证 UI 单行显示不串行。
fn regex_snippet(text: &str, start: usize, end: usize) -> String {
    crate::query::regex_snippet(text, start, end)
}

/// 生成命中摘要：截取查询词附近的上下文。
fn make_snippet(text: &str, query: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let max = 160usize;

    // 找到第一个命中位置（忽略大小写）
    let lower_text: String = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let pos = lower_text.find(&lower_query).unwrap_or(0);
    let start = lower_text[..pos].chars().count().saturating_sub(40);

    let slice: String = chars
        .iter()
        .skip(start)
        .take(max)
        .collect::<String>()
        .trim()
        .to_string();
    if start > 0 {
        format!("…{slice}")
    } else {
        slice
    }
}

/// 预构建索引服务：实现 [`crate::service::KnowledgeService`]。
///
/// Web 端把它注入 UI，即可获得检索、预览与双链能力（只读）。
pub struct PrebuiltService {
    index: std::sync::Arc<PrebuiltIndex>,
}

impl PrebuiltService {
    /// 用索引产物创建服务。
    pub fn new(index: PrebuiltIndex) -> Self {
        Self {
            index: std::sync::Arc::new(index),
        }
    }

    /// 索引产物引用。
    pub fn index(&self) -> &PrebuiltIndex {
        &self.index
    }
}

impl crate::service::KnowledgeService for PrebuiltService {
    fn root(&self) -> String {
        self.index.root.clone()
    }

    fn document_count(&self) -> usize {
        self.index.doc_count()
    }

    fn documents(&self) -> Vec<crate::types::DocumentRecord> {
        self.index.documents()
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        self.index.search(query, limit)
    }

    fn search_query(&self, query: &crate::query::SearchQuery) -> Vec<SearchHit> {
        self.index.search_query(query)
    }

    fn validate_query(&self, query: &crate::query::SearchQuery) -> Option<String> {
        if query.mode != crate::query::QueryMode::Regex {
            return None;
        }
        crate::regex::Regex::new(&query.text).err()
    }

    fn tags_of(&self, rel_path: &str) -> Vec<String> {
        self.index.tags_of(rel_path)
    }

    fn all_tags(&self) -> Vec<(String, usize)> {
        self.index.all_tags()
    }

    fn read_document(&self, rel_path: &str) -> Option<String> {
        self.index.read_document(rel_path)
    }

    fn backlinks(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.index.backlinks(rel_path)
    }

    fn outgoing_links(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.index.outgoing_links(rel_path)
    }

    fn status(&self) -> String {
        format!(
            "Web 端 · 预构建只读索引 · {} 个文档",
            self.index.doc_count()
        )
    }

    fn locate(&self, rel_path: &str, query: &str) -> Option<crate::locate::HitLocation> {
        self.index.locate(rel_path, query)
    }

    fn graph_snapshot(&self) -> crate::locate::GraphSnapshot {
        self.index.graph_snapshot()
    }

    fn references(&self, rel_path: &str) -> Vec<crate::refs::ReferenceView> {
        self.index.references(rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_index_without_refs_still_parses() {
        let idx = sample_index();
        let mut json: serde_json::Value =
            serde_json::from_str(&idx.to_json().unwrap()).unwrap();
        for d in json["docs"].as_array_mut().unwrap() {
            d.as_object_mut().unwrap().remove("refs");
        }
        let legacy = PrebuiltIndex::from_json(&json.to_string()).unwrap();
        assert!(legacy.docs.iter().all(|d| d.refs.is_empty()));
        assert!(legacy.references("a.md").is_empty());
    }

    #[test]
    fn references_roundtrip_through_json_and_recompute() {
        let mut idx = sample_index();
        idx.docs[1].text = "见 @a.md#第1段 的内容".into();
        let all: Vec<(String, String, crate::format::Format)> = idx
            .docs
            .iter()
            .map(|d| (d.path.clone(), d.text.clone(), crate::format::Format::Markdown))
            .collect();
        idx.docs[1].refs = crate::refs::build_reference_views(&idx.docs[1].text, &all);
        assert_eq!(idx.docs[1].refs.len(), 1);
        assert!(idx.docs[1].refs[0].resolved, "got {:#?}", idx.docs[1].refs);

        let restored = PrebuiltIndex::from_json(&idx.to_json().unwrap()).unwrap();
        assert_eq!(restored.docs[1].refs, idx.docs[1].refs);

        // 旧产物缺 refs 时，运行期用同一套解析规则补算
        let mut legacy = restored.clone();
        for d in &mut legacy.docs {
            d.refs.clear();
        }
        let recomputed = legacy.references("b.md");
        assert_eq!(recomputed.len(), 1, "got {recomputed:#?}");
        assert!(recomputed[0].resolved);
    }

    fn sample_index() -> PrebuiltIndex {
        PrebuiltIndex {
            schema_version: PREBUILT_SCHEMA_VERSION,
            generated_at: 1000,
            root: "示例库".into(),
            docs: vec![
                PrebuiltDoc {
                    path: "a.md".into(),
                    title: "模流分析入门".into(),
                    format: "Markdown".into(),
                    size: 100,
                    modified_at: 1,
                    text: "模流分析是注塑成型前的重要仿真手段，用于评估浇口位置与熔接线位置。".into(),
                    segments: Vec::new(),
                    refs: Vec::new(),
                    tags: Vec::new(),
                    nodes: Vec::new(),
                },
                PrebuiltDoc {
                    path: "b.md".into(),
                    title: "工艺参数基线".into(),
                    format: "Markdown".into(),
                    size: 80,
                    modified_at: 2,
                    text: "注射速度与熔体温度的基线取值，用于工艺评审。".into(),
                    segments: Vec::new(),
                    refs: Vec::new(),
                    tags: Vec::new(),
                    nodes: Vec::new(),
                },
            ],
            links: vec![LinkRecord {
                from_path: "b.md".into(),
                to_path: "a.md".into(),
                target: "模流分析".into(),
                granularity: "块级".into(),
            }],
            postings: vec![
                Posting {
                    token: "模流".into(),
                    docs: vec![0],
                    tf: vec![2],
                },
                Posting {
                    token: "模流分析".into(),
                    docs: vec![0],
                    tf: vec![1],
                },
                Posting {
                    token: "分析".into(),
                    docs: vec![0],
                    tf: vec![1],
                },
                Posting {
                    token: "浇口".into(),
                    docs: vec![0],
                    tf: vec![1],
                },
                Posting {
                    token: "工艺".into(),
                    docs: vec![1],
                    tf: vec![1],
                },
                Posting {
                    token: "参数".into(),
                    docs: vec![1],
                    tf: vec![1],
                },
            ],
            graph_positions: Vec::new(),
        }
    }

    #[test]
    fn json_roundtrip() {
        let idx = sample_index();
        let text = idx.to_json().unwrap();
        let back = PrebuiltIndex::from_json(&text).unwrap();
        assert_eq!(back.doc_count(), 2);
        assert_eq!(back.postings.len(), 6);
        assert_eq!(back.links.len(), 1);
    }

    #[test]
    fn search_finds_matching_doc() {
        let idx = sample_index();
        let hits = idx.search("模流分析", 10);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "a.md");
    }

    #[test]
    fn search_matches_query_and_index_segmentation() {
        let idx = sample_index();
        // 查询切分必须命中索引里的词条，否则会出现「零命中」缺陷
        let hits = idx.search("浇口位置", 10);
        assert!(!hits.is_empty(), "查询与索引分词不一致会导致零命中");
        assert_eq!(hits[0].path, "a.md");
    }

    #[test]
    fn search_with_unrelated_query_returns_empty() {
        let idx = sample_index();
        assert!(idx.search("完全无关的词", 10).is_empty());
    }

    #[test]
    fn backlinks_and_outgoing_links() {
        let idx = sample_index();
        assert_eq!(idx.backlinks("a.md").len(), 1);
        assert_eq!(idx.backlinks("a.md")[0].from_path, "b.md");
        assert_eq!(idx.outgoing_links("b.md").len(), 1);
        assert!(idx.outgoing_links("a.md").is_empty());
    }

    #[test]
    fn snippet_contains_query_context() {
        let idx = sample_index();
        let hits = idx.search("浇口", 5);
        assert!(hits[0].snippet.contains("浇口"));
    }

    #[test]
    fn segments_roundtrip_through_json() {
        let mut idx = sample_index();
        idx.docs[0].segments = crate::locate::segment_text(
            &idx.docs[0].text,
            crate::format::Format::Markdown,
        );
        let json = idx.to_json().unwrap();
        assert!(json.contains("\"segments\""), "片段应随索引产物打包");
        let back = PrebuiltIndex::from_json(&json).unwrap();
        assert_eq!(back.docs[0].segments.len(), idx.docs[0].segments.len());
        let joined: String = back.docs[0]
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(joined, back.docs[0].text, "片段拼接必须无损");
    }

    #[test]
    fn legacy_index_without_segments_still_locates() {
        // v1 旧产物没有 segments 字段：serde default 为空 → 运行期用同一套规则补算
        let idx = sample_index();
        let mut value: serde_json::Value =
            serde_json::from_str(&idx.to_json().unwrap()).unwrap();
        for d in value["docs"].as_array_mut().unwrap() {
            d.as_object_mut().unwrap().remove("segments");
        }
        let legacy = PrebuiltIndex::from_json(&value.to_string()).unwrap();
        assert!(legacy.docs[0].segments.is_empty());
        let segs = legacy.segments("a.md");
        assert!(!segs.is_empty(), "旧产物应能在运行期补算片段");
        let hit = legacy.locate("a.md", "浇口").expect("旧产物也应能定位");
        assert!(hit.snippet.contains("浇口"));
    }

    #[test]
    fn graph_snapshot_comes_from_links() {
        let idx = sample_index();
        let g = idx.graph_snapshot();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1, "b.md → a.md 一条边");
        assert!(g.nodes.iter().any(|n| n.path == "a.md"));
    }

    #[test]
    fn legacy_index_without_graph_positions_still_renders_graph() {
        // schema v1 旧产物没有 graph_positions 字段（远端热替换也可能拿到旧产物）：
        // serde default 为空数组 → 运行期退回环形布局，图谱照常渲染，不白屏。
        let idx = sample_index();
        let mut value: serde_json::Value =
            serde_json::from_str(&idx.to_json().unwrap()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("graph_positions");
        let legacy = PrebuiltIndex::from_json(&value.to_string()).unwrap();
        assert!(legacy.graph_positions.is_empty());

        let g = legacy.graph_snapshot();
        assert_eq!(g.nodes.len(), 2, "旧产物仍应出图");
        assert_eq!(g.edges.len(), 1);
        for n in &g.nodes {
            assert!(n.x.is_finite() && n.y.is_finite(), "{} 坐标非法", n.path);
            assert!((0.0..=g.width).contains(&n.x));
            assert!((0.0..=g.height).contains(&n.y));
        }
    }

    #[test]
    fn graph_snapshot_uses_precomputed_positions() {
        let mut idx = sample_index();
        idx.graph_positions = vec![crate::locate::GraphPosition {
            path: "a.md".into(),
            x: 123.0,
            y: 456.0,
        }];
        let g = idx.graph_snapshot();
        let a = g.nodes.iter().find(|n| n.path == "a.md").unwrap();
        assert_eq!(a.x, 123.0, "a.md 应使用构建期预计算的坐标");
        assert_eq!(a.y, 456.0);
    }
}

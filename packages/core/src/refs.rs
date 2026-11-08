//! F-06 跨格式引用：在文档中引用其他格式文档的**指定片段**。
//!
//! ## 语法
//!
//! `@相对路径#选择器`，选择器对应 [`crate::locate::TextSegment::label`]：
//!
//! - `@03-台账/模具台账.xlsx#第2行` → Excel 第 2 行
//! - `@05-方案/项目方案.pptx#第1页` → PPT 第 1 页
//! - `@02-图示/评审流程.drawio#节点3` → Drawio 第 3 个节点
//! - `@01-笔记/工艺参数基线.md#2` → 纯数字按 1-based 段序号
//!
//! 选择器按「去空白后的标签包含匹配」解析，因此 `第2行` 能命中
//! `台账 · 第 2 行`；标签匹配不到时再退回选择器里的数字序号。
//!
//! ## 为什么不动 `parse.rs`
//!
//! 既有 `extract_links()` 的 token 字符集不含 `#`，遇到新语法会自然截断到
//! 扩展名，得到**正确的目标路径**，因此带选择器的引用仍会正常进入双链 /
//! 反链 / 图谱，不会产生悬空目标。F-06 只额外解析 `#` 之后的选择器，
//! 与既有索引、双链偏移完全解耦；不带 `#` 的 `@引用` 行为完全不变。
//!
//! 本模块只依赖 `serde` 与纯 Rust 的 `locate` / `format`，wasm32 可编译。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::format::Format;
use crate::locate::{segment_text, TextSegment};

/// 从正文里解析出的引用（尚未解析目标片段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// 原文中的完整 token，例如 `@03-台账/模具台账.xlsx#第2行`。
    pub raw: String,
    /// 目标路径（不含 `#选择器`）。
    pub target: String,
    /// 选择器原文（不含 `#`）。
    pub selector: String,
    /// token 在原文中的字节偏移。
    pub offset: usize,
}

/// 可直接交给 UI 渲染的引用视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceView {
    /// 完整 token，UI 用它把正文切成「文本 + 卡片」。
    pub raw: String,
    /// 解析后的目标文档相对路径（解析失败时等于原 target）。
    pub target_path: String,
    /// 选择器原文。
    pub selector: String,
    /// 命中片段的位置标签，例如「台账 · 第 2 行」。
    pub label: String,
    /// 源片段内容快照（截断，供卡片内联显示）。
    pub snippet: String,
    /// 目标文档中的片段下标，用于点选跳转。
    pub segment_index: usize,
    /// 源片段内容指纹；源文件更新后会变化，用于「标记变更」。
    pub source_hash: u64,
    /// 目标与选择器是否都成功解析。
    pub resolved: bool,
    /// token 在源文档中的字节偏移。
    pub offset: usize,
    /// 命中的流程图节点稳定 id（F-11）。
    ///
    /// 只有当目标文档是 Visio / Drawio、且选择器命中了某个**具体节点**时才有值。
    /// 空串表示这是普通的「段落 / 行 / 页」级引用。
    ///
    /// `#[serde(default)]`：旧产物没有该字段时为空串，语义不变。
    #[serde(default)]
    pub node_id: String,
    /// 源片段相对**上次快照**的变更摘要（F-06 进阶）。
    ///
    /// `None` 表示没有可比对的快照（首次见到该引用），
    /// 或源片段未发生变更。UI 据此渲染「源已变更」的差异块。
    #[serde(default)]
    pub diff: Option<SourceDiff>,
}

impl ReferenceView {
    fn unresolved(r: &Reference) -> Self {
        Self {
            raw: r.raw.clone(),
            target_path: r.target.clone(),
            selector: r.selector.clone(),
            label: String::new(),
            snippet: String::new(),
            segment_index: 0,
            source_hash: 0,
            resolved: false,
            offset: r.offset,
            node_id: String::new(),
            diff: None,
        }
    }
}

/// 差异行的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLineKind {
    /// 未改动的上下文行。
    Context,
    /// 新增行。
    Added,
    /// 删除行。
    Removed,
}

/// 差异中的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

/// 源片段的变更摘要（F-06 进阶：源变更差异高亮）。
///
/// # 为什么需要「快照」
///
/// 引用卡片里的 `source_hash` 只能回答「变没变」，回答不了「变了什么」。
/// 要给出差异，必须有一份**上次看到的内容**作为对照。
///
/// 快照由调用方提供（见 [`build_reference_views_with_snapshots`]）：
/// 本模块不碰存储，只做纯计算，因此三端都能用、Web 端也能用构建期快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDiff {
    /// 新增行数。
    pub added: usize,
    /// 删除行数。
    pub removed: usize,
    /// 行级差异。
    pub lines: Vec<DiffLine>,
}

impl SourceDiff {
    /// 是否存在实际变更。
    pub fn changed(&self) -> bool {
        self.added > 0 || self.removed > 0
    }
}

/// 快照键：`(目标路径, 选择器)` → 唯一键。
///
/// 用 ASCII 单元分隔符（U+001F）而不是 `#`，避免与选择器本身含 `#` 时产生歧义。
pub fn snapshot_key(target_path: &str, selector: &str) -> String {
    format!("mkv:refsnap:{target_path}\u{1f}{selector}")
}

/// 计算两段文本的行级差异。
///
/// 实现策略：
/// 1. 先剥掉公共前缀与公共后缀——真实场景里源文件通常只改了一小块，
///    剥完之后待比较的区间通常很小；
/// 2. 对剩余区间做 LCS（最长公共子序列）动态规划；
/// 3. 区间过大时（LCS 单元格数超过上限）**放弃精细比较**，
///    直接判定为「整体替换」——宁可显示粗一点的结果，也不让 UI 卡住。
///
/// 行尾空白与换行差异不参与比较（`trim_end` 后按行切分）。
pub fn diff_lines(old: &str, new: &str) -> SourceDiff {
    let old_lines: Vec<&str> = old.lines().map(|l| l.trim_end()).collect();
    let new_lines: Vec<&str> = new.lines().map(|l| l.trim_end()).collect();

    if old_lines == new_lines {
        return SourceDiff {
            added: 0,
            removed: 0,
            lines: Vec::new(),
        };
    }

    // 公共前缀
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    // 公共后缀
    let mut suffix = 0usize;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid = &old_lines[prefix..old_lines.len() - suffix];
    let new_mid = &new_lines[prefix..new_lines.len() - suffix];

    let mut lines: Vec<DiffLine> = old_lines[..prefix]
        .iter()
        .map(|t| DiffLine {
            kind: DiffLineKind::Context,
            text: (*t).to_string(),
        })
        .collect();

    let (added, removed) = diff_middle(old_mid, new_mid, &mut lines);

    for t in &new_lines[new_lines.len() - suffix..] {
        lines.push(DiffLine {
            kind: DiffLineKind::Context,
            text: (*t).to_string(),
        });
    }

    SourceDiff {
        added,
        removed,
        lines,
    }
}

/// 对中间区间做 LCS 比较，把结果追加到 `lines`。
///
/// 返回 `(新增行数, 删除行数)`。
fn diff_middle(old: &[&str], new: &[&str], lines: &mut Vec<DiffLine>) -> (usize, usize) {
    // LCS 单元格数上限：超过就退化成「整体替换」，避免大文件把 UI 拖死。
    const MAX_CELLS: usize = 400_000;

    if old.is_empty() && new.is_empty() {
        return (0, 0);
    }
    if old.is_empty() {
        for t in new {
            lines.push(DiffLine {
                kind: DiffLineKind::Added,
                text: (*t).to_string(),
            });
        }
        return (new.len(), 0);
    }
    if new.is_empty() {
        for t in old {
            lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                text: (*t).to_string(),
            });
        }
        return (0, old.len());
    }
    if old.len().saturating_mul(new.len()) > MAX_CELLS {
        for t in old {
            lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                text: (*t).to_string(),
            });
        }
        for t in new {
            lines.push(DiffLine {
                kind: DiffLineKind::Added,
                text: (*t).to_string(),
            });
        }
        return (new.len(), old.len());
    }

    // LCS 动态规划表
    let n = old.len();
    let m = new.len();
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * (m + 1) + j] = if old[i] == new[j] {
                table[(i + 1) * (m + 1) + (j + 1)] + 1
            } else {
                table[(i + 1) * (m + 1) + j].max(table[i * (m + 1) + (j + 1)])
            };
        }
    }

    let (mut added, mut removed) = (0usize, 0usize);
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: old[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if table[(i + 1) * (m + 1) + j] >= table[i * (m + 1) + (j + 1)] {
            lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                text: old[i].to_string(),
            });
            removed += 1;
            i += 1;
        } else {
            lines.push(DiffLine {
                kind: DiffLineKind::Added,
                text: new[j].to_string(),
            });
            added += 1;
            j += 1;
        }
    }
    while i < n {
        lines.push(DiffLine {
            kind: DiffLineKind::Removed,
            text: old[i].to_string(),
        });
        removed += 1;
        i += 1;
    }
    while j < m {
        lines.push(DiffLine {
            kind: DiffLineKind::Added,
            text: new[j].to_string(),
        });
        added += 1;
        j += 1;
    }
    (added, removed)
}

/// 正文渲染片段：普通文本，或一条内联引用卡片。
#[derive(Debug, Clone, PartialEq)]
pub enum TextChunk {
    Plain(String),
    Reference(ReferenceView),
}

/// 从文本中抽取**带 `#选择器`** 的 @ 引用。
///
/// 不带选择器的 `@文件` / `[[双链]]` 一律忽略——它们归既有双链系统处理，
/// F-06 只负责「引用处显示源片段内容」。
pub fn parse_references(text: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // '@' 前紧邻字母/数字时视为邮箱的一部分（与 parse.rs 保持一致）。
        if i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            i += 1;
            continue;
        }

        let rest = &text[i + 1..];
        // 目标部分：到 '#' 或第一个非 token 字符为止。
        let stop = rest.find(|c: char| !is_target_char(c)).unwrap_or(rest.len());
        let target = &rest[..stop];
        if !has_known_extension(target) || !rest[stop..].starts_with('#') {
            i += 1;
            continue;
        }

        // 选择器部分：中文 / 字母 / 数字，不含空白与标点。
        let after_hash = &rest[stop + 1..];
        let sel_len = after_hash
            .find(|c: char| !is_selector_char(c))
            .unwrap_or(after_hash.len());
        let selector = &after_hash[..sel_len];
        if selector.is_empty() {
            i += 1;
            continue;
        }

        let raw_len = 1 + stop + 1 + sel_len; // '@' + target + '#' + selector
        out.push(Reference {
            raw: text[i..i + raw_len].to_string(),
            target: target.to_string(),
            selector: selector.to_string(),
            offset: i,
        });
        i += raw_len;
    }
    out
}

fn is_target_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
}

fn is_selector_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn has_known_extension(target: &str) -> bool {
    let Some((_, ext)) = target.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "markdown" | "xlsx" | "pptx" | "docx" | "vsdx" | "drawio"
    )
}

/// 内容指纹（FNV-1a，纯 Rust、确定性、跨端一致）。
pub fn hash_text(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 在片段列表里解析选择器，返回片段下标。
pub fn match_selector(segments: &[TextSegment], selector: &str) -> Option<usize> {
    let s = compact(selector);
    if s.is_empty() {
        return None;
    }
    // 1) 纯数字 = 1-based 段序号
    if let Ok(n) = s.parse::<usize>() {
        return (n >= 1 && n <= segments.len()).then_some(n - 1);
    }
    // 2) 标签包含匹配（双方都去空白）
    if let Some(seg) = segments.iter().find(|g| compact(&g.label).contains(&s)) {
        return Some(seg.index);
    }
    // 3) 选择器里的数字 → 序号兜底
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Ok(n) = digits.parse::<usize>() {
        return (n >= 1 && n <= segments.len()).then_some(n - 1);
    }
    None
}

/// 选择器匹配结果。
///
/// # 为什么不能只返回一个裸下标
///
/// **节点清单**与**文本片段**是两个独立的下标空间：
/// 前者按流程图里的节点顺序编号，后者按原文的行 / 段顺序编号。
///
/// 早期实现用同一个 `usize` 承载两种命中：「命中节点」返回的是节点下标，
/// 「退回片段匹配」返回的是片段下标，而调用方一律拿它去索引片段数组。
/// 结果就是节点命中会指向错误的片段，甚至越界。
///
/// 这里把两种命中**显式区分**，让调用方按类型取数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorMatch {
    /// 命中某个流程图节点，值为**节点下标**。
    Node(usize),
    /// 命中文本片段，值为**片段下标**。
    Segment(usize),
}

/// 在**流程图节点**清单里解析选择器（F-11 节点级引用）。
///
/// 与 [`match_selector`] 的关系：片段选择器按「序号 / 标签」匹配，
/// 节点选择器优先按**稳定 id** 匹配（Drawio 的 `mxCell@id`、Visio 的 shape ID），
/// 因为 id 才是真正稳定的东西——序号会随图编辑而漂移。
///
/// 节点匹配规则统一由 [`crate::diagram::match_node_selector`] 实现，
/// 避免两处各写一份、日后走样。
pub fn match_node_selector(
    nodes: &[crate::diagram::DiagramNode],
    segments: &[TextSegment],
    selector: &str,
) -> Option<SelectorMatch> {
    if !nodes.is_empty() {
        if let Some(i) = crate::diagram::match_node_selector(nodes, selector) {
            return Some(SelectorMatch::Node(i));
        }
    }
    // 退回片段选择器（节点清单为空，或选择器指向的是页 / 段 / 行）
    match_selector(segments, selector).map(SelectorMatch::Segment)
}

/// 构造引用 token：`@相对路径#选择器`。
/// 路径若含空白或 `#`，会被拒绝（返回 None），避免产生无法解析的 token。
///
/// # 为什么校验得这么严
///
/// [`parse_references`] 的字符集很窄：目标部分只认字母 / 数字 / `_ - . /`，
/// 选择器部分只认字母 / 数字（中文也算字母）。一旦掺进空格、`#` 或其它符号，
/// 解析出来的 token 就和写进正文的不一致——引用会**静默失效**，
/// 用户却以为已经插好了。因此这里宁可返回 `None`，也不产出解析端认不出来的 token。
///
/// # 规则
///
/// - 目标路径：非空、**相对路径**（不能以 `/` 开头）、只用 `/` 分隔、
///   不含空白、不含 `#`，且每个字符都在目标字符集内；
/// - 选择器：非空、不含空白、不含 `#`，且每个字符都在选择器字符集内
///   （纯数字表示 1-based 段序号，见 [`match_selector`]）。
///
/// # 示例
///
/// ```
/// use mkv_core::refs::format_reference_token;
/// assert_eq!(
///     format_reference_token("03-台账/模具台账.xlsx", "2"),
///     Some("@03-台账/模具台账.xlsx#2".to_string())
/// );
/// assert_eq!(format_reference_token("/绝对路径.md", "1"), None);
/// ```
pub fn format_reference_token(target_path: &str, selector: &str) -> Option<String> {
    if !is_valid_target_path(target_path) || !is_valid_selector(selector) {
        return None;
    }
    Some(format!("@{target_path}#{selector}"))
}

/// 目标路径是否合法（与 [`parse_references`] 的目标字符集保持一致）。
///
/// 只允许 `/` 作分隔符：Windows 风格的 `\` 会让同一个相对路径出现两种写法，
/// 引用也就无法与文档清单里的路径对上。
fn is_valid_target_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

/// 选择器是否合法（与 [`parse_references`] 的选择器字符集保持一致）。
fn is_valid_selector(selector: &str) -> bool {
    !selector.is_empty() && selector.chars().all(is_selector_char)
}

/// 从目标文档的片段列表里，挑出适合做选择器的候选（label 文本）。
/// 返回 (候选序号, 标签, 类型) 三元组；候选序号是 1-based。
///
/// # 候选序号是什么
///
/// 候选序号是**去重后候选列表里的位置**（1-based，即 1、2、3……），
/// 而不是原始片段下标 + 1：被去重丢弃的片段**不占号**，
/// 保留下来的候选连续编号，UI 侧按「第几个候选」展示时不会跳号。
///
/// # 候选序号为什么是 1-based
///
/// [`match_selector`] 把纯数字选择器解释成**第几个片段**（1-based），
/// 而不是 [`TextSegment::index`] 那个 0-based 下标。
/// 候选里直接给出 1-based 序号，UI 侧就能原样拼成 `#n`，
/// 不必再在 UI 里做一次 +1 / -1 的换算，也就不会差一位。
///
/// 序号在**去重时**就定下来，node 优先排序只影响**展示顺序**：
/// 被排到前面的 node 仍带着它原本的序号，展示顺序与序号可以不一致。
///
/// 需注意：候选序号是**去重后候选列表**的位置，而 [`match_selector`]
/// 的纯数字分支命中的是**原始片段列表**的位置。
/// 两者仅在「没有片段被去重丢弃」时完全一致；
/// 一旦前面有重复标签被丢弃，用候选序号拼出的 `#n` 会指向相邻片段。
/// 这种场景请改用标签选择器（`#第1段`），它按标签匹配、不受去重影响。
///
/// # 为什么 node 排在最前
///
/// Visio / Drawio 的**节点级**引用最精确：它能落到某一个具体图形上，
/// 而「第 3 页」只能落到整页。把节点候选排前面，用户点第一个就能拿到最精确的引用。
///
/// # 去重
///
/// 同一 (标签, 类型) 只保留**第一次**出现的那一项：标签是给人看的，
/// 重复的标签（例如两个 sheet 都叫「第 1 行」）既难分辨，参考价值也低。
/// 类型参与去重键，是因为同名但不同类型的片段（例如「节点 1」与「第 1 页」）
/// 在语义上是两类不同的引用目标，不该被合并掉。
pub fn selector_candidates(segments: &[TextSegment]) -> Vec<(usize, String, String)> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out: Vec<(usize, String, String)> = Vec::new();
    for seg in segments {
        let label = seg.label.trim();
        if label.is_empty() {
            continue;
        }
        if !seen.insert((label.to_string(), seg.kind.clone())) {
            continue;
        }
        // 序号 = 去重后候选列表中的 1-based 位置：被去重丢弃的片段不占号。
        let seq = out.len() + 1;
        out.push((seq, label.to_string(), seg.kind.clone()));
    }
    // node 优先；`sort_by_key` 是稳定排序，其余候选保持原有顺序。
    out.sort_by_key(|(_, _, kind)| u8::from(kind != "node"));
    out
}

/// 目标路径解析：精确路径 → 路径后缀 → 文件名（不含扩展名）。
fn resolve_target<'a>(
    target: &str,
    docs: &'a [(String, String, Format)],
) -> Option<&'a (String, String, Format)> {
    if let Some(d) = docs.iter().find(|(p, _, _)| p == target) {
        return Some(d);
    }
    if let Some(d) = docs
        .iter()
        .find(|(p, _, _)| p.ends_with(target) || target.ends_with(p.as_str()))
    {
        return Some(d);
    }
    let stem = target.rsplit('/').next().unwrap_or(target);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    docs.iter().find(|(p, _, _)| {
        let name = p.rsplit('/').next().unwrap_or(p);
        let base = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        base == stem
    })
}

/// 生成一份文档的全部引用视图。
///
/// `docs` 为全库 `(相对路径, 抽取后的纯文本, 格式)`，用于解析目标片段。
/// 构建期（导出索引）与运行期（Desktop 实时）共用这一套语义。
pub fn build_reference_views(
    source_text: &str,
    docs: &[(String, String, Format)],
) -> Vec<ReferenceView> {
    build_reference_views_with_nodes(source_text, docs, &HashMap::new())
}

/// 生成一份文档的全部引用视图，**带流程图节点清单**（F-11 节点级引用）。
///
/// `node_map` 的键是文档相对路径，值是该文档的节点清单。
/// 只有 Drawio / Visio 文档会有节点；其余格式不参与匹配。
///
/// 节点清单存在时，选择器**优先按稳定 id 匹配**，
/// 这样 `@flow.drawio#node-a` 能精确定位到某个图形节点，
/// 而不依赖「节点 3」这种会随编辑漂移的序号。
pub fn build_reference_views_with_nodes(
    source_text: &str,
    docs: &[(String, String, Format)],
    node_map: &HashMap<String, Vec<crate::diagram::DiagramNode>>,
) -> Vec<ReferenceView> {
    build_reference_views_with_snapshots(source_text, docs, node_map, &HashMap::new())
}

/// 生成一份文档的全部引用视图，**带流程图节点清单与源变更快照**。
///
/// # 参数
///
/// - `node_map`：键为文档相对路径，值为该文档的流程图节点清单。
///   只有 Drawio / Visio 文档会有节点；其余格式不参与匹配。
///   节点清单存在时，选择器**优先按稳定 id 匹配**，
///   这样 `@flow.drawio#node-a` 能精确定位到某个图形节点，
///   而不依赖「节点 3」这种会随编辑漂移的序号。
/// - `snapshots`：键为 [`snapshot_key`]，值为**上次看到**的源片段文本。
///   命中快照且内容有变化时，引用视图会带上 [`SourceDiff`] 差异摘要。
///   没有快照（首次见到）时不产出差异，只给指纹。
pub fn build_reference_views_with_snapshots(
    source_text: &str,
    docs: &[(String, String, Format)],
    node_map: &HashMap<String, Vec<crate::diagram::DiagramNode>>,
    snapshots: &HashMap<String, String>,
) -> Vec<ReferenceView> {
    parse_references(source_text)
        .into_iter()
        .map(|r| {
            let Some(res) = resolve_reference(&r, docs, node_map) else {
                return ReferenceView::unresolved(&r);
            };
            // 与上次快照比对，给出「源已变更」的行级差异
            let diff = snapshots
                .get(&snapshot_key(&res.path, &r.selector))
                .and_then(|old| {
                    let d = diff_lines(old, &res.current);
                    if d.changed() { Some(d) } else { None }
                });
            ReferenceView {
                raw: r.raw,
                target_path: res.path,
                selector: r.selector,
                label: res.label,
                snippet: res.snippet,
                segment_index: res.segment_index,
                source_hash: res.source_hash,
                resolved: true,
                offset: r.offset,
                node_id: res.node_id,
                diff,
            }
        })
        .collect()
}

/// 一条引用解析后的全部信息。
struct ResolvedRef {
    /// 目标文档相对路径。
    path: String,
    /// 展示标题（节点为「页 · 文本」，片段为位置标签）。
    label: String,
    /// 摘要（截断后的节点文本或片段原文）。
    snippet: String,
    /// 命中的流程图节点 id（普通引用为空串）。
    node_id: String,
    /// 源片段指纹。
    source_hash: u64,
    /// 跳转用的片段下标。
    segment_index: usize,
    /// 当前看到的源片段全文（用于与快照做差异比对）。
    current: String,
}

/// 解析一条引用。
///
/// 节点命中与片段命中的展示形态不同：
/// - 节点：标题是「页 · 节点文本」，摘要取节点文本，指纹取节点身份；
/// - 片段：标题是位置标签，摘要取片段原文，指纹取片段原文。
fn resolve_reference(
    r: &Reference,
    docs: &[(String, String, Format)],
    node_map: &HashMap<String, Vec<crate::diagram::DiagramNode>>,
) -> Option<ResolvedRef> {
    let (path, text, format) = resolve_target(&r.target, docs)?;
    let segments = segment_text(text, *format);
    let empty_nodes: Vec<crate::diagram::DiagramNode> = Vec::new();
    let nodes = node_map.get(path).unwrap_or(&empty_nodes);
    let matched = match_node_selector(nodes, &segments, &r.selector)?;
    match matched {
        SelectorMatch::Node(i) => {
            let n = &nodes[i];
            let snippet: String = n.label.chars().take(120).collect();
            // 跳转目标：优先找原文里含该节点文本的片段，找不到就回到开头
            let seg_idx = segments
                .iter()
                .find(|s| !n.label.is_empty() && s.text.contains(&n.label))
                .map(|s| s.index)
                .unwrap_or(0);
            Some(ResolvedRef {
                path: path.clone(),
                label: n.display_label(),
                snippet: snippet.trim().to_string(),
                node_id: n.id.clone(),
                source_hash: hash_text(&format!("{}|{}", n.id, n.label)),
                segment_index: seg_idx,
                current: n.label.clone(),
            })
        }
        SelectorMatch::Segment(i) => {
            let seg = &segments[i];
            Some(ResolvedRef {
                path: path.clone(),
                label: seg.label.clone(),
                snippet: seg.text.chars().take(120).collect::<String>().trim().to_string(),
                node_id: String::new(),
                source_hash: hash_text(&seg.text),
                segment_index: seg.index,
                current: seg.text.clone(),
            })
        }
    }
}

/// 解析一份文档里所有引用的「目标路径 + 当前源片段文本」。
///
/// 用于**建立快照基线**：调用方把结果写进存储，
/// 之后源文件被修改时即可与当前内容做行级差异比对。
///
/// 返回 `(目标路径, 选择器, 当前源片段原文)` 三元组。
pub fn resolve_reference_sources(
    source_text: &str,
    docs: &[(String, String, Format)],
    node_map: &HashMap<String, Vec<crate::diagram::DiagramNode>>,
) -> Vec<(String, String, String)> {
    parse_references(source_text)
        .into_iter()
        .filter_map(|r| {
            let res = resolve_reference(&r, docs, node_map)?;
            Some((res.path, r.selector, res.current))
        })
        .collect()
}

/// 把一段正文按引用 token 切成可顺序渲染的片段。
///
/// 只按解析阶段记录的**字节偏移**切分，不做二次字符串查找，
/// 因此同一 token 重复出现时也不会错位；切片均落在 UTF-8 字符边界。
pub fn render_chunks(text: &str, views: &[ReferenceView]) -> Vec<TextChunk> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for v in views {
        let end = v.offset.saturating_add(v.raw.len());
        if v.offset < cursor
            || end > text.len()
            || !text.is_char_boundary(v.offset)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        if v.offset > cursor {
            out.push(TextChunk::Plain(text[cursor..v.offset].to_string()));
        }
        out.push(TextChunk::Reference(v.clone()));
        cursor = end;
    }
    if cursor < text.len() {
        out.push(TextChunk::Plain(text[cursor..].to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<(String, String, Format)> {
        vec![
            (
                "03-台账/模具台账.xlsx".into(),
                "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n台账: PN-2048 | MDL-11\n"
                    .into(),
                Format::Excel,
            ),
            (
                "05-方案/项目方案.pptx".into(),
                "--- ppt/slides/slide1.xml ---\n项目方案\n模流分析结论\n".into(),
                Format::PowerPoint,
            ),
        ]
    }

    #[test]
    fn parses_only_refs_with_selectors() {
        let text = "见 @03-台账/模具台账.xlsx#第2行 与 @05-方案/项目方案.pptx#第1页；\n\
                    普通 @工艺参数基线.md 不算，邮箱 someone@example.com 也不算。";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 2, "got {refs:?}");
        assert_eq!(refs[0].target, "03-台账/模具台账.xlsx");
        assert_eq!(refs[0].selector, "第2行");
        assert_eq!(refs[1].selector, "第1页");
        assert_eq!(&text[refs[0].offset..refs[0].offset + refs[0].raw.len()], refs[0].raw);
    }

    #[test]
    fn selector_matches_labels_and_numeric_index() {
        let docs = sample_docs();
        let excel = segment_text(&docs[0].1, Format::Excel);
        assert_eq!(match_selector(&excel, "第2行"), Some(1));
        assert_eq!(match_selector(&excel, "2"), Some(1));
        let ppt = segment_text(&docs[1].1, Format::PowerPoint);
        assert_eq!(match_selector(&ppt, "第1页"), Some(0));
        assert_eq!(match_selector(&ppt, "不存在的片段"), None);
    }

    #[test]
    fn views_resolve_real_segments_and_detect_source_change() {
        let docs = sample_docs();
        let source = "台账行：@03-台账/模具台账.xlsx#第2行\n方案页：@05-方案/项目方案.pptx#第1页\n";
        let views = build_reference_views(source, &docs);
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| v.resolved), "got {views:#?}");
        assert!(views[0].snippet.contains("PN-1024"), "{}", views[0].snippet);
        assert!(views[0].label.contains("第 2 行"), "{}", views[0].label);

        // 源文档第 2 行内容变化 → 指纹必须变化（PRD「标记变更」）
        let mut changed = docs.clone();
        changed[0].1 = changed[0].1.replace("PN-1024", "PN-9999");
        let after = build_reference_views(source, &changed);
        assert_ne!(views[0].source_hash, after[0].source_hash);
        assert!(after[0].snippet.contains("PN-9999"));
    }

    #[test]
    fn unresolved_target_or_selector_is_reported_not_panicking() {
        let views = build_reference_views(
            "缺目标 @不存在.xlsx#第1行，缺片段 @03-台账/模具台账.xlsx#第99行",
            &sample_docs(),
        );
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| !v.resolved), "got {views:#?}");
    }

    #[test]
    fn render_chunks_is_lossless() {
        let docs = sample_docs();
        let source = "台账行：@03-台账/模具台账.xlsx#第2行\n方案页：@05-方案/项目方案.pptx#第1页\n结尾";
        let views = build_reference_views(source, &docs);
        let chunks = render_chunks(source, &views);
        let rebuilt: String = chunks
            .iter()
            .map(|c| match c {
                TextChunk::Plain(s) => s.as_str(),
                TextChunk::Reference(v) => v.raw.as_str(),
            })
            .collect();
        assert_eq!(rebuilt, source, "切分后必须能无损拼回原文");
        assert_eq!(
            chunks.iter().filter(|c| matches!(c, TextChunk::Reference(_))).count(),
            2
        );
    }

    #[test]
    fn hash_is_stable_across_calls_and_sensitive_to_content() {
        assert_eq!(hash_text("冷却水路"), hash_text("冷却水路"));
        assert_ne!(hash_text("冷却水路"), hash_text("冷却水道"));
    }

    // ------------------------------------------------------ 引用 token 构造

    fn seg(index: usize, label: &str, kind: &str) -> TextSegment {
        TextSegment {
            index,
            label: label.to_string(),
            kind: kind.to_string(),
            text: label.to_string(),
        }
    }

    #[test]
    fn formats_valid_reference_tokens() {
        assert_eq!(
            format_reference_token("03-台账/模具台账.xlsx", "2"),
            Some("@03-台账/模具台账.xlsx#2".to_string())
        );
        assert_eq!(
            format_reference_token("01-笔记/工艺参数基线.md", "标题"),
            Some("@01-笔记/工艺参数基线.md#标题".to_string())
        );
    }

    /// 生成的 token 必须能被 [`parse_references`] **原样**解析回来。
    ///
    /// 这是本函数存在的全部意义：若解析出的 target / selector 与写入的不一致，
    /// 引用会静默指向错误片段，用户却毫无察觉。
    #[test]
    fn formatted_token_round_trips_through_parser() {
        let token = format_reference_token("03-台账/模具台账.xlsx", "2").expect("应能构造");
        let parsed = parse_references(&token);
        assert_eq!(parsed.len(), 1, "应恰好解析出一条引用：{parsed:?}");
        assert_eq!(parsed[0].target, "03-台账/模具台账.xlsx");
        assert_eq!(parsed[0].selector, "2");
        assert_eq!(parsed[0].raw, token);
    }

    #[test]
    fn rejects_invalid_paths_and_selectors() {
        // 空路径 / 空选择器
        assert_eq!(format_reference_token("", "1"), None);
        assert_eq!(format_reference_token("a.md", ""), None);
        // 含空白：会被解析端截断，token 与解析结果必然不一致
        assert_eq!(format_reference_token("我的 文档.md", "1"), None);
        assert_eq!(format_reference_token("a.md", "第 1 段"), None);
        // 含 '#'
        assert_eq!(format_reference_token("a.md#x", "1"), None);
        assert_eq!(format_reference_token("a.md", "1#2"), None);
        // 绝对路径无法被 resolve_target 命中
        assert_eq!(format_reference_token("/a.md", "1"), None);
        // 换行 / 制表符
        assert_eq!(format_reference_token("a\nb.md", "1"), None);
        assert_eq!(format_reference_token("a.md", "1\t2"), None);
        // 解析端字符集之外的符号
        assert_eq!(format_reference_token("a(1).md", "1"), None);
        assert_eq!(format_reference_token("a.md", "第1段!"), None);
    }

    // ------------------------------------------------------ 选择器候选

    #[test]
    fn selector_candidates_use_one_based_indexes_and_dedup_labels() {
        let segments = vec![
            seg(0, "标题 · 模流分析", "heading"),
            seg(1, "第 1 段", "paragraph"),
            seg(2, "第 1 段", "paragraph"), // 重复标签
            seg(3, "第 2 段", "paragraph"),
        ];
        let cands = selector_candidates(&segments);
        assert_eq!(cands.len(), 3, "重复标签只保留第一个：{cands:?}");
        assert_eq!(cands[0], (1, "标题 · 模流分析".to_string(), "heading".to_string()));
        // 去重后保留的是**第一次**出现的位置，因此序号仍是 2 而不是 3
        assert_eq!(cands[1], (2, "第 1 段".to_string(), "paragraph".to_string()));
        assert_eq!(cands[2], (3, "第 2 段".to_string(), "paragraph".to_string()));
    }

    #[test]
    fn selector_candidates_put_nodes_first() {
        let segments = vec![
            seg(0, "第 1 页", "page"),
            seg(1, "节点 1", "node"),
            seg(2, "第 2 页", "page"),
            seg(3, "节点 2", "node"),
        ];
        let cands = selector_candidates(&segments);
        let kinds: Vec<&str> = cands.iter().map(|(_, _, k)| k.as_str()).collect();
        assert_eq!(kinds, vec!["node", "node", "page", "page"]);
        // 排序只影响展示顺序，序号仍指向原片段位置
        assert_eq!(cands[0].0, 2);
        assert_eq!(cands[1].0, 4);
    }

    #[test]
    fn selector_candidates_of_empty_input_is_empty() {
        assert!(selector_candidates(&[]).is_empty());
    }

    /// 候选序号必须与 [`match_selector`] 的语义**严格对齐**，
    /// 否则 UI 拼出的 `#n` 会落到相邻片段上。
    #[test]
    fn candidate_indexes_match_match_selector() {
        let segs = segment_text("Sheet1: 浇口 | 位置\nSheet1: A2 | B2\n", Format::Excel);
        let cands = selector_candidates(&segs);
        assert!(!cands.is_empty());
        for (n, _, _) in &cands {
            assert_eq!(
                match_selector(&segs, &n.to_string()),
                Some(*n - 1),
                "候选序号 {n} 必须命中第 {n} 个片段"
            );
        }
    }
}

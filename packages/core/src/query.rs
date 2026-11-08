//! 检索查询模型（F-12 正则检索 / F-07 多维过滤的共同契约）。
//!
//! # 为什么单独一个模块
//!
//! v0.1 的检索接口是 `search(&self, query: &str, limit: usize) -> Vec<SearchHit>`，
//! 只有一个裸字符串。要支持「正则模式」与「多维过滤」，光加参数会很快失控：
//! `search(query, limit, regex, format, tag, from, to, source)` 这种八个位置参数
//! 在调用点完全不可读，且每加一个过滤维度就要改所有实现（native / web / null）。
//!
//! 因此改成**一个查询对象**：
//!
//! - [`SearchQuery`] 描述「查什么」：文本、模式、过滤器、条数上限；
//! - [`QueryMode`] 区分普通词项检索与正则检索；
//! - [`QueryFilter`] 描述「在哪些文档里查」：格式 / 标签 / 时间窗 / 来源。
//!
//! 过滤器是**纯数据 + 纯函数求值**：[`QueryFilter::matches`] 只看一条
//! [`crate::types::SearchHit`] 与一份标签表，不碰索引、不碰文件系统，
//! 因此三端（Web / Desktop / Mobile）可以共用同一套语义，也便于单测。
//!
//! # 向后兼容
//!
//! [`crate::service::KnowledgeService::search`] 保持原签名不变，
//! 新增的 [`crate::service::KnowledgeService::search_query`] 默认实现
//! 就是「旧 search + 过滤」。这样既有调用方与既有实现（`NullService`、
//! 第三方实现）都不需要改。

use serde::{Deserialize, Serialize};

use crate::types::SearchHit;

/// 检索模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QueryMode {
    /// 普通词项检索：查询词经分词后按 AND 语义匹配（F-02 既有行为）。
    #[default]
    Terms,
    /// 正则检索：把查询当正则表达式，在原文上匹配（F-12）。
    ///
    /// 语义与普通检索**刻意不同**：正则不做分词，直接对原文做模式匹配。
    /// 因此 `模流.*分析` 能命中「模流分析」「模流与仿真分析」，
    /// 而普通模式下 `模流 分析` 是两个独立词项。
    Regex,
}

impl QueryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryMode::Terms => "terms",
            QueryMode::Regex => "regex",
        }
    }
}

/// 检索过滤器（F-07：标签 / 类型 / 时间 / 来源多维组合）。
///
/// 语义：**各维度之间是 AND，维度内部是 OR**。
///
/// - 格式：命中任意一个即通过；为空表示不限格式。
/// - 标签：命中任意一个即通过；为空表示不限标签。
/// - 时间窗：`modified_from` / `modified_to` 闭区间，Unix 秒；`None` 表示该侧不限。
/// - 来源：`source` 对**路径前缀**做匹配（如 `03-台账/`）；为空表示不限。
///
/// 之所以「维度内 OR」而不是「AND」：用户勾选「Excel」和「PPT」时，
/// 期望是「这两类都要看」，而不是「既是 Excel 又是 PPT」（空集）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryFilter {
    /// 限定格式（如 `Excel` / `Markdown`）。空 = 不限。
    pub formats: Vec<String>,
    /// 限定标签。空 = 不限。
    pub tags: Vec<String>,
    /// 修改时间下界（含，Unix 秒）。`None` = 不限。
    pub modified_from: Option<u64>,
    /// 修改时间上界（含，Unix 秒）。`None` = 不限。
    pub modified_to: Option<u64>,
    /// 来源路径前缀（如 `03-台账/`）。空 = 不限。
    pub source: Option<String>,
}

impl QueryFilter {
    /// 是否为空过滤器（不限任何维度）。
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
            && self.tags.is_empty()
            && self.modified_from.is_none()
            && self.modified_to.is_none()
            && self.source.as_deref().map(str::is_empty).unwrap_or(true)
    }

    /// 判断一条命中是否通过过滤。
    ///
    /// `tags_of` 用于取出该文档的标签（Web 端来自预构建索引，Desktop 端来自
    /// 元数据库）。传入闭包而不是直接收标签表，是为了让调用方决定标签从哪来，
    /// 也避免在此处产生 `HashMap` 分配。
    pub fn matches(
        &self,
        hit: &SearchHit,
        modified_at: u64,
        tags_of: impl Fn(&str) -> Vec<String>,
    ) -> bool {
        // 格式
        if !self.formats.is_empty()
            && !self
                .formats
                .iter()
                .any(|f| f.eq_ignore_ascii_case(hit.format.trim()))
        {
            return false;
        }

        // 标签
        if !self.tags.is_empty() {
            let doc_tags = tags_of(&hit.path);
            if !self.tags.iter().any(|want| {
                doc_tags
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(want.trim()))
            }) {
                return false;
            }
        }

        // 修改时间窗（闭区间）
        if let Some(from) = self.modified_from {
            if modified_at < from {
                return false;
            }
        }
        if let Some(to) = self.modified_to {
            if modified_at > to {
                return false;
            }
        }

        // 来源前缀
        if let Some(src) = self.source.as_deref() {
            let src = src.trim();
            if !src.is_empty() && !hit.path.starts_with(src) {
                return false;
            }
        }

        true
    }
}

/// 一次检索请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    /// 查询文本。普通模式下是关键词，正则模式下是正则表达式。
    pub text: String,
    /// 检索模式。
    pub mode: QueryMode,
    /// 过滤器。
    pub filter: QueryFilter,
    /// 结果条数上限。
    pub limit: usize,
}

impl SearchQuery {
    /// 普通词项检索（无过滤）。
    pub fn terms(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            mode: QueryMode::Terms,
            filter: QueryFilter::default(),
            limit,
        }
    }

    /// 正则检索（无过滤）。
    pub fn regex(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            mode: QueryMode::Regex,
            filter: QueryFilter::default(),
            limit,
        }
    }

    /// 附加过滤器（builder 风格）。
    pub fn with_filter(mut self, filter: QueryFilter) -> Self {
        self.filter = filter;
        self
    }

    /// 切换模式。
    pub fn with_mode(mut self, mode: QueryMode) -> Self {
        self.mode = mode;
        self
    }

    /// 查询文本去掉首尾空白后是否为空。
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// 命中片段的起始标记（正则检索摘要用）。
///
/// # 为什么用哨兵字符而不是让 UI 自己高亮
///
/// UI crate（`mkv-ui`）刻意不引入正则引擎——那会给 wasm 产物增加体积，
/// 而正则检索本身已经由 core 用自研零依赖引擎完成。若 UI 拿到的是一段
/// 纯文本摘要，它无法判断哪一段是命中、哪一段是上下文。
///
/// 因此由 core 在生成摘要时就把命中片段夹在哨兵字符之间，
/// UI 只需按标记切分并渲染 `<mark>`，不需要任何模式匹配能力。
pub const MATCH_MARK_BEGIN: char = '\u{3008}'; // 《
/// 命中片段的结束标记（正则检索摘要用）。
pub const MATCH_MARK_END: char = '\u{3009}'; // 》

/// 生成正则命中摘要：截取命中处上下文，并用哨兵字符标记命中片段。
///
/// `start` / `end` 是**字节偏移**（来自 [`crate::regex::Regex::find_at`]）。
/// 本函数内部换算成字符下标，因此对多字节中文安全。
pub fn regex_snippet(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start_byte = start.min(text.len());
    let end_byte = end.min(text.len()).max(start_byte);
    let start_char = text[..start_byte].chars().count();
    let end_char = text[..end_byte].chars().count().max(start_char);

    let matched: String = chars[start_char..end_char].iter().collect();
    let lo = start_char.saturating_sub(30);
    let hi = (end_char + 60).min(chars.len());
    let prefix: String = chars[lo..start_char].iter().collect();
    let suffix: String = chars[end_char..hi].iter().collect();

    let prefix = prefix.trim().replace('\n', " ");
    let suffix = suffix.trim().replace('\n', " ");
    let matched = matched.replace('\n', " ");

    let mut out = String::new();
    if lo > 0 {
        out.push('\u{2026}'); // …
    }
    out.push_str(&prefix);
    out.push(MATCH_MARK_BEGIN);
    out.push_str(&matched);
    out.push(MATCH_MARK_END);
    out.push_str(&suffix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, format: &str) -> SearchHit {
        SearchHit {
            path: path.to_string(),
            title: "t".to_string(),
            format: format.to_string(),
            score: 1.0,
            snippet: String::new(),
            modified_at: 0,
            tags: Vec::new(),
        }
    }

    #[test]
    fn empty_filter_passes_everything() {
        let f = QueryFilter::default();
        assert!(f.is_empty());
        let h = hit("a/x.md", "Markdown");
        assert!(f.matches(&h, 0, |_| vec![]));
    }

    #[test]
    fn format_dimension_is_or_within_and_and_across_dimensions() {
        let f = QueryFilter {
            formats: vec!["excel".into(), "ppt".into()],
            ..Default::default()
        };
        assert!(f.matches(&hit("a/x.xlsx", "Excel"), 0, |_| vec![]));
        assert!(f.matches(&hit("a/x.pptx", "PPT"), 0, |_| vec![]));
        assert!(!f.matches(&hit("a/x.md", "Markdown"), 0, |_| vec![]));

        // 跨维度是 AND：格式过了但时间窗没过，仍然不通过
        let f2 = QueryFilter {
            formats: vec!["excel".into()],
            modified_from: Some(100),
            ..Default::default()
        };
        assert!(!f2.matches(&hit("a/x.xlsx", "Excel"), 50, |_| vec![]));
        assert!(f2.matches(&hit("a/x.xlsx", "Excel"), 150, |_| vec![]));
    }

    #[test]
    fn tag_dimension_matches_any_requested_tag() {
        let f = QueryFilter {
            tags: vec!["模具".into(), "工艺".into()],
            ..Default::default()
        };
        let tags = |_: &str| vec!["模具".to_string(), "待评审".to_string()];
        assert!(f.matches(&hit("a/x.md", "Markdown"), 0, tags));
        let tags2 = |_: &str| vec!["其它".to_string()];
        assert!(!f.matches(&hit("a/x.md", "Markdown"), 0, tags2));
    }

    #[test]
    fn time_window_is_closed_interval() {
        let f = QueryFilter {
            modified_from: Some(100),
            modified_to: Some(200),
            ..Default::default()
        };
        assert!(!f.matches(&hit("a.md", "Markdown"), 99, |_| vec![]));
        assert!(f.matches(&hit("a.md", "Markdown"), 100, |_| vec![]));
        assert!(f.matches(&hit("a.md", "Markdown"), 200, |_| vec![]));
        assert!(!f.matches(&hit("a.md", "Markdown"), 201, |_| vec![]));
    }

    #[test]
    fn source_prefix_filters_by_path() {
        let f = QueryFilter {
            source: Some("03-台账/".into()),
            ..Default::default()
        };
        assert!(f.matches(&hit("03-台账/a.xlsx", "Excel"), 0, |_| vec![]));
        assert!(!f.matches(&hit("01-笔记/a.md", "Markdown"), 0, |_| vec![]));
    }

    #[test]
    fn query_builders_set_mode_and_filter() {
        let q = SearchQuery::terms("模流", 10).with_filter(QueryFilter {
            formats: vec!["Excel".into()],
            ..Default::default()
        });
        assert_eq!(q.mode, QueryMode::Terms);
        assert_eq!(q.limit, 10);
        assert_eq!(q.filter.formats, vec!["Excel".to_string()]);

        let r = SearchQuery::regex("模流.*分析", 5);
        assert_eq!(r.mode, QueryMode::Regex);
        assert!(!r.is_blank());
        assert!(SearchQuery::terms("   ", 5).is_blank());
    }

    #[test]
    fn regex_snippet_marks_the_matched_part() {
        let text = "前面的内容。模流分析用于评估浇口位置。后面的内容。";
        let start = text.find("模流分析").expect("应能定位");
        let end = start + "模流分析".len();
        let snip = regex_snippet(text, start, end);
        assert!(
            snip.contains(&format!("{MATCH_MARK_BEGIN}模流分析{MATCH_MARK_END}")),
            "命中片段应被哨兵包裹，实际：{snip}"
        );
        assert!(snip.contains("浇口位置"), "应保留上下文，实际：{snip}");
    }

    #[test]
    fn regex_snippet_handles_multibyte_boundaries() {
        let text = "模流分析";
        let snip = regex_snippet(text, 0, "模流".len());
        assert!(snip.contains(&format!("{MATCH_MARK_BEGIN}模流{MATCH_MARK_END}")));
        assert!(snip.contains("分析"), "多字节边界不应吃掉尾部，实际：{snip}");
    }

    #[test]
    fn regex_snippet_of_empty_match_is_still_well_formed() {
        let snip = regex_snippet("abc", 1, 1);
        assert!(snip.contains(MATCH_MARK_BEGIN) && snip.contains(MATCH_MARK_END));
    }
}

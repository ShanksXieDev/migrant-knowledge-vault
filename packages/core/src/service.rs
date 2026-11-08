//! 知识库服务抽象：UI 层与核心实现之间的契约。
//!
//! UI 层（`mkv-ui`）只依赖本 trait，不直接依赖 Tantivy / SQLite / Git，
//! 因此可在 wasm32（Web）与原生（Desktop / Mobile）两端编译。
//!
//! - Desktop / Mobile：由 `native` 特性下的 [`crate::native::NativeService`] 实现。
//! - Web：采用预构建只读索引（见技术方案设计 §4），由 Web 端自行实现。

use crate::types::{DocumentRecord, LinkRecord, ReferenceTarget, SearchHit};

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

    /// 按查询对象检索（F-12 正则模式 + F-07 多维过滤）。
    ///
    /// 默认实现 = 「旧 search + 过滤」，因此**既有实现无需修改**：
    /// - 普通模式且无过滤器 → 直接走旧 `search`；
    /// - 有过滤器 → 在旧结果上做纯数据过滤（[`crate::query::QueryFilter::matches`]）；
    /// - 正则模式 → 默认**不支持**，返回空并由 UI 提示「该后端不支持正则检索」。
    ///
    /// 需要真正支持正则检索的后端覆写本方法。
    fn search_query(&self, query: &crate::query::SearchQuery) -> Vec<SearchHit> {
        if query.is_blank() {
            return Vec::new();
        }
        if query.mode == crate::query::QueryMode::Regex {
            // 默认后端不实现正则：返回空比返回普通检索结果诚实，
            // 否则用户会以为正则生效了。
            return Vec::new();
        }
        let hits = self.search(&query.text, query.limit);
        self.apply_filter(hits, &query.filter)
    }

    /// 校验查询是否可用（F-12 正则模式）。
    ///
    /// 返回 `Some(说明)` 表示查询有误，`None` 表示可用。
    ///
    /// 之所以需要这个方法：正则写错与「正则合法但没命中」在
    /// [`Self::search_query`] 眼里都是空结果集。UI 若只看到空列表，
    /// 无法区分「模式没生效」和「确实没匹配上」，只能给用户一句含糊提示。
    ///
    /// 默认返回 `None`：不支持正则的后端不报正则错误（它的正则模式本就不生效）。
    fn validate_query(&self, _query: &crate::query::SearchQuery) -> Option<String> {
        None
    }

    /// 在检索结果上应用过滤器（各后端共用同一套判定语义）。
    fn apply_filter(
        &self,
        hits: Vec<SearchHit>,
        filter: &crate::query::QueryFilter,
    ) -> Vec<SearchHit> {
        if filter.is_empty() {
            return hits;
        }
        hits.into_iter()
            .filter(|h| {
                filter.matches(h, h.modified_at, |path| self.tags_of(path))
            })
            .collect()
    }

    /// 某文档的标签（F-07 标签过滤的数据来源）。
    ///
    /// 默认返回空：Web 预构建索引与 Native 都可覆写。
    fn tags_of(&self, _rel_path: &str) -> Vec<String> {
        Vec::new()
    }

    /// 全部标签及出现次数（F-07 过滤面板的候选项）。
    ///
    /// 默认空：不支持标签的后端不渲染标签过滤控件。
    fn all_tags(&self) -> Vec<(String, usize)> {
        Vec::new()
    }

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

    /// 检索命中的精确位置（PRD §7.1：跳到页 / 单元格 / 段落）。
    ///
    /// 默认 `None`：不支持定位的后端会让结果列表隐藏位置标签，不谎报能力。
    fn locate(&self, _rel_path: &str, _query: &str) -> Option<crate::locate::HitLocation> {
        None
    }

    /// 双链图谱快照（F-10）。默认空图。
    fn graph_snapshot(&self) -> crate::locate::GraphSnapshot {
        crate::locate::GraphSnapshot::default()
    }

    /// 跨格式引用视图（F-06）：文档里 `@文件#片段` 对应的源片段内容。
    ///
    /// Web 只读端返回构建期快照；Desktop 端应实时读取源文件，源更新后
    /// 卡片内容随之刷新（PRD「源更新可刷新 / 标记变更」）。
    /// 默认空：不支持该能力的后端不渲染引用卡片，不谎报能力。
    fn references(&self, _rel_path: &str) -> Vec<crate::refs::ReferenceView> {
        Vec::new()
    }

    /// 可作为引用目标的文档及其选择器候选（F-06 插入引用入口）。
    ///
    /// 默认空：不支持插入引用的后端不渲染「插入引用」入口。
    fn reference_targets(&self) -> Vec<ReferenceTarget> {
        Vec::new()
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

    /// 已安装的编辑器插件清单（UI / 诊断用）。Web 只读端为空。
    fn editor_plugins(&self) -> Vec<crate::plugin::PluginDescriptor> {
        Vec::new()
    }

    /// 是否支持「启动时检测上游编辑器更新」。Web 只读端为 false。
    fn supports_editor_updates(&self) -> bool {
        false
    }

    /// 启动检测得到的更新计划；`None` 表示未检测或不支持。
    fn editor_update_plan(&self) -> Option<crate::editor_updates::UpdatePlan> {
        None
    }

    /// 应用用户对升级的决定（确认 / 跳过 / 稍后），返回受影响的插件数。
    fn decide_editor_update(
        &self,
        _choice: &crate::editor_updates::UpdateChoice,
    ) -> crate::Result<usize> {
        Ok(0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::segment_text;
    use crate::refs::{format_reference_token, selector_candidates};

    /// 未覆写 `reference_targets` 的后端返回空列表（Web 只读端不渲染插入引用入口）。
    #[test]
    fn default_reference_targets_is_empty() {
        let svc = NullService;
        assert!(svc.reference_targets().is_empty());
    }

    /// `ReferenceTarget` 的数据形状：路径、标题与 (序号, 标签, 类型) 三元组。
    #[test]
    fn reference_target_carries_path_title_and_candidates() {
        let target = ReferenceTarget {
            path: "03-台账/模具台账.xlsx".to_string(),
            title: "模具台账".to_string(),
            candidates: vec![
                (1, "台账 · 第 1 行".to_string(), "row".to_string()),
                (2, "台账 · 第 2 行".to_string(), "row".to_string()),
            ],
        };
        assert_eq!(target.path, "03-台账/模具台账.xlsx");
        assert_eq!(target.title, "模具台账");
        assert_eq!(target.candidates.len(), 2);
        assert_eq!(target.candidates[0].0, 1);
        assert_eq!(target.candidates[1].1, "台账 · 第 2 行");
        // DTO 必须可克隆 + 可比较（UI 层按值传递）。
        assert_eq!(target.clone(), target);
    }

    /// 候选序号 → `#N` 选择器 → 引用 token 的完整链路。
    ///
    /// 候选序号取自 `selector_candidates`（去重后列表里的 1-based 位置），
    /// 直接拼成 `#N` 是最稳的写法：标签里可能带空格与中文标点，
    /// 而选择器字符集只认字母数字。
    #[test]
    fn candidates_compose_into_valid_reference_token() {
        let text = "第一段正文。\n\n第二段正文。\n";
        let segments = segment_text(text, crate::format::Format::Markdown);
        let candidates = selector_candidates(&segments);
        assert!(!candidates.is_empty(), "示例文档应有候选片段");
        for (seq, _label, _kind) in &candidates {
            let selector = seq.to_string();
            let token = format_reference_token("03-台账/模具台账.xlsx", &selector)
                .expect("候选序号是合法选择器");
            assert_eq!(token, format!("@03-台账/模具台账.xlsx#{seq}"));
            // token 必须能被解析端认出来，否则引用会静默失效。
            let parsed = crate::refs::parse_references(&token);
            assert_eq!(parsed.len(), 1, "token 应可被解析：{token}");
            assert_eq!(parsed[0].target, "03-台账/模具台账.xlsx");
            assert_eq!(parsed[0].selector, selector);
        }
    }

    /// 非法路径 / 非法选择器不产出 token（宁可失败也不写坏引用）。
    #[test]
    fn invalid_target_or_selector_yields_no_token() {
        assert!(format_reference_token("/绝对路径.md", "1").is_none());
        assert!(format_reference_token("笔记.md", "带 空格").is_none());
        assert!(format_reference_token("笔记.md", "").is_none());
    }
}

//! F-06 进阶项①「编辑器内插入引用入口」的 UI 级回归：
//! - 工具条出现「插入引用」入口；
//! - 面板里的目标文档下拉给出可选文档与片段候选；
//! - 单元格 / 形状 / 段落旁有「引用」按钮；
//! - 选中的候选序号拼成 `@路径#序号`，并**追加**到插入位置原有文本之后。
//!
//! 断言策略：SSR 只渲染初始树，因此「插入」这一动作通过两条路径验证：
//! 1. UI 暴露的纯函数 [`mkv_ui::reference_insert_action`]——按钮处理器走的就是它；
//! 2. 把该动作喂给**真实插件契约** [`mkv_core::plugin::apply_action`]，
//!    再读回视图，确认正文里的引用 token 确实追加在原文之后。

use std::sync::Arc;

use dioxus::prelude::*;
use mkv_core::editor::EditSession;
use mkv_core::format::Format;
use mkv_core::locate::segment_text;
use mkv_core::plugin::{
    self, CellView, EditorPlugin, ParagraphView, PluginAction, PluginDescriptor, PluginDocument,
    PluginView, ShapeView, SheetView, SlideView,
};
use mkv_core::refs::selector_candidates;
use mkv_core::service::KnowledgeService;
use mkv_core::types::{DocumentRecord, LinkRecord, ReferenceTarget, SearchHit};
use mkv_ui::{
    reference_insert_action, EditorHost, InsertTarget, RefInsertError, RefInsertPanel, VaultContext,
};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

// ---------------------------------------------------------------- mock 插件

/// 示例表格视图：一个有内容的单元格 + 一个空格。
fn sample_grid_view() -> PluginView {
    PluginView::Grid {
        sheets: vec![SheetView {
            name: "台账".into(),
            rows: 2,
            cols: 2,
            cells: vec![CellView {
                row: 0,
                col: 1,
                text: "PN-1024".into(),
            }],
        }],
    }
}

fn sample_deck_view() -> PluginView {
    PluginView::Deck {
        slides: vec![SlideView {
            index: 0,
            title: "首页".into(),
            shapes: vec![ShapeView {
                index: 0,
                text: "标题文字".into(),
            }],
        }],
    }
}

fn sample_word_view() -> PluginView {
    PluginView::Word {
        paragraphs: vec![ParagraphView {
            index: 0,
            text: "本周进展".into(),
            style: "Normal".into(),
        }],
        style_options: vec!["Normal".into()],
    }
}

/// 把「设置文本」类动作落到视图上（模拟真实插件的 `apply`）。
fn apply_to_view(view: &mut PluginView, action: &PluginAction) {
    match (view, action) {
        (
            PluginView::Grid { sheets },
            PluginAction::SetCell {
                sheet,
                row,
                col,
                text,
            },
        ) => {
            if let Some(s) = sheets.get_mut(*sheet) {
                if let Some(c) = s.cells.iter_mut().find(|c| c.row == *row && c.col == *col) {
                    c.text = text.clone();
                }
            }
        }
        (
            PluginView::Deck { slides },
            PluginAction::SetShapeText {
                slide,
                shape,
                text,
            },
        ) => {
            if let Some(s) = slides.get_mut(*slide) {
                if let Some(x) = s.shapes.iter_mut().find(|x| x.index == *shape) {
                    x.text = text.clone();
                }
            }
        }
        (
            PluginView::Word { paragraphs, .. },
            PluginAction::SetParagraph { index, text },
        ) => {
            if let Some(p) = paragraphs.iter_mut().find(|p| p.index == *index) {
                p.text = text.clone();
            }
        }
        _ => {}
    }
}

/// 把视图状态编码成插件私有字节（契约规定状态只在 `PluginDocument.state` 里传）。
fn encode_view(view: &PluginView) -> Vec<u8> {
    serde_json::to_vec(view).expect("视图可序列化")
}

fn decode_view(state: &[u8]) -> PluginView {
    serde_json::from_slice(state).expect("视图可反序列化")
}

/// 测试用插件：视图固定，动作会真的落到状态里。
struct MockPlugin {
    id: &'static str,
    initial: PluginView,
}

impl EditorPlugin for MockPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor::builtin(
            self.id,
            "MockCraft",
            "xlsx",
            "local/mock",
            "0000000",
            "0.1.0",
            &[],
        )
    }

    fn supports(&self, _format: Format) -> bool {
        true
    }

    fn open(&self, _bytes: &[u8]) -> mkv_core::Result<PluginDocument> {
        Ok(PluginDocument {
            plugin_id: self.id.to_string(),
            state: encode_view(&self.initial),
        })
    }

    fn view(&self, doc: &PluginDocument) -> mkv_core::Result<PluginView> {
        Ok(decode_view(&doc.state))
    }

    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> mkv_core::Result<()> {
        let mut view = decode_view(&doc.state);
        apply_to_view(&mut view, action);
        doc.state = encode_view(&view);
        Ok(())
    }

    fn serialize(&self, doc: &PluginDocument, _original: &[u8]) -> mkv_core::Result<Vec<u8>> {
        Ok(doc.state.clone())
    }
}

/// 安装 mock 插件（覆盖式注册，重复调用幂等）。
fn install_mock(id: &'static str, initial: PluginView) {
    plugin::install(vec![Arc::new(MockPlugin { id, initial })]).expect("插件应能注册");
}

// ---------------------------------------------------------------- mock 服务

/// 模拟桌面端知识库：能给出可作为引用目标的文档及其片段候选。
struct RefService;

impl KnowledgeService for RefService {
    fn root(&self) -> String {
        "D:/MyVault".into()
    }

    fn document_count(&self) -> usize {
        3
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        Vec::new()
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        Some("台账正文".into())
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn reference_targets(&self) -> Vec<ReferenceTarget> {
        // 候选序号直接来自 core 的 `selector_candidates`：UI 不另造一套编号。
        let segments = segment_text("第一段。\n\n第二段。\n", Format::Markdown);
        let candidates = selector_candidates(&segments);
        vec![
            ReferenceTarget {
                path: "03-台账/模具台账.xlsx".into(),
                title: "模具台账".into(),
                candidates: candidates.clone(),
            },
            ReferenceTarget {
                path: "01-笔记/说明.md".into(),
                title: "说明笔记".into(),
                candidates,
            },
        ]
    }
}

/// 唯一目标文档没有任何片段候选。
struct EmptyRefService;

impl KnowledgeService for EmptyRefService {
    fn root(&self) -> String {
        "D:/MyVault".into()
    }

    fn document_count(&self) -> usize {
        1
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        Vec::new()
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        Some("空文档".into())
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn reference_targets(&self) -> Vec<ReferenceTarget> {
        vec![ReferenceTarget {
            path: "04-空文档.md".into(),
            title: "空文档".into(),
            candidates: Vec::new(),
        }]
    }
}

/// 后端不支持插入引用（`reference_targets()` 为默认空实现）。
struct NoRefService;

impl KnowledgeService for NoRefService {
    fn root(&self) -> String {
        String::new()
    }

    fn document_count(&self) -> usize {
        0
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        Vec::new()
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

fn ctx() -> VaultContext {
    VaultContext::with_service(Box::new(RefService))
}

fn empty_ctx() -> VaultContext {
    VaultContext::with_service(Box::new(EmptyRefService))
}

fn session_for(id: &'static str, view: &PluginView) -> EditSession {
    EditSession {
        rel_path: "05-草稿/周报.docx".into(),
        original: Vec::new(),
        plugin_id: id.to_string(),
        document: PluginDocument {
            plugin_id: id.to_string(),
            state: encode_view(view),
        },
    }
}

// ---------------------------------------------------------------- 工具条入口

#[component]
fn EditorHostProbe() -> Element {
    install_mock("mockcraft", sample_grid_view());
    let ctx = ctx();
    rsx! { EditorHost { ctx, session: session_for("mockcraft", &sample_grid_view()) } }
}

#[test]
fn editor_toolbar_offers_insert_reference_entry() {
    let html = render(EditorHostProbe);
    assert!(html.contains("插入引用"), "工具条应提供「插入引用」入口：{html}");
    assert!(
        html.contains("mkv-editor-actions"),
        "入口应挂在工具条动作区：{html}"
    );
}

#[test]
fn every_cell_has_a_reference_pick_button() {
    let html = render(EditorHostProbe);
    assert!(
        html.contains("mkv-ref-pick"),
        "单元格旁应有「引用」按钮：{html}"
    );
    assert!(
        html.contains("PN-1024"),
        "单元格正文应照常渲染：{html}"
    );
}

#[component]
fn WordHostProbe() -> Element {
    install_mock("mockword", sample_word_view());
    let ctx = ctx();
    rsx! { EditorHost { ctx, session: session_for("mockword", &sample_word_view()) } }
}

#[component]
fn DeckHostProbe() -> Element {
    install_mock("mockdeck", sample_deck_view());
    let ctx = ctx();
    rsx! { EditorHost { ctx, session: session_for("mockdeck", &sample_deck_view()) } }
}

#[test]
fn paragraph_and_shape_also_offer_reference_pick() {
    let html = render(WordHostProbe);
    assert!(
        html.contains("mkv-ref-pick") && html.contains("本周进展"),
        "段落旁应有「引用」按钮：{html}"
    );

    let html2 = render(DeckHostProbe);
    assert!(
        html2.contains("mkv-ref-pick") && html2.contains("标题文字"),
        "形状旁应有「引用」按钮：{html2}"
    );
}

// ---------------------------------------------------------------- 插入面板

#[component]
fn PanelProbe() -> Element {
    let ctx = ctx();
    let edit = use_signal(|| session_for("mockcraft", &sample_grid_view()));
    let target = use_signal(|| Some(InsertTarget::Word { paragraph: 0 }));
    rsx! { RefInsertPanel { ctx, edit, target } }
}

#[test]
fn insert_panel_lists_target_documents_and_candidates() {
    let html = render(PanelProbe);
    assert!(html.contains("mkv-ref-insert"), "应渲染插入引用面板：{html}");
    assert!(
        html.contains("模具台账") && html.contains("说明笔记"),
        "目标文档下拉应给出可选文档：{html}"
    );
    assert!(
        html.contains("03-台账/模具台账.xlsx"),
        "下拉项的值应是引用 token 里用的相对路径：{html}"
    );
    assert!(
        html.contains("第 1 段"),
        "片段下拉应展示候选标签：{html}"
    );
    assert!(
        !html.contains("尚未选择"),
        "已选中插入位置时不应再提示未选择：{html}"
    );
    assert!(
        html.contains("以 @路径#片段 的形式追加到所选位置末尾"),
        "应说明追加语义：{html}"
    );
}

#[component]
fn EmptyPanelProbe() -> Element {
    let ctx = empty_ctx();
    let edit = use_signal(|| session_for("mockcraft", &sample_grid_view()));
    let target = use_signal(|| Some(InsertTarget::Word { paragraph: 0 }));
    rsx! { RefInsertPanel { ctx, edit, target } }
}

#[test]
fn panel_disables_selector_when_target_has_no_candidates() {
    let html = render(EmptyPanelProbe);
    assert!(
        html.contains("该文档没有可选片段"),
        "没有候选片段时必须明确说明：{html}"
    );
    assert!(
        html.contains("disabled"),
        "没有候选片段时片段下拉应禁用：{html}"
    );
}

#[component]
fn NoRefPanelProbe() -> Element {
    let ctx = VaultContext::with_service(Box::new(NoRefService));
    let edit = use_signal(|| session_for("mockcraft", &sample_grid_view()));
    let target = use_signal(|| Some(InsertTarget::Word { paragraph: 0 }));
    rsx! { RefInsertPanel { ctx, edit, target } }
}

#[test]
fn backend_without_reference_targets_says_so_instead_of_empty_dropdown() {
    let html = render(NoRefPanelProbe);
    assert!(
        html.contains("当前后端没有可用的引用目标"),
        "不支持插入引用的后端应如实说明：{html}"
    );
    assert!(
        !html.contains("<select"),
        "没有目标时不应给出空下拉：{html}"
    );
}

// ---------------------------------------------------------------- token + 追加

#[test]
fn insert_appends_token_after_existing_text() {
    let view = sample_grid_view();
    let target = InsertTarget::Grid {
        sheet: 0,
        row: 0,
        col: 1,
    };
    let action =
        reference_insert_action(&view, &target, "03-台账/模具台账.xlsx", "2").expect("应能生成动作");
    match action {
        PluginAction::SetCell {
            sheet,
            row,
            col,
            text,
        } => {
            assert_eq!((sheet, row, col), (0, 0, 1), "动作应落在原来的位置");
            assert!(text.starts_with("PN-1024"), "原有文本必须保留在前缀：{text}");
            assert!(
                text.ends_with("@03-台账/模具台账.xlsx#2"),
                "引用 token 应追加到末尾：{text}"
            );
            assert_eq!(text, "PN-1024@03-台账/模具台账.xlsx#2");
        }
        other => panic!("应复用既有的 SetCell 动作，不新增变体：{other:?}"),
    }
}

#[test]
fn insert_appends_token_for_paragraph_and_shape() {
    let action = reference_insert_action(
        &sample_word_view(),
        &InsertTarget::Word { paragraph: 0 },
        "03-台账/模具台账.xlsx",
        "1",
    )
    .expect("合法位置应能生成动作");
    match action {
        PluginAction::SetParagraph { index, text } => {
            assert_eq!(index, 0);
            assert_eq!(text, "本周进展@03-台账/模具台账.xlsx#1");
        }
        other => panic!("段落应复用 SetParagraph 动作：{other:?}"),
    }

    let action = reference_insert_action(
        &sample_deck_view(),
        &InsertTarget::Deck {
            slide: 0,
            shape: 0,
        },
        "03-台账/模具台账.xlsx",
        "1",
    )
    .expect("合法位置应能生成动作");
    match action {
        PluginAction::SetShapeText {
            slide,
            shape,
            text,
        } => {
            assert_eq!((slide, shape), (0, 0));
            assert_eq!(text, "标题文字@03-台账/模具台账.xlsx#1");
        }
        other => panic!("形状应复用 SetShapeText 动作：{other:?}"),
    }
}

#[test]
fn invalid_selector_is_rejected_before_touching_content() {
    let view = sample_grid_view();
    let target = InsertTarget::Grid {
        sheet: 0,
        row: 0,
        col: 1,
    };
    // 空选择器 / 带空格的选择器都不是合法选择器：宁可失败也不写坏引用。
    assert_eq!(
        reference_insert_action(&view, &target, "03-台账/模具台账.xlsx", ""),
        Err(RefInsertError::InvalidSelector)
    );
    assert_eq!(
        reference_insert_action(&view, &target, "03-台账/模具台账.xlsx", "第 2 行"),
        Err(RefInsertError::InvalidSelector)
    );
    // 绝对路径不是合法目标路径。
    assert_eq!(
        reference_insert_action(&view, &target, "/绝对路径.md", "1"),
        Err(RefInsertError::InvalidSelector)
    );
    // 位置不存在时报「位置失效」，而不是把引用插到别处。
    assert_eq!(
        reference_insert_action(
            &view,
            &InsertTarget::Grid {
                sheet: 0,
                row: 9,
                col: 9
            },
            "03-台账/模具台账.xlsx",
            "1"
        ),
        Err(RefInsertError::MissingPosition)
    );
}

#[test]
fn candidate_sequence_from_core_composes_into_token() {
    // 候选序号来自 core 的 `selector_candidates`，直接当 `#N` 选择器最稳。
    let segments = segment_text("第一段。\n\n第二段。\n", Format::Markdown);
    let candidates = selector_candidates(&segments);
    assert!(!candidates.is_empty());
    let (seq, _label, _kind) = &candidates[0];
    let action = reference_insert_action(
        &sample_grid_view(),
        &InsertTarget::Grid {
            sheet: 0,
            row: 0,
            col: 1,
        },
        "03-台账/模具台账.xlsx",
        &seq.to_string(),
    )
    .expect("候选序号是合法选择器");
    match action {
        PluginAction::SetCell { text, .. } => {
            assert_eq!(text, format!("PN-1024@03-台账/模具台账.xlsx#{seq}"));
        }
        other => panic!("应复用 SetCell 动作：{other:?}"),
    }
}

/// 端到端：动作交给**真实插件契约**执行后，正文里确实多出引用 token。
#[test]
fn insert_action_writes_reference_into_plugin_state() {
    install_mock("mockcraft", sample_grid_view());
    let mut session = session_for("mockcraft", &sample_grid_view());
    let view = plugin::view(&session).expect("mock 插件已安装");
    let action = reference_insert_action(
        &view,
        &InsertTarget::Grid {
            sheet: 0,
            row: 0,
            col: 1,
        },
        "03-台账/模具台账.xlsx",
        "2",
    )
    .expect("应能生成动作");
    plugin::apply_action(&mut session, &action).expect("插件应能执行动作");

    let after = plugin::view(&session).expect("mock 插件已安装");
    let PluginView::Grid { sheets } = after else {
        panic!("仍是表格视图");
    };
    let cell = sheets[0]
        .cells
        .iter()
        .find(|c| c.row == 0 && c.col == 1)
        .expect("单元格仍在");
    assert_eq!(cell.text, "PN-1024@03-台账/模具台账.xlsx#2");
}

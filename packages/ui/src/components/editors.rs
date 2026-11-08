//! 插件化编辑器面板与上游升级提示。
//!
//! # 与 v0.1 的区别
//!
//! 旧版直接把 `EditDocument::{Workbook, Deck, Word}` 的三个模型塞进 UI，
//! 换个实现就要改 UI。现在 UI 只认 `mkv_core::plugin` 的稳定 DTO：
//!
//! ```text
//!   plugin::view(&session)         → PluginView（Grid / Deck / Word）
//!            ↓ 渲染
//!   用户操作 → PluginAction        → plugin::apply_action(&mut session, ..)
//!            ↓ 保存
//!   KnowledgeService::save_editor  → 插件 serialize() → 原生文件 + 增量索引
//! ```
//!
//! 因此新增 / 替换编辑器插件都不需要改本文件；Web 只读端未安装插件时
//! `view()` 返回错误，面板自然不渲染，也不会出现「假编辑入口」。
//!
//! 子组件化还有一个工程原因：Dioxus 的事件闭包要求 `'static`，
//! 不能捕获 `rsx!` 循环里的临时引用，所以把每行 / 每页 / 每段做成
//! 接收 owned props 的子组件。

use dioxus::prelude::*;
use mkv_core::editor::EditSession;
use mkv_core::editor_updates::{UpdateChoice, UpdatePlan};
use mkv_core::plugin::{
    self, CellView, ParagraphView, PluginAction, PluginView, ShapeView, SlideView,
};
use mkv_core::refs::format_reference_token;
use mkv_core::types::ReferenceTarget;

use crate::VaultContext;

/// 把动作交给当前会话的插件；失败只记日志，保持原状态。
fn apply(edit: &mut Signal<EditSession>, action: PluginAction) {
    let mut guard = edit.write();
    if let Err(e) = plugin::apply_action(&mut *guard, &action) {
        eprintln!("[mkv-editor] 插件动作失败: {e}");
    }
}

// ------------------------------------------------- 插入引用（F-06 进阶项①）

/// 当前选中的「插入引用」位置。
///
/// 索引语义直接沿用既有插件动作，UI 不另造一套：
/// - 表格：工作表序号 + 行 + 列 → [`PluginAction::SetCell`]；
/// - 演示：页序号 + 页内形状序号 → [`PluginAction::SetShapeText`]；
/// - 文档：段落序号 → [`PluginAction::SetParagraph`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertTarget {
    /// 表格单元格。
    Grid { sheet: usize, row: usize, col: usize },
    /// 幻灯片里的形状。
    Deck { slide: usize, shape: usize },
    /// 文档段落。
    Word { paragraph: usize },
}

impl InsertTarget {
    /// 位置的人话描述（状态栏提示用）。
    pub fn describe(&self) -> String {
        match *self {
            InsertTarget::Grid { sheet, row, col } => {
                format!("第 {} 个工作表 · 单元格 R{}C{}", sheet + 1, row + 1, col + 1)
            }
            InsertTarget::Deck { slide, shape } => {
                format!("第 {} 页 · 第 {} 个形状", slide + 1, shape + 1)
            }
            InsertTarget::Word { paragraph } => format!("第 {} 段", paragraph + 1),
        }
    }
}

/// 「插入引用」失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefInsertError {
    /// 尚未选择插入位置。
    NoPosition,
    /// 目标路径 / 选择器不合法，拼不出解析端认识的 token。
    InvalidSelector,
    /// 当前视图里已经找不到该插入位置（例如内容已被改过）。
    MissingPosition,
}

/// 从当前视图里取出插入位置**已有**的文本。
fn position_text(view: &PluginView, target: &InsertTarget) -> Option<String> {
    match *target {
        InsertTarget::Grid { sheet, row, col } => {
            let sheets = match view {
                PluginView::Grid { sheets } => sheets,
                _ => return None,
            };
            sheets.get(sheet)?
                .cells
                .iter()
                .find(|c| c.row == row && c.col == col)
                .map(|c| c.text.clone())
        }
        InsertTarget::Deck { slide, shape } => {
            let slides = match view {
                PluginView::Deck { slides } => slides,
                _ => return None,
            };
            let page = slides.iter().find(|s| s.index == slide)?;
            page.shapes
                .iter()
                .find(|x| x.index == shape)
                .map(|x| x.text.clone())
        }
        InsertTarget::Word { paragraph } => {
            let paragraphs = match view {
                PluginView::Word { paragraphs, .. } => paragraphs,
                _ => return None,
            };
            paragraphs
                .iter()
                .find(|p| p.index == paragraph)
                .map(|p| p.text.clone())
        }
    }
}

/// 计算「插入引用」要提交给插件的动作。
///
/// 固定三步：
/// 1. [`format_reference_token`] 把「目标相对路径 + 选择器」拼成 `@路径#选择器`；
/// 2. 从当前视图取出插入位置**已有**的文本；
/// 3. 把 token **追加**到已有文本末尾，再复用既有的
///    [`PluginAction::SetCell`] / [`SetShapeText`] / [`SetParagraph`] 提交。
///
/// 刻意不新增 `PluginAction` 变体：插件契约 v1 已冻结
/// （补充约束 30），设置文本的三种动作已经够用。
pub fn reference_insert_action(
    view: &PluginView,
    target: &InsertTarget,
    target_path: &str,
    selector: &str,
) -> Result<PluginAction, RefInsertError> {
    let token =
        format_reference_token(target_path, selector).ok_or(RefInsertError::InvalidSelector)?;
    let existing = position_text(view, target).ok_or(RefInsertError::MissingPosition)?;
    let new_text = format!("{existing}{token}");
    Ok(match *target {
        InsertTarget::Grid { sheet, row, col } => PluginAction::SetCell {
            sheet,
            row,
            col,
            text: new_text,
        },
        InsertTarget::Deck { slide, shape } => PluginAction::SetShapeText {
            slide,
            shape,
            text: new_text,
        },
        InsertTarget::Word { paragraph } => PluginAction::SetParagraph {
            index: paragraph,
            text: new_text,
        },
    })
}

/// 编辑器宿主：按插件声明的视图形态挂载通用面板。
#[component]
pub fn EditorHost(ctx: VaultContext, session: EditSession) -> Element {
    let edit = use_signal(|| session);
    // 三个面板共用同一个「当前插入位置」信号。
    let insert_target = use_signal(|| None::<InsertTarget>);
    // 「插入引用」面板默认收起：目标列表要读全部文档原文，只在用户需要时求值。
    let ref_open = use_signal(|| false);
    let current = edit.read().clone();
    let view = plugin::view(&current).ok();
    let editor_name = plugin::registry()
        .read()
        .ok()
        .and_then(|r| r.get(&current.plugin_id))
        .map(|p| p.descriptor().display_name)
        .unwrap_or_else(|| "插件编辑器".to_string());
    let rel_path = current.rel_path.clone();
    let ref_open_value = *ref_open.read();

    rsx! {
        section { class: "mkv-editor",
            div { class: "mkv-editor-toolbar",
                span { class: "mkv-editor-name", "{editor_name}" }
                span { class: "mkv-muted mkv-editor-path", "{rel_path}" }
                div { class: "mkv-editor-actions",
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        title: "在正文里插入跨格式引用",
                        onclick: move |_| {
                            let currently = *ref_open.peek();
                            let mut o = ref_open;
                            o.set(!currently);
                        },
                        "插入引用"
                    }
                    button {
                        class: "mkv-btn mkv-btn-primary",
                        onclick: move |_| {
                            let mut status = ctx.status;
                            let current = edit.read().clone();
                            match ctx.service.read().save_editor(&current) {
                                Ok(()) => status.set("已保存原生文件并更新索引".to_string()),
                                Err(e) => status.set(format!("保存失败：{e}")),
                            }
                        },
                        "保存"
                    }
                    button {
                        class: "mkv-btn mkv-btn-ghost",
                        onclick: move |_| {
                            let mut session = ctx.edit_session;
                            let mut status = ctx.status;
                            session.set(None);
                            status.set("已退出编辑".to_string());
                        },
                        "退出编辑"
                    }
                }
            }
            if ref_open_value {
                RefInsertPanel { ctx, edit, target: insert_target }
            }
            div { class: "mkv-editor-body",
                match view {
                    Some(PluginView::Grid { .. }) => rsx! { GridCraftPanel { ctx, edit, target: insert_target } },
                    Some(PluginView::Deck { .. }) => rsx! { DeckCraftPanel { ctx, edit, target: insert_target } },
                    Some(PluginView::Word { .. }) => rsx! { WordCraftPanel { ctx, edit, target: insert_target } },
                    None => rsx! {
                        div { class: "mkv-muted", "编辑器插件未安装或视图不可用" }
                    },
                }
            }
        }
    }
}

/// 「插入引用」面板：选目标文档 → 选片段 → 把 `@路径#选择器` 追加到插入位置。
///
/// **代价**：目标列表来自 [`mkv_core::service::KnowledgeService::reference_targets`]，
/// native 实现会读取全部文档原文并切分片段，因此本面板**只在用户打开时挂载**
/// （由 [`EditorHost`] 条件渲染），绝不能放进渲染热路径。
#[component]
pub fn RefInsertPanel(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
) -> Element {
    // 只求值一次：面板挂载时取一次目标列表，之后复用。
    let targets: Vec<ReferenceTarget> = use_hook(|| ctx.service.read().reference_targets());
    let selected_path = use_signal(|| targets.first().map(|t| t.path.clone()).unwrap_or_default());
    let selected_selector = use_signal(|| String::new());

    let current_path = selected_path.read().clone();
    let current_selector = selected_selector.read().clone();
    let position = target.read().clone();

    let current = targets.iter().find(|t| t.path == current_path);
    let candidates: Vec<(usize, String)> = current
        .map(|t| {
            t.candidates
                .iter()
                .map(|(seq, label, _kind)| (*seq, label.clone()))
                .collect()
        })
        .unwrap_or_default();
    let position_text = match position {
        Some(p) => p.describe(),
        None => "尚未选择（在单元格 / 形状 / 段落旁点「引用」）".to_string(),
    };

    // 后端不支持插入引用（`reference_targets()` 默认为空）时，
    // 面板不给出无用的下拉，而是如实说明。
    if targets.is_empty() {
        return rsx! {
            div { class: "mkv-ref-insert",
                div { class: "mkv-ref-insert-row",
                    span { class: "mkv-muted", "当前后端没有可用的引用目标" }
                }
            }
        };
    }

    rsx! {
        div { class: "mkv-ref-insert",
            div { class: "mkv-ref-insert-row",
                span { class: "mkv-muted", "插入位置" }
                span { class: "mkv-ref-insert-pos", "{position_text}" }
            }
            div { class: "mkv-ref-insert-row",
                span { class: "mkv-muted", "目标文档" }
                select {
                    onchange: move |ev| {
                        let value = ev.value();
                        let mut p = selected_path;
                        p.set(value);
                        // 换文档后候选序号全部失效，选择器重置。
                        let mut s = selected_selector;
                        s.set(String::new());
                    },
                    for t in targets.iter() {
                        option {
                            value: "{t.path}",
                            selected: t.path == current_path,
                            "{t.title}"
                        }
                    }
                }
            }
            div { class: "mkv-ref-insert-row",
                span { class: "mkv-muted", "目标片段" }
                select {
                    disabled: candidates.is_empty(),
                    onchange: move |ev| {
                        let value = ev.value();
                        let mut s = selected_selector;
                        s.set(value);
                    },
                    for (seq, label) in candidates.iter() {
                        option {
                            value: "{seq}",
                            selected: seq.to_string() == current_selector,
                            "{seq}. {label}"
                        }
                    }
                }
                if candidates.is_empty() {
                    span { class: "mkv-muted", "该文档没有可选片段" }
                }
            }
            div { class: "mkv-ref-insert-row",
                button {
                    class: "mkv-btn mkv-btn-sm mkv-btn-primary",
                    onclick: move |_| {
                        let mut status = ctx.status;
                        let mut edit = edit;
                        let Some(pos) = *target.read() else {
                            status.set("请先选择插入位置".to_string());
                            return;
                        };
                        let path = selected_path.read().clone();
                        let selector = selected_selector.read().clone();
                        if path.is_empty() {
                            status.set("请先选择目标文档".to_string());
                            return;
                        }
                        let session = edit.read().clone();
                        let Some(view) = plugin::view(&session).ok() else {
                            status.set("编辑器视图不可用".to_string());
                            return;
                        };
                        match reference_insert_action(&view, &pos, &path, &selector) {
                            Ok(action) => {
                                apply(&mut edit, action);
                                status.set(format!("已插入引用 @{path}#{selector}"));
                            }
                            Err(RefInsertError::NoPosition) => {
                                status.set("请先选择插入位置".to_string());
                            }
                            Err(RefInsertError::InvalidSelector) => {
                                status.set("选择器不合法".to_string());
                            }
                            Err(RefInsertError::MissingPosition) => {
                                status.set("插入位置已失效，请重新选择".to_string());
                            }
                        }
                    },
                    "插入"
                }
                span { class: "mkv-muted", "以 @路径#片段 的形式追加到所选位置末尾" }
            }
        }
    }
}

// ---------------------------------------------------------------- Grid 视图

#[component]
fn GridCell(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
    sheet: usize,
    cell: CellView,
) -> Element {
    let mut edit = edit;
    let row = cell.row;
    let col = cell.col;
    let text = cell.text;
    rsx! {
        td { class: "mkv-grid-cell",
            div { class: "mkv-grid-cell-tools",
                input {
                    value: "{text}",
                    onchange: move |ev| apply(&mut edit, PluginAction::SetCell {
                        sheet, row, col, text: ev.value(),
                    }),
                }
                button {
                    class: "mkv-btn mkv-btn-sm mkv-ref-pick",
                    title: "在这个单元格插入引用",
                    onclick: move |_| {
                        let mut t = target;
                        t.set(Some(InsertTarget::Grid { sheet, row, col }));
                        let mut s = ctx.status;
                        s.set(format!(
                            "已选择插入位置：第 {} 个工作表 · R{}C{}",
                            sheet + 1,
                            row + 1,
                            col + 1
                        ));
                    },
                    "引用"
                }
            }
        }
    }
}

/// 表格视图通用面板（由 `PluginView::Grid` 驱动，不依赖具体插件模型）。
#[component]
pub fn GridCraftPanel(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
) -> Element {
    let mut edit = edit;
    let session = edit.read().clone();
    let Some(PluginView::Grid { sheets }) = plugin::view(&session).ok() else {
        return rsx! { div { class: "mkv-muted", "该插件未提供表格视图" } };
    };
    let sheet_idx = use_signal(|| 0usize);
    let current_sheet = *sheet_idx.peek();
    let Some(sheet) = sheets.get(current_sheet).cloned() else {
        return rsx! { div { class: "mkv-muted", "工作簿没有工作表" } };
    };

    // 稀疏单元格 → 二维显示矩阵（补齐空格，保证行列增删后视觉稳定）
    let rows = sheet.rows.max(1);
    let cols = sheet.cols.max(1);
    let mut grid: Vec<Vec<CellView>> = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| CellView {
                    row: r,
                    col: c,
                    text: String::new(),
                })
                .collect()
        })
        .collect();
    for c in &sheet.cells {
        if c.row < rows && c.col < cols {
            grid[c.row][c.col] = c.clone();
        }
    }
    let tab_names: Vec<(usize, String)> = sheets
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.name.clone()))
        .collect();

    rsx! {
        div { class: "mkv-grid",
            div { class: "mkv-grid-tabs",
                for (i, name) in tab_names.into_iter() {
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let mut s = sheet_idx;
                            s.set(i);
                        },
                        "{name}"
                    }
                }
            }
            div { class: "mkv-grid-toolbar",
                span { class: "mkv-muted", "工作表 {current_sheet + 1} · {rows} 行 / {cols} 列" }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::InsertRow { sheet: current_sheet, at: 0 }),
                    "在首行插入"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::RemoveRow { sheet: current_sheet, at: 0 }),
                    "删除首行"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::InsertColumn { sheet: current_sheet, at: 0 }),
                    "在首列插入"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::RemoveColumn { sheet: current_sheet, at: 0 }),
                    "删除首列"
                }
            }
            div { class: "mkv-grid-scroll",
                table { class: "mkv-grid-table",
                    tbody {
                        for line in grid.iter() {
                            tr {
                                td { class: "mkv-grid-rownum", "{line[0].row + 1}" }
                                for cell in line.iter() {
                                    GridCell {
                                        ctx,
                                        edit,
                                        target,
                                        sheet: current_sheet,
                                        cell: cell.clone(),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Deck 视图

#[component]
fn ShapeEditor(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
    slide: usize,
    shape: ShapeView,
) -> Element {
    let mut edit = edit;
    let index = shape.index;
    let text = shape.text;
    rsx! {
        div { class: "mkv-word-shape",
            textarea {
                class: "mkv-word-text",
                value: "{text}",
                onchange: move |ev| apply(&mut edit, PluginAction::SetShapeText {
                    slide, shape: index, text: ev.value(),
                }),
            }
            button {
                class: "mkv-btn mkv-btn-sm mkv-ref-pick",
                title: "在这个形状插入引用",
                onclick: move |_| {
                    let mut t = target;
                    t.set(Some(InsertTarget::Deck { slide, shape: index }));
                    let mut s = ctx.status;
                    s.set(format!("已选择插入位置：第 {} 页 · 第 {} 个形状", slide + 1, index + 1));
                },
                "引用"
            }
        }
    }
}

#[component]
fn SlideEditor(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
    slide: SlideView,
) -> Element {
    let mut edit = edit;
    let index = slide.index;
    let title = slide.title;
    let shapes = slide.shapes;
    rsx! {
        div { class: "mkv-word-para",
            div { class: "mkv-word-para-tools",
                span { "第 {index + 1} 页 · {title}" }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::MoveSlide {
                        from: index, to: index.saturating_sub(1),
                    }),
                    "↑"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::MoveSlide {
                        from: index, to: index + 1,
                    }),
                    "↓"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::RemoveSlide { index }),
                    "删除页"
                }
            }
            for shape in shapes.into_iter() {
                ShapeEditor { ctx, edit, target, slide: index, shape }
            }
        }
    }
}

/// 演示文稿视图通用面板。
#[component]
pub fn DeckCraftPanel(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
) -> Element {
    let session = edit.read().clone();
    let Some(PluginView::Deck { slides }) = plugin::view(&session).ok() else {
        return rsx! { div { class: "mkv-muted", "该插件未提供演示视图" } };
    };
    let count = slides.len();
    rsx! {
        div { class: "mkv-word",
            div { class: "mkv-word-toolbar",
                span { class: "mkv-muted", "{count} 页 · 形状文本可直接编辑" }
            }
            div { class: "mkv-word-body",
                for slide in slides.into_iter() {
                    SlideEditor { ctx, edit, target, slide }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Word 视图

#[component]
fn ParagraphEditor(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
    paragraph: ParagraphView,
    style_options: Vec<String>,
) -> Element {
    let mut edit = edit;
    let index = paragraph.index;
    let text = paragraph.text;
    let style = paragraph.style;
    rsx! {
        div { class: "mkv-word-para",
            div { class: "mkv-word-para-tools",
                span { class: "mkv-muted", "第 {index + 1} 段" }
                select {
                    onchange: move |ev| apply(&mut edit, PluginAction::SetParagraphStyle {
                        index, style: ev.value(),
                    }),
                    for opt in style_options.iter() {
                        option { value: "{opt}", selected: *opt == style, "{opt}" }
                    }
                }
                button {
                    class: "mkv-btn mkv-btn-sm mkv-ref-pick",
                    title: "在这一段插入引用",
                    onclick: move |_| {
                        let mut t = target;
                        t.set(Some(InsertTarget::Word { paragraph: index }));
                        let mut s = ctx.status;
                        s.set(format!("已选择插入位置：第 {} 段", index + 1));
                    },
                    "引用"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::RemoveParagraph { index }),
                    "删除"
                }
            }
            textarea {
                class: "mkv-word-text",
                value: "{text}",
                onchange: move |ev| apply(&mut edit, PluginAction::SetParagraph {
                    index, text: ev.value(),
                }),
            }
        }
    }
}

/// 文档视图通用面板；样式选项由插件声明，UI 不硬编码样式表。
#[component]
pub fn WordCraftPanel(
    ctx: VaultContext,
    edit: Signal<EditSession>,
    target: Signal<Option<InsertTarget>>,
) -> Element {
    let mut edit = edit;
    let session = edit.read().clone();
    let Some(PluginView::Word { paragraphs, style_options }) = plugin::view(&session).ok() else {
        return rsx! { div { class: "mkv-muted", "该插件未提供文档视图" } };
    };
    let at = paragraphs.len();
    let default_style = style_options.first().cloned().unwrap_or_else(|| "Normal".into());
    let count = paragraphs.len();
    rsx! {
        div { class: "mkv-word",
            div { class: "mkv-word-toolbar",
                span { class: "mkv-muted", "{count} 段" }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| apply(&mut edit, PluginAction::InsertParagraph {
                        at, style: default_style.clone(),
                    }),
                    "新增段落"
                }
            }
            div { class: "mkv-word-body",
                for paragraph in paragraphs.into_iter() {
                    ParagraphEditor {
                        ctx,
                        edit,
                        target,
                        paragraph,
                        style_options: style_options.clone(),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- 启动升级提示

/// 应用启动时检测三个上游 Craft 项目是否有新提交，由用户决定是否升级。
///
/// Web 只读端 `supports_editor_updates()` 为 false，不发起任何网络探测。
/// 探测失败 / 离线时得到空计划，横幅不出现，不打断主流程。
#[component]
pub fn EditorUpdateBanner(ctx: VaultContext) -> Element {
    let dismissed = use_signal(|| false);
    let plan = use_signal(|| None::<Option<UpdatePlan>>);

    use_future(move || async move {
        let mut signal = plan;
        if ctx.service.read().supports_editor_updates() {
            let detected = ctx.service.read().editor_update_plan();
            signal.set(Some(detected));
        }
    });

    if !ctx.service.read().supports_editor_updates() || *dismissed.peek() {
        return rsx! {};
    }
    let Some(Some(p)) = plan.read().clone() else {
        return rsx! {};
    };
    if !p.has_updates() {
        return rsx! {};
    }
    let ids = p.actionable_ids();
    let ids_upgrade = ids.clone();
    let ids_skip = ids.clone();
    let count = ids.len();
    let names = ids.join(" / ");

    rsx! {
        div { class: "mkv-update-banner",
            span { class: "mkv-update-text",
                "检测到 {count} 个编辑器插件的上游仓库有新提交（{names}）"
            }
            button {
                class: "mkv-btn mkv-btn-sm mkv-btn-primary",
                onclick: move |_| {
                    let mut status = ctx.status;
                    let mut d = dismissed;
                    match ctx.service.read().decide_editor_update(&UpdateChoice::Upgrade { ids: ids_upgrade.clone() }) {
                        Ok(n) => status.set(format!(
                            "已确认升级 {n} 个插件；运行 mkv editors upgrade 完成插件重建后重启生效"
                        )),
                        Err(e) => status.set(format!("升级确认失败：{e}")),
                    }
                    d.set(true);
                },
                "更新"
            }
            button {
                class: "mkv-btn mkv-btn-sm",
                onclick: move |_| {
                    let mut status = ctx.status;
                    let mut d = dismissed;
                    match ctx.service.read().decide_editor_update(&UpdateChoice::Skip { ids: ids_skip.clone() }) {
                        Ok(n) => status.set(format!("已跳过 {n} 个上游版本")),
                        Err(e) => status.set(format!("跳过失败：{e}")),
                    }
                    d.set(true);
                },
                "跳过此版本"
            }
            button {
                class: "mkv-btn mkv-btn-sm mkv-btn-ghost",
                onclick: move |_| {
                    let mut status = ctx.status;
                    let mut d = dismissed;
                    let _ = ctx.service.read().decide_editor_update(&UpdateChoice::Defer);
                    status.set("已暂缓插件升级".to_string());
                    d.set(true);
                },
                "稍后"
            }
        }
    }
}

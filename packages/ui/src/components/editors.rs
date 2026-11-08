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

use crate::VaultContext;

/// 把动作交给当前会话的插件；失败只记日志，保持原状态。
fn apply(edit: &mut Signal<EditSession>, action: PluginAction) {
    let mut guard = edit.write();
    if let Err(e) = plugin::apply_action(&mut *guard, &action) {
        eprintln!("[mkv-editor] 插件动作失败: {e}");
    }
}

/// 编辑器宿主：按插件声明的视图形态挂载通用面板。
#[component]
pub fn EditorHost(ctx: VaultContext, session: EditSession) -> Element {
    let edit = use_signal(|| session);
    let current = edit.read().clone();
    let view = plugin::view(&current).ok();
    let editor_name = plugin::registry()
        .read()
        .ok()
        .and_then(|r| r.get(&current.plugin_id))
        .map(|p| p.descriptor().display_name)
        .unwrap_or_else(|| "插件编辑器".to_string());
    let rel_path = current.rel_path.clone();

    rsx! {
        section { class: "mkv-editor",
            div { class: "mkv-editor-toolbar",
                span { class: "mkv-editor-name", "{editor_name}" }
                span { class: "mkv-muted mkv-editor-path", "{rel_path}" }
                div { class: "mkv-editor-actions",
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
            div { class: "mkv-editor-body",
                match view {
                    Some(PluginView::Grid { .. }) => rsx! { GridCraftPanel { edit } },
                    Some(PluginView::Deck { .. }) => rsx! { DeckCraftPanel { edit } },
                    Some(PluginView::Word { .. }) => rsx! { WordCraftPanel { edit } },
                    None => rsx! {
                        div { class: "mkv-muted", "编辑器插件未安装或视图不可用" }
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Grid 视图

#[component]
fn GridCell(edit: Signal<EditSession>, sheet: usize, cell: CellView) -> Element {
    let mut edit = edit;
    let row = cell.row;
    let col = cell.col;
    let text = cell.text;
    rsx! {
        td { class: "mkv-grid-cell",
            input {
                value: "{text}",
                onchange: move |ev| apply(&mut edit, PluginAction::SetCell {
                    sheet, row, col, text: ev.value(),
                }),
            }
        }
    }
}

/// 表格视图通用面板（由 `PluginView::Grid` 驱动，不依赖具体插件模型）。
#[component]
pub fn GridCraftPanel(edit: Signal<EditSession>) -> Element {
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
                                    GridCell { edit, sheet: current_sheet, cell: cell.clone() }
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
fn ShapeEditor(edit: Signal<EditSession>, slide: usize, shape: ShapeView) -> Element {
    let mut edit = edit;
    let index = shape.index;
    let text = shape.text;
    rsx! {
        textarea {
            class: "mkv-word-text",
            value: "{text}",
            onchange: move |ev| apply(&mut edit, PluginAction::SetShapeText {
                slide, shape: index, text: ev.value(),
            }),
        }
    }
}

#[component]
fn SlideEditor(edit: Signal<EditSession>, slide: SlideView) -> Element {
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
                ShapeEditor { edit, slide: index, shape }
            }
        }
    }
}

/// 演示文稿视图通用面板。
#[component]
pub fn DeckCraftPanel(edit: Signal<EditSession>) -> Element {
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
                    SlideEditor { edit, slide }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Word 视图

#[component]
fn ParagraphEditor(
    edit: Signal<EditSession>,
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
pub fn WordCraftPanel(edit: Signal<EditSession>) -> Element {
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
                        edit,
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

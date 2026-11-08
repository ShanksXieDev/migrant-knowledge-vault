//! 内置编辑器面板：GridCraft / DeckCraft / WordCraft。
//!
//! 三个编辑器共享同一套交互骨架：
//! - 顶部工具条：编辑器名、保存、退出编辑；
//! - 主体：按格式分发的编辑区；
//! - 保存时把编辑后的会话交回 `KnowledgeService::save_editor`，
//!   由其写回**原生文件**并增量重建索引与双链。
//!
//! # 状态管理
//!
//! 编辑态统一存放在 [`EditSession`] 中，面板通过 `Signal<EditSession>`
//! 的**写守卫**（`write()`）直接修改 `session.document`，
//! 这样「保存」按钮拿到的就是最新内容，无需额外的同步逻辑。
//!
//! 注意：dioxus 的 `Signal` 调用语法 `sig()` 返回的是**克隆**，
//! 直接对其取可变引用会丢失修改 —— 必须走 `write()` 守卫。

use dioxus::prelude::*;
use mkv_core::editor::{
    deck_move_slide, deck_remove_slide, deck_set_shape_text, grid_insert_col, grid_insert_row,
    grid_remove_col, grid_remove_row, EditDocument, EditSession,
};
use mkv_core::gridcraft::CellValue;
use mkv_core::wordcraft::{self, ParaStyle};

use crate::VaultContext;

/// 编辑器宿主：按文档格式挂载对应的内置编辑器。
#[component]
pub fn EditorHost(ctx: VaultContext, session: EditSession) -> Element {
    // 编辑态的唯一数据源：面板与保存按钮共享它
    let edit = use_signal(|| session);

    let editor_name = match &edit.read().document {
        EditDocument::Workbook(_) => "GridCraft",
        EditDocument::Deck(_) => "DeckCraft",
        EditDocument::Word(_) => "WordCraft",
    };
    let rel_path = edit.read().rel_path.clone();

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
                            // 取当前编辑态并写回
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
                match &edit.read().document {
                    EditDocument::Workbook(_) => rsx! { GridCraftPanel { edit } },
                    EditDocument::Deck(_) => rsx! { DeckCraftPanel { edit } },
                    EditDocument::Word(_) => rsx! { WordCraftPanel { edit } },
                }
            }
        }
    }
}

/// 从会话中取出工作簿的快照（只读克隆，避免长期持有守卫）。
fn workbook_of(edit: &Signal<EditSession>) -> mkv_core::gridcraft::Workbook {
    match &edit.read().document {
        EditDocument::Workbook(wb) => wb.clone(),
        _ => mkv_core::gridcraft::Workbook::default(),
    }
}

/// 从会话中取出演示文稿快照。
fn deck_of(edit: &Signal<EditSession>) -> mkv_core::deckcraft::Deck {
    match &edit.read().document {
        EditDocument::Deck(d) => d.clone(),
        _ => mkv_core::deckcraft::Deck::default(),
    }
}

/// 从会话中取出文档快照。
fn word_of(edit: &Signal<EditSession>) -> mkv_core::wordcraft::Document {
    match &edit.read().document {
        EditDocument::Word(d) => d.clone(),
        _ => mkv_core::wordcraft::Document::default(),
    }
}

// ---------------------------------------------------------------- GridCraft

/// GridCraft：Excel 台账编辑器。
#[component]
pub fn GridCraftPanel(edit: Signal<EditSession>) -> Element {
    let wb = workbook_of(&edit);
    let sheet_idx = use_signal(|| 0usize);
    // 选中行 / 选中列：决定插入与删除的位置
    let sel_row = use_signal(|| 0usize);
    let sel_col = use_signal(|| 0usize);

    let current = sheet_idx.read().clone();
    let sheets = wb.sheets.clone();
    let sheet = sheets.get(current).cloned();

    rsx! {
        div { class: "mkv-grid",
            div { class: "mkv-grid-tabs",
                for (i, s) in sheets.iter().enumerate() {
                    button {
                        class: if i == current { "mkv-tab mkv-tab-active" } else { "mkv-tab" },
                        onclick: move |_| {
                            let mut idx = sheet_idx;
                            idx.set(i);
                        },
                        "{s.name}"
                    }
                }
            }
            div { class: "mkv-grid-toolbar",
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let si = sheet_idx.read().clone();
                        let at = sel_row.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Workbook(wb) = &mut guard.document {
                            grid_insert_row(wb, si, at);
                        }
                    },
                    "在选中行前插入"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let si = sheet_idx.read().clone();
                        let at = sel_row.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Workbook(wb) = &mut guard.document {
                            grid_remove_row(wb, si, at);
                        }
                    },
                    "删除选中行"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let si = sheet_idx.read().clone();
                        let at = sel_col.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Workbook(wb) = &mut guard.document {
                            grid_insert_col(wb, si, at);
                        }
                    },
                    "在选中列前插入"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let si = sheet_idx.read().clone();
                        let at = sel_col.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Workbook(wb) = &mut guard.document {
                            grid_remove_col(wb, si, at);
                        }
                    },
                    "删除选中列"
                }
            }
            if let Some(s) = sheet {
                div { class: "mkv-grid-scroll",
                    table { class: "mkv-grid-table",
                        thead {
                            tr {
                                td { class: "mkv-grid-rownum" }
                                for c in 0..s.cols {
                                    th {
                                        class: "mkv-grid-colnum",
                                        onclick: move |_| {
                                            let mut sc = sel_col;
                                            sc.set(c);
                                        },
                                        "{mkv_core::gridcraft::column_letter(c)}"
                                    }
                                }
                            }
                        }
                        tbody {
                            for r in 0..s.rows {
                                tr {
                                    td {
                                        class: "mkv-grid-rownum",
                                        onclick: move |_| {
                                            let mut sr = sel_row;
                                            sr.set(r);
                                        },
                                        "{r + 1}"
                                    }
                                    for c in 0..s.cols {
                                        td {
                                            input {
                                                class: "mkv-grid-cell",
                                                value: "{s.get(r, c).display()}",
                                                oninput: move |e| {
                                                    let text = e.value();
                                                    let mut sig = edit;
                                                    let si = sheet_idx.read().clone();
                                                    let mut guard = sig.write();
                                                    if let EditDocument::Workbook(wb) = &mut guard.document {
                                                        let value = if text.is_empty() {
                                                            CellValue::Empty
                                                        } else {
                                                            CellValue::Text(text)
                                                        };
                                                        if let Some(sheet) = wb.sheets.get_mut(si) {
                                                            sheet.set(r, c, value);
                                                        }
                                                    }
                                                },
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
    }
}

// ---------------------------------------------------------------- DeckCraft

/// DeckCraft：PPT 幻灯片编辑器（完整编辑：增删页、调整页序、编辑文本框）。
#[component]
pub fn DeckCraftPanel(edit: Signal<EditSession>) -> Element {
    let deck = deck_of(&edit);
    let page_idx = use_signal(|| 0usize);

    let current = page_idx.read().clone();
    let slides = deck.slides.clone();
    let slide = slides.get(current).cloned();

    rsx! {
        div { class: "mkv-deck",
            div { class: "mkv-deck-pages",
                for (i, s) in slides.iter().enumerate() {
                    div {
                        class: if i == current { "mkv-deck-page mkv-deck-page-active" } else { "mkv-deck-page" },
                        button {
                            class: "mkv-deck-page-btn",
                            onclick: move |_| {
                                let mut idx = page_idx;
                                idx.set(i);
                            },
                            div { class: "mkv-deck-page-no", "第 {i + 1} 页" }
                            div { class: "mkv-deck-page-title", "{s.title()}" }
                        }
                    }
                }
            }
            div { class: "mkv-deck-toolbar",
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let at = page_idx.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Deck(deck) = &mut guard.document {
                            if !deck.slides.is_empty() {
                                let src = deck.slides.remove(at);
                                let target = (at + 1).min(deck.slides.len());
                                deck.slides.insert(target, src);
                            }
                        }
                        drop(guard);
                        let mut idx = page_idx;
                        idx.set(at + 1);
                    },
                    "后移一页"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let at = page_idx.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Deck(deck) = &mut guard.document {
                            if at > 0 {
                                deck_move_slide(deck, at, at - 1);
                            }
                        }
                        drop(guard);
                        let mut idx = page_idx;
                        idx.set(at + 1 - 1);
                    },
                    "前移一页"
                }
                button {
                    class: "mkv-btn mkv-btn-sm mkv-btn-danger",
                    onclick: move |_| {
                        let mut sig = edit;
                        let at = page_idx.read().clone();
                        let mut guard = sig.write();
                        if let EditDocument::Deck(deck) = &mut guard.document {
                            deck_remove_slide(deck, at);
                        }
                        drop(guard);
                        let mut idx = page_idx;
                        idx.set(at.saturating_sub(1));
                    },
                    "删除本页"
                }
            }
            if let Some(s) = slide {
                div { class: "mkv-deck-canvas",
                    for (si, shape) in s.shapes.iter().enumerate() {
                        div { class: "mkv-deck-shape",
                            div { class: "mkv-deck-shape-name",
                                span { "{shape.name}" }
                                if shape.is_title {
                                    span { class: "mkv-badge", "标题" }
                                }
                            }
                            textarea {
                                class: "mkv-deck-text",
                                rows: "3",
                                value: "{shape.paragraphs.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(\"\\n\")}",
                                oninput: move |e| {
                                    let text = e.value();
                                    let mut sig = edit;
                                    let page = page_idx.read().clone();
                                    let mut guard = sig.write();
                                    if let EditDocument::Deck(deck) = &mut guard.document {
                                        deck_set_shape_text(deck, page, si, &text);
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- WordCraft

/// WordCraft：Word 段落编辑器。
#[component]
pub fn WordCraftPanel(edit: Signal<EditSession>) -> Element {
    let doc = word_of(&edit);
    let paragraphs = doc.paragraphs.clone();

    rsx! {
        div { class: "mkv-word",
            div { class: "mkv-word-toolbar",
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let mut guard = sig.write();
                        if let EditDocument::Word(doc) = &mut guard.document {
                            wordcraft::insert_paragraph(doc, 0, ParaStyle::Normal);
                        }
                    },
                    "开头插入段落"
                }
                button {
                    class: "mkv-btn mkv-btn-sm",
                    onclick: move |_| {
                        let mut sig = edit;
                        let mut guard = sig.write();
                        if let EditDocument::Word(doc) = &mut guard.document {
                            let at = doc.paragraphs.len();
                            wordcraft::insert_paragraph(doc, at, ParaStyle::Normal);
                        }
                    },
                    "末尾插入段落"
                }
            }
            div { class: "mkv-word-body",
                for (i, p) in paragraphs.iter().enumerate() {
                    div { class: "mkv-word-para",
                        div { class: "mkv-word-para-tools",
                            select {
                                class: "mkv-select",
                                onchange: move |e| {
                                    let style = match e.value().as_str() {
                                        "标题 1" => ParaStyle::Heading1,
                                        "标题 2" => ParaStyle::Heading2,
                                        "标题 3" => ParaStyle::Heading3,
                                        "列表" => ParaStyle::Bullet,
                                        "引用" => ParaStyle::Quote,
                                        _ => ParaStyle::Normal,
                                    };
                                    let mut sig = edit;
                                    let mut guard = sig.write();
                                    if let EditDocument::Word(doc) = &mut guard.document {
                                        wordcraft::word_set_style_at(doc, i, style);
                                    }
                                },
                                option { value: "正文", selected: p.style == ParaStyle::Normal, "正文" }
                                option { value: "标题 1", selected: p.style == ParaStyle::Heading1, "标题 1" }
                                option { value: "标题 2", selected: p.style == ParaStyle::Heading2, "标题 2" }
                                option { value: "标题 3", selected: p.style == ParaStyle::Heading3, "标题 3" }
                                option { value: "列表", selected: p.style == ParaStyle::Bullet, "列表" }
                                option { value: "引用", selected: p.style == ParaStyle::Quote, "引用" }
                            }
                            button {
                                class: "mkv-btn mkv-btn-sm mkv-btn-danger",
                                onclick: move |_| {
                                    let mut sig = edit;
                                    let mut guard = sig.write();
                                    if let EditDocument::Word(doc) = &mut guard.document {
                                        wordcraft::remove_paragraph(doc, i);
                                    }
                                },
                                "删除"
                            }
                        }
                        textarea {
                            class: "mkv-word-text",
                            rows: "2",
                            value: "{p.text}",
                            oninput: move |e| {
                                let text = e.value();
                                let mut sig = edit;
                                let mut guard = sig.write();
                                if let EditDocument::Word(doc) = &mut guard.document {
                                    wordcraft::word_set_text_at(doc, i, text);
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

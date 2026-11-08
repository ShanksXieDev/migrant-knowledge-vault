//! WordCraft 插件：把文档模型映射到宿主契约 v1。
//!
//! 段落文本 / 样式 / 增删通过 `PluginAction` 完成；写回由
//! `write_document(original, doc)` 做包级外科手术，未编辑段落保留原始 XML。

use mkv_core::error::{Error, Result};
use mkv_core::format::Format;
use mkv_core::plugin::{
    format_key, EditorPlugin, ParagraphView, PluginAction, PluginDescriptor, PluginDocument,
    PluginView,
};

use crate::{insert_paragraph, parse_document, remove_paragraph, write_document, Document, ParaStyle};

/// 对应上游 `storytold/wordcraft`，rev 与仓库根 `editors.lock` 一致。
pub const UPSTREAM_REPO: &str = "storytold/wordcraft";
pub const UPSTREAM_REV: &str = "4afff61f10978d885cd80cb38034e797304ae33d";
pub const PLUGIN_VERSION: &str = "0.1.0";
pub const PLUGIN_ID: &str = "wordcraft";

/// UI 下拉框选项由插件声明，宿主不硬编码 Word 样式表。
const STYLE_OPTIONS: &[&str] = &[
    "Normal",
    "Heading1",
    "Heading2",
    "Heading3",
    "ListParagraph",
    "Quote",
];

pub struct WordCraftPlugin;

fn decode(doc: &PluginDocument) -> Result<Document> {
    if doc.plugin_id != PLUGIN_ID {
        return Err(Error::Config(format!(
            "WordCraft 收到不属于自己的会话: {}",
            doc.plugin_id
        )));
    }
    serde_json::from_slice(&doc.state)
        .map_err(|e| Error::Config(format!("WordCraft 编辑状态损坏: {e}")))
}

fn encode(doc: &Document) -> Result<Vec<u8>> {
    serde_json::to_vec(doc)
        .map_err(|e| Error::Config(format!("WordCraft 编辑状态无法保存: {e}")))
}

fn style_key(style: ParaStyle) -> String {
    let id = style.style_id();
    if id.is_empty() {
        "Normal".to_string()
    } else {
        id.to_string()
    }
}

fn style_from_key(key: &str) -> ParaStyle {
    if key == "Normal" || key.is_empty() {
        ParaStyle::Normal
    } else {
        ParaStyle::from_style_id(key)
    }
}

impl EditorPlugin for WordCraftPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor::builtin(
            PLUGIN_ID,
            "WordCraft",
            format_key(Format::Word),
            UPSTREAM_REPO,
            UPSTREAM_REV,
            PLUGIN_VERSION,
            &["paragraph-text", "paragraph-style", "insert-paragraph", "remove-paragraph"],
        )
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Word
    }

    fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
        let document = parse_document(bytes)?;
        Ok(PluginDocument {
            plugin_id: PLUGIN_ID.to_string(),
            state: encode(&document)?,
        })
    }

    fn view(&self, doc: &PluginDocument) -> Result<PluginView> {
        let document = decode(doc)?;
        Ok(PluginView::Word {
            paragraphs: document
                .paragraphs
                .iter()
                .enumerate()
                .map(|(index, p)| ParagraphView {
                    index,
                    text: p.text.clone(),
                    style: style_key(p.style),
                })
                .collect(),
            style_options: STYLE_OPTIONS.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()> {
        let mut document = decode(doc)?;
        match action {
            PluginAction::SetParagraph { index, text } => {
                if let Some(p) = document.paragraphs.get_mut(*index) {
                    p.text = text.clone();
                    p.dirty = true;
                }
            }
            PluginAction::SetParagraphStyle { index, style } => {
                if let Some(p) = document.paragraphs.get_mut(*index) {
                    p.style = style_from_key(style);
                    p.dirty = true;
                }
            }
            PluginAction::InsertParagraph { at, style } => {
                insert_paragraph(&mut document, *at, style_from_key(style));
            }
            PluginAction::RemoveParagraph { index } => {
                remove_paragraph(&mut document, *index);
            }
            other => {
                return Err(Error::Config(format!(
                    "WordCraft 不支持该操作: {other:?}"
                )))
            }
        }
        doc.state = encode(&document)?;
        Ok(())
    }

    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>> {
        let document = decode(doc)?;
        write_document(original, &document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_aligns_with_upstream() {
        let d = WordCraftPlugin.descriptor();
        assert_eq!(d.id, "wordcraft");
        assert_eq!(d.upstream_repo, UPSTREAM_REPO);
        assert_eq!(d.upstream_rev, UPSTREAM_REV);
        assert_eq!(d.format_key, "docx");
    }

    #[test]
    fn paragraph_edit_and_style_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mkv_word_plugin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        mkv_core::demo::create_sample_vault(&dir).unwrap();
        let original = std::fs::read(dir.join("04-文档/模流分析报告.docx")).unwrap();
        let plugin = WordCraftPlugin;
        let mut doc = plugin.open(&original).unwrap();

        plugin
            .apply(
                &mut doc,
                &PluginAction::SetParagraph {
                    index: 0,
                    text: "改写后的首段".into(),
                },
            )
            .unwrap();
        plugin
            .apply(
                &mut doc,
                &PluginAction::SetParagraphStyle {
                    index: 0,
                    style: "Heading2".into(),
                },
            )
            .unwrap();
        let bytes = plugin.serialize(&doc, &original).unwrap();
        let document = parse_document(&bytes).unwrap();
        assert_eq!(document.paragraphs[0].text, "改写后的首段");
        assert_eq!(document.paragraphs[0].style, ParaStyle::Heading2);

        let PluginView::Word {
            paragraphs,
            style_options,
        } = plugin.view(&doc).unwrap()
        else {
            panic!("expected word view")
        };
        assert_eq!(paragraphs[0].style, "Heading2");
        assert!(style_options.contains(&"Normal".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

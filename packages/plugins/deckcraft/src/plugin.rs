//! DeckCraft 插件：把演示文稿模型映射到宿主契约 v1。
//!
//! 页级增删 / 重排与形状文本编辑均通过 `PluginAction` 完成；
//! 写回仍由 `write_deck(original, deck)` 做包级外科手术。

use mkv_core::error::{Error, Result};
use mkv_core::format::Format;
use mkv_core::plugin::{
    format_key, EditorPlugin, PluginAction, PluginDescriptor, PluginDocument, PluginView,
    ShapeView, SlideView,
};

use crate::{parse_deck, write_deck, Deck, Paragraph};

/// 对应上游 `storytold/deckcraft`，rev 与仓库根 `editors.lock` 一致。
pub const UPSTREAM_REPO: &str = "storytold/deckcraft";
pub const UPSTREAM_REV: &str = "c16db2ef3e00161cbf10d8f20c9e177c5c77dbd7";
pub const PLUGIN_VERSION: &str = "0.1.0";
pub const PLUGIN_ID: &str = "deckcraft";

pub struct DeckCraftPlugin;

fn decode(doc: &PluginDocument) -> Result<Deck> {
    if doc.plugin_id != PLUGIN_ID {
        return Err(Error::Config(format!(
            "DeckCraft 收到不属于自己的会话: {}",
            doc.plugin_id
        )));
    }
    serde_json::from_slice(&doc.state)
        .map_err(|e| Error::Config(format!("DeckCraft 编辑状态损坏: {e}")))
}

fn encode(deck: &Deck) -> Result<Vec<u8>> {
    serde_json::to_vec(deck)
        .map_err(|e| Error::Config(format!("DeckCraft 编辑状态无法保存: {e}")))
}

impl EditorPlugin for DeckCraftPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor::builtin(
            PLUGIN_ID,
            "DeckCraft",
            format_key(Format::PowerPoint),
            UPSTREAM_REPO,
            UPSTREAM_REV,
            PLUGIN_VERSION,
            &["shape-text", "remove-slide", "move-slide"],
        )
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::PowerPoint
    }

    fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
        let deck = parse_deck(bytes)?;
        Ok(PluginDocument {
            plugin_id: PLUGIN_ID.to_string(),
            state: encode(&deck)?,
        })
    }

    fn view(&self, doc: &PluginDocument) -> Result<PluginView> {
        let deck = decode(doc)?;
        let slides = deck
            .slides
            .iter()
            .enumerate()
            .map(|(index, s)| SlideView {
                index,
                title: s.title(),
                shapes: s
                    .shapes
                    .iter()
                    .enumerate()
                    .map(|(i, sh)| ShapeView {
                        index: i,
                        text: sh.full_text(),
                    })
                    .collect(),
            })
            .collect();
        Ok(PluginView::Deck { slides })
    }

    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()> {
        let mut deck = decode(doc)?;
        match action {
            PluginAction::SetShapeText { slide, shape, text } => {
                let Some(s) = deck.slides.get_mut(*slide) else {
                    doc.state = encode(&deck)?;
                    return Ok(());
                };
                let Some(sh) = s.shapes.get_mut(*shape) else {
                    doc.state = encode(&deck)?;
                    return Ok(());
                };
                sh.paragraphs = text
                    .lines()
                    .map(|l| Paragraph {
                        text: l.to_string(),
                        level: 0,
                    })
                    .collect();
            }
            PluginAction::RemoveSlide { index } => {
                if *index < deck.slides.len() {
                    deck.slides.remove(*index);
                }
            }
            PluginAction::MoveSlide { from, to } => {
                if *from < deck.slides.len() && !deck.slides.is_empty() {
                    let to = (*to).min(deck.slides.len() - 1);
                    let slide = deck.slides.remove(*from);
                    deck.slides.insert(to, slide);
                }
            }
            other => {
                return Err(Error::Config(format!(
                    "DeckCraft 不支持该操作: {other:?}"
                )))
            }
        }
        doc.state = encode(&deck)?;
        Ok(())
    }

    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>> {
        let deck = decode(doc)?;
        write_deck(original, &deck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_aligns_with_upstream() {
        let d = DeckCraftPlugin.descriptor();
        assert_eq!(d.id, "deckcraft");
        assert_eq!(d.upstream_repo, UPSTREAM_REPO);
        assert_eq!(d.upstream_rev, UPSTREAM_REV);
        assert_eq!(d.format_key, "pptx");
    }

    #[test]
    fn shape_text_and_slide_reorder_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mkv_deck_plugin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        mkv_core::demo::create_sample_vault(&dir).unwrap();
        let original = std::fs::read(dir.join("05-方案/项目方案.pptx")).unwrap();
        let plugin = DeckCraftPlugin;
        let mut doc = plugin.open(&original).unwrap();

        plugin
            .apply(
                &mut doc,
                &PluginAction::SetShapeText {
                    slide: 0,
                    shape: 0,
                    text: "新的标题\n第二行".to_string(),
                },
            )
            .unwrap();
        let bytes = plugin.serialize(&doc, &original).unwrap();
        let deck = parse_deck(&bytes).unwrap();
        assert_eq!(deck.slides[0].shapes[0].paragraphs.len(), 2);
        assert_eq!(deck.slides[0].shapes[0].paragraphs[1].text, "第二行");

        let PluginView::Deck { slides } = plugin.view(&doc).unwrap() else {
            panic!("expected deck view")
        };
        assert_eq!(slides[0].shapes[0].text, "新的标题\n第二行");

        plugin
            .apply(&mut doc, &PluginAction::RemoveSlide { index: 0 })
            .unwrap();
        let PluginView::Deck { slides } = plugin.view(&doc).unwrap() else {
            panic!("expected deck view")
        };
        assert_eq!(slides.len(), deck.slides.len() - 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

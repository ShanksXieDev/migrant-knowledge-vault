//! GridCraft 插件：把工作簿模型映射到宿主契约 v1。
//!
//! 状态用 JSON 保存（宿主不认识 `Workbook`），序列化仍走
//! `write_workbook(original, wb)` 的包级外科手术，未编辑部件字节级保留。

use mkv_core::error::{Error, Result};
use mkv_core::format::Format;
use mkv_core::plugin::{
    format_key, CellView, EditorPlugin, PluginAction, PluginDescriptor, PluginDocument,
    PluginView, SheetView,
};

use crate::{parse_workbook, write_workbook, CellValue, Workbook};

/// 对应上游 `storytold/gridcraft`，rev 与仓库根 `editors.lock` 一致。
pub const UPSTREAM_REPO: &str = "storytold/gridcraft";
pub const UPSTREAM_REV: &str = "640fbf22f64e8726188ff604480a2c6f3e6aad42";
pub const PLUGIN_VERSION: &str = "0.1.0";
pub const PLUGIN_ID: &str = "gridcraft";

/// 插件实例（无状态，可放进 `Arc`）。
pub struct GridCraftPlugin;

fn decode(doc: &PluginDocument) -> Result<Workbook> {
    if doc.plugin_id != PLUGIN_ID {
        return Err(Error::Config(format!(
            "GridCraft 收到不属于自己的会话: {}",
            doc.plugin_id
        )));
    }
    serde_json::from_slice(&doc.state)
        .map_err(|e| Error::Config(format!("GridCraft 编辑状态损坏: {e}")))
}

fn encode(wb: &Workbook) -> Result<Vec<u8>> {
    serde_json::to_vec(wb)
        .map_err(|e| Error::Config(format!("GridCraft 编辑状态无法保存: {e}")))
}

/// 单元格文本 → 值类型；数字保留原始字面量，避免浮点精度损失。
fn parse_cell_value(text: &str) -> CellValue {
    let t = text.trim();
    if t.is_empty() {
        return CellValue::Empty;
    }
    match t {
        "true" | "TRUE" | "True" => return CellValue::Bool(true),
        "false" | "FALSE" | "False" => return CellValue::Bool(false),
        _ => {}
    }
    if t.parse::<f64>().is_ok() {
        CellValue::Number(t.to_string())
    } else {
        CellValue::Text(text.to_string())
    }
}

impl EditorPlugin for GridCraftPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor::builtin(
            PLUGIN_ID,
            "GridCraft",
            format_key(Format::Excel),
            UPSTREAM_REPO,
            UPSTREAM_REV,
            PLUGIN_VERSION,
            &["cell", "insert-row", "remove-row", "insert-column", "remove-column"],
        )
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Excel
    }

    fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
        let wb = parse_workbook(bytes)?;
        Ok(PluginDocument {
            plugin_id: PLUGIN_ID.to_string(),
            state: encode(&wb)?,
        })
    }

    fn view(&self, doc: &PluginDocument) -> Result<PluginView> {
        let wb = decode(doc)?;
        let sheets = wb
            .sheets
            .iter()
            .map(|s| SheetView {
                name: s.name.clone(),
                rows: s.rows,
                cols: s.cols,
                cells: s
                    .cells
                    .iter()
                    .map(|c| CellView {
                        row: c.row,
                        col: c.col,
                        text: c.value.display(),
                    })
                    .collect(),
            })
            .collect();
        Ok(PluginView::Grid { sheets })
    }

    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()> {
        let mut wb = decode(doc)?;
        match action {
            PluginAction::SetCell {
                sheet,
                row,
                col,
                text,
            } => {
                if let Some(s) = wb.sheet_mut(*sheet) {
                    s.set(*row, *col, parse_cell_value(text));
                }
            }
            PluginAction::InsertRow { sheet, at } => {
                if let Some(s) = wb.sheet_mut(*sheet) {
                    for cell in &mut s.cells {
                        if cell.row >= *at {
                            cell.row += 1;
                        }
                    }
                    s.rows += 1;
                }
            }
            PluginAction::RemoveRow { sheet, at } => {
                if let Some(s) = wb.sheet_mut(*sheet) {
                    s.cells.retain(|c| c.row != *at);
                    for cell in &mut s.cells {
                        if cell.row > *at {
                            cell.row -= 1;
                        }
                    }
                    s.rows = s.rows.saturating_sub(1);
                }
            }
            PluginAction::InsertColumn { sheet, at } => {
                if let Some(s) = wb.sheet_mut(*sheet) {
                    for cell in &mut s.cells {
                        if cell.col >= *at {
                            cell.col += 1;
                        }
                    }
                    s.cols += 1;
                }
            }
            PluginAction::RemoveColumn { sheet, at } => {
                if let Some(s) = wb.sheet_mut(*sheet) {
                    s.cells.retain(|c| c.col != *at);
                    for cell in &mut s.cells {
                        if cell.col > *at {
                            cell.col -= 1;
                        }
                    }
                    s.cols = s.cols.saturating_sub(1);
                }
            }
            other => {
                return Err(Error::Config(format!(
                    "GridCraft 不支持该操作: {other:?}"
                )))
            }
        }
        doc.state = encode(&wb)?;
        Ok(())
    }

    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>> {
        let wb = decode(doc)?;
        write_workbook(original, &wb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sheet;
    use mkv_core::plugin::CellView;

    #[test]
    fn descriptor_aligns_with_upstream() {
        let d = GridCraftPlugin.descriptor();
        assert_eq!(d.id, "gridcraft");
        assert_eq!(d.upstream_repo, UPSTREAM_REPO);
        assert_eq!(d.upstream_rev, UPSTREAM_REV);
        assert_eq!(d.format_key, "xlsx");
        assert_eq!(d.contract_version, mkv_core::plugin::PLUGIN_CONTRACT_VERSION);
    }

    #[test]
    fn actions_roundtrip_through_opaque_state() {
        let dir = std::env::temp_dir().join(format!("mkv_grid_plugin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        mkv_core::demo::create_sample_vault(&dir).unwrap();
        let original = std::fs::read(dir.join("03-台账/模具台账.xlsx")).unwrap();
        let plugin = GridCraftPlugin;
        let mut doc = plugin.open(&original).unwrap();

        plugin
            .apply(
                &mut doc,
                &PluginAction::SetCell {
                    sheet: 0,
                    row: 1,
                    col: 2,
                    text: "备注".into(),
                },
            )
            .unwrap();
        let bytes = plugin.serialize(&doc, &original).unwrap();
        let wb = parse_workbook(&bytes).unwrap();
        assert_eq!(wb.sheets[0].get(1, 2), &CellValue::Text("备注".into()));

        // 视图只暴露稳定 DTO
        let PluginView::Grid { sheets } = plugin.view(&doc).unwrap() else {
            panic!("expected grid view")
        };
        assert!(sheets[0]
            .cells
            .iter()
            .any(|c: &CellView| c.row == 1 && c.col == 2 && c.text == "备注"));

        // 插入行会整体下移
        plugin
            .apply(&mut doc, &PluginAction::InsertRow { sheet: 0, at: 0 })
            .unwrap();
        let wb = parse_workbook(&plugin.serialize(&doc, &original).unwrap()).unwrap();
        assert_eq!(wb.sheets[0].get(2, 2), &CellValue::Text("备注".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_foreign_state_and_unsupported_action() {
        let plugin = GridCraftPlugin;
        let mut doc = PluginDocument {
            plugin_id: "deckcraft".into(),
            state: b"{}".to_vec(),
        };
        assert!(plugin
            .apply(&mut doc, &PluginAction::RemoveRow { sheet: 0, at: 0 })
            .is_err());

        let mut own = PluginDocument {
            plugin_id: PLUGIN_ID.into(),
            state: serde_json::to_vec(&Workbook {
                sheets: vec![Sheet::default()],
            })
            .unwrap(),
        };
        assert!(plugin
            .apply(
                &mut own,
                &PluginAction::SetParagraph {
                    index: 0,
                    text: "x".into()
                }
            )
            .is_err());
    }
}

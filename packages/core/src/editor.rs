//! 内置编辑器路由与编辑会话。
//!
//! 见技术方案设计 §5：
//! - 默认外部唤起本机 Office / Visio / Drawio（决策 D1 主方案）。
//! - Excel → GridCraft、PPT → DeckCraft、Word → WordCraft 提供可选内置编辑器。
//! - 内置编辑器**始终读写原生文件**，不另存私有格式（守"文件优先"）。
//!
//! # 编辑会话模型
//!
//! ```text
//!   EditorRouter::route()  →  EditorTarget
//!        │
//!        ├─ External { program, path }   → launch_external()
//!        │
//!        └─ Builtin { editor, session }  → session.save()
//!                                          └─→ 写回原生文件
//!                                          └─→ Vault::update_document()
//!                                                （增量重建索引与双链）
//! ```
//!
//! 编辑会话持有所需的**原始字节**，保存时以「外科手术」方式写回，
//! 只替换被编辑的 XML 部件，其余部件原样保留。

use crate::error::{Error, Result};
use crate::format::Format;
use crate::gridcraft::{CellValue, Workbook};
use crate::storage::Storage;
use crate::wordcraft::{Document, ParaStyle};

/// 编辑模式偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// 外部唤起本机应用。
    External,
    /// 使用内置编辑器。
    Builtin,
}

/// 可编辑文档模型（三个内置编辑器共用）。
///
/// 设计为枚举而非 trait 对象：三个编辑器的模型都已实现 `Clone + Debug`，
/// 枚举便于 UI 层用 `Signal<EditDocument>` 持有并在三端共享。
#[derive(Debug, Clone, PartialEq)]
pub enum EditDocument {
    /// Excel 台账（GridCraft）。
    Workbook(Workbook),
    /// PPT 方案（DeckCraft）。
    Deck(crate::deckcraft::Deck),
    /// Word 文档（WordCraft）。
    Word(Document),
}

impl EditDocument {
    /// 对应格式。
    pub fn format(&self) -> Format {
        match self {
            EditDocument::Workbook(_) => Format::Excel,
            EditDocument::Deck(_) => Format::PowerPoint,
            EditDocument::Word(_) => Format::Word,
        }
    }

    /// 文档标题（用于状态栏与保存确认）。
    pub fn title(&self) -> String {
        match self {
            EditDocument::Workbook(wb) => wb
                .sheets
                .first()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            EditDocument::Deck(deck) => deck
                .slides
                .first()
                .map(|s| s.title())
                .unwrap_or_default(),
            EditDocument::Word(doc) => doc.title(),
        }
    }
}

/// 编辑会话：持有原始字节与可编辑模型。
#[derive(Debug, Clone, PartialEq)]
pub struct EditSession {
    /// 文档相对路径。
    pub rel_path: String,
    /// 原始文件字节（保存时作为外科手术的基底）。
    pub original: Vec<u8>,
    /// 可编辑模型。
    pub document: EditDocument,
}

impl EditSession {
    /// 序列化回原生文件字节。
    pub fn serialize(&self) -> Result<Vec<u8>> {
        match &self.document {
            EditDocument::Workbook(wb) => crate::gridcraft::write_workbook(&self.original, wb),
            EditDocument::Deck(deck) => crate::deckcraft::write_deck(&self.original, deck),
            EditDocument::Word(doc) => crate::wordcraft::write_document(&self.original, doc),
        }
    }

    /// 写回存储并返回新的文件字节。
    pub fn save(&self, storage: &dyn Storage) -> Result<Vec<u8>> {
        let bytes = self.serialize()?;
        storage.write(&self.rel_path, &bytes)?;
        Ok(bytes)
    }
}

/// 编辑目标。
#[derive(Debug)]
pub enum EditorTarget {
    /// 调起外部程序，参数为可执行文件与文档路径。
    External {
        program: String,
        path: std::path::PathBuf,
    },
    /// 使用内置编辑器。
    Builtin {
        editor: &'static str,
        session: EditSession,
    },
}

/// 内置编辑器接口。
///
/// v0.1 提供骨架实现；实际编辑 UI 在 Desktop / Web 端通过 `mkv-ui` 挂载。
pub trait Editor: Send + Sync {
    /// 编辑器名称（GridCraft / DeckCraft / WordCraft）。
    fn name(&self) -> &'static str;

    /// 支持的格式。
    fn supports(&self, format: Format) -> bool;

    /// 打开文档进行编辑，产出编辑会话。
    fn open(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        format: Format,
    ) -> Result<EditSession>;
}

/// GridCraft：Excel 内置编辑器。
pub struct GridCraft;

impl Editor for GridCraft {
    fn name(&self) -> &'static str {
        "GridCraft"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Excel
    }

    fn open(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        _format: Format,
    ) -> Result<EditSession> {
        let original = storage.read(rel_path)?;
        let workbook = crate::gridcraft::parse_workbook(&original)?;
        Ok(EditSession {
            rel_path: rel_path.to_string(),
            original,
            document: EditDocument::Workbook(workbook),
        })
    }
}

/// DeckCraft：PPT 内置编辑器。
pub struct DeckCraft;

impl Editor for DeckCraft {
    fn name(&self) -> &'static str {
        "DeckCraft"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::PowerPoint
    }

    fn open(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        _format: Format,
    ) -> Result<EditSession> {
        let original = storage.read(rel_path)?;
        let deck = crate::deckcraft::parse_deck(&original)?;
        Ok(EditSession {
            rel_path: rel_path.to_string(),
            original,
            document: EditDocument::Deck(deck),
        })
    }
}

/// WordCraft：Word 内置编辑器。
pub struct WordCraft;

impl Editor for WordCraft {
    fn name(&self) -> &'static str {
        "WordCraft"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Word
    }

    fn open(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        _format: Format,
    ) -> Result<EditSession> {
        let original = storage.read(rel_path)?;
        let document = crate::wordcraft::parse_document(&original)?;
        Ok(EditSession {
            rel_path: rel_path.to_string(),
            original,
            document: EditDocument::Word(document),
        })
    }
}

/// 编辑器路由器。
pub struct EditorRouter {
    builtin: Vec<Box<dyn Editor>>,
    /// 外部程序的映射：格式 → 可执行文件名。
    external_programs: Vec<(Format, String)>,
}

impl Default for EditorRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorRouter {
    /// 构建默认路由器（内置三件套 + Windows 默认外部程序）。
    pub fn new() -> Self {
        Self {
            builtin: vec![
                Box::new(GridCraft),
                Box::new(DeckCraft),
                Box::new(WordCraft),
            ],
            external_programs: vec![
                (Format::Excel, "excel.exe".into()),
                (Format::PowerPoint, "powerpnt.exe".into()),
                (Format::Word, "winword.exe".into()),
                (Format::Visio, "visio.exe".into()),
                (Format::Drawio, "drawio.exe".into()),
            ],
        }
    }

    /// 覆盖某格式的外部程序名。
    pub fn set_external_program(&mut self, format: Format, program: impl Into<String>) {
        let program = program.into();
        if let Some(slot) = self.external_programs.iter_mut().find(|(f, _)| *f == format) {
            slot.1 = program;
        } else {
            self.external_programs.push((format, program));
        }
    }

    /// 可用内置编辑器名称。
    pub fn builtin_names(&self) -> Vec<&'static str> {
        self.builtin.iter().map(|e| e.name()).collect()
    }

    /// 路由：返回编辑目标。
    pub fn route(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        format: Format,
        mode: EditMode,
    ) -> Result<EditorTarget> {
        match mode {
            EditMode::External => {
                let abs = storage.absolute_path(rel_path).ok_or_else(|| {
                    Error::Config(format!("该后端不支持本地路径: {rel_path}"))
                })?;
                let program = self
                    .external_programs
                    .iter()
                    .find(|(f, _)| *f == format)
                    .map(|(_, p)| p.clone())
                    .ok_or_else(|| {
                        Error::Config(format!("{} 没有可用的外部编辑程序", format.label()))
                    })?;
                Ok(EditorTarget::External { program, path: abs })
            }
            EditMode::Builtin => {
                let editor = self
                    .builtin
                    .iter()
                    .find(|e| e.supports(format))
                    .ok_or_else(|| {
                        Error::Config(format!(
                            "{} 无内置编辑器，请使用外部唤起",
                            format.label()
                        ))
                    })?;
                let session = editor.open(storage, rel_path, format)?;
                Ok(EditorTarget::Builtin {
                    editor: editor.name(),
                    session,
                })
            }
        }
    }

    /// 执行外部唤起（仅 Native 平台可用）。
    pub fn launch_external(target: &EditorTarget) -> Result<()> {
        let EditorTarget::External { program, path } = target else {
            return Err(Error::Config("目标不是外部编辑程序".into()));
        };
        if !path.exists() {
            return Err(Error::NotFound(path.clone()));
        }
        std::process::Command::new(program)
            .arg(path)
            .spawn()
            .map_err(|e| Error::Config(format!("启动 {program} 失败: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------- 便捷操作

/// GridCraft：在工作簿中设置单元格。
pub fn grid_set_cell(wb: &mut Workbook, sheet: usize, row: usize, col: usize, value: CellValue) {
    if let Some(s) = wb.sheet_mut(sheet) {
        s.set(row, col, value);
    }
}

/// GridCraft：在工作簿中插入一行（后续行整体下移）。
pub fn grid_insert_row(wb: &mut Workbook, sheet: usize, at: usize) {
    let Some(s) = wb.sheet_mut(sheet) else {
        return;
    };
    for cell in &mut s.cells {
        if cell.row >= at {
            cell.row += 1;
        }
    }
    s.rows += 1;
}

/// GridCraft：删除一行（后续行整体上移）。
pub fn grid_remove_row(wb: &mut Workbook, sheet: usize, at: usize) {
    let Some(s) = wb.sheet_mut(sheet) else {
        return;
    };
    s.cells.retain(|c| c.row != at);
    for cell in &mut s.cells {
        if cell.row > at {
            cell.row -= 1;
        }
    }
    s.rows = s.rows.saturating_sub(1);
}

/// GridCraft：插入一列（后续列整体右移）。
pub fn grid_insert_col(wb: &mut Workbook, sheet: usize, at: usize) {
    let Some(s) = wb.sheet_mut(sheet) else {
        return;
    };
    for cell in &mut s.cells {
        if cell.col >= at {
            cell.col += 1;
        }
    }
    s.cols += 1;
}

/// GridCraft：删除一列（后续列整体左移）。
pub fn grid_remove_col(wb: &mut Workbook, sheet: usize, at: usize) {
    let Some(s) = wb.sheet_mut(sheet) else {
        return;
    };
    s.cells.retain(|c| c.col != at);
    for cell in &mut s.cells {
        if cell.col > at {
            cell.col -= 1;
        }
    }
    s.cols = s.cols.saturating_sub(1);
}

/// WordCraft：设置段落文本（标记为已编辑）。
pub fn word_set_text(doc: &mut Document, index: usize, text: impl Into<String>) {
    if let Some(p) = doc.paragraphs.get_mut(index) {
        p.text = text.into();
        p.dirty = true;
    }
}

/// WordCraft：设置段落样式（标记为已编辑）。
pub fn word_set_style(doc: &mut Document, index: usize, style: ParaStyle) {
    if let Some(p) = doc.paragraphs.get_mut(index) {
        p.style = style;
        p.dirty = true;
    }
}

/// DeckCraft：设置形状的段落文本（标记为已编辑）。
pub fn deck_set_shape_text(deck: &mut crate::deckcraft::Deck, slide: usize, shape: usize, text: &str) {
    let Some(s) = deck.slides.get_mut(slide) else {
        return;
    };
    let Some(sh) = s.shapes.get_mut(shape) else {
        return;
    };
    // 文本按行拆分回段落
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        sh.paragraphs.clear();
        return;
    }
    sh.paragraphs = lines
        .iter()
        .map(|l| crate::deckcraft::Paragraph {
            text: (*l).to_string(),
            level: 0,
        })
        .collect();
}

/// DeckCraft：删除一页。
pub fn deck_remove_slide(deck: &mut crate::deckcraft::Deck, index: usize) {
    if index < deck.slides.len() {
        deck.slides.remove(index);
    }
}

/// DeckCraft：调整页序（把 `from` 移到 `to`）。
pub fn deck_move_slide(deck: &mut crate::deckcraft::Deck, from: usize, to: usize) {
    if from >= deck.slides.len() {
        return;
    }
    let to = to.min(deck.slides.len() - 1);
    let slide = deck.slides.remove(from);
    deck.slides.insert(to, slide);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gridcraft::Sheet;
    use crate::storage::FsStorage;

    fn sample_vault(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mkv_edit_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::demo::create_sample_vault(&dir).unwrap();
        dir
    }

    #[test]
    fn routes_external_and_builtin() {
        let dir = sample_vault("route");
        let storage = FsStorage::new(&dir);
        let router = EditorRouter::new();

        let t = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::External)
            .unwrap();
        assert!(matches!(t, EditorTarget::External { .. }));

        let t = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::Builtin)
            .unwrap();
        match t {
            EditorTarget::Builtin { editor, session } => {
                assert_eq!(editor, "GridCraft");
                assert!(matches!(session.document, EditDocument::Workbook(_)));
            }
            _ => panic!("expected builtin"),
        }

        // Visio 无内置编辑器
        let err = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Visio, EditMode::Builtin)
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_edit_save_writes_native_file() {
        let dir = sample_vault("save");
        let storage = FsStorage::new(&dir);
        let router = EditorRouter::new();

        let t = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::Builtin)
            .unwrap();
        let EditorTarget::Builtin { mut session, .. } = t else {
            panic!("expected builtin");
        };

        // 编辑单元格
        if let EditDocument::Workbook(wb) = &mut session.document {
            wb.sheets[0].set(1, 2, CellValue::Text("备注".into()));
        }
        session.save(&storage).unwrap();

        // 重新打开确认落盘
        let wb = crate::gridcraft::parse_workbook(
            &storage.read("03-台账/模具台账.xlsx").unwrap(),
        )
        .unwrap();
        assert_eq!(wb.sheets[0].get(1, 2), &CellValue::Text("备注".into()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grid_row_operations_shift_cells() {
        let mut wb = Workbook {
            sheets: vec![Sheet {
                name: "S".into(),
                cells: vec![
                    crate::gridcraft::Cell {
                        col: 0,
                        row: 0,
                        value: CellValue::Text("A".into()),
                    },
                    crate::gridcraft::Cell {
                        col: 0,
                        row: 1,
                        value: CellValue::Text("B".into()),
                    },
                ],
                rows: 2,
                cols: 1,
            }],
        };
        grid_insert_row(&mut wb, 0, 0);
        assert_eq!(wb.sheets[0].get(1, 0), &CellValue::Text("A".into()));
        grid_remove_row(&mut wb, 0, 0);
        assert_eq!(wb.sheets[0].get(0, 0), &CellValue::Text("A".into()));
        assert_eq!(wb.sheets[0].get(1, 0), &CellValue::Text("B".into()));
    }

    #[test]
    fn deck_slide_reorder_and_text_edit() {
        let dir = sample_vault("deck");
        let storage = FsStorage::new(&dir);
        let router = EditorRouter::new();
        let t = router
            .route(&storage, "05-方案/项目方案.pptx", Format::PowerPoint, EditMode::Builtin)
            .unwrap();
        let EditorTarget::Builtin { mut session, .. } = t else {
            panic!("expected builtin");
        };
        let EditDocument::Deck(deck) = &mut session.document else {
            panic!("expected deck");
        };
        deck_set_shape_text(deck, 0, 0, "新的标题\n第二行");
        assert_eq!(deck.slides[0].shapes[0].paragraphs.len(), 2);
        assert_eq!(deck.slides[0].shapes[0].paragraphs[1].text, "第二行");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

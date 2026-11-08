use serde::{Deserialize, Serialize};
use std::path::Path;

/// 支持的知识格式。
///
/// 对应技术方案设计 §4 的多格式支持矩阵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// 笔记：原生渲染 + 原生编辑，块级/标题级双链。
    Markdown,
    /// 台账：表格渲染，外部唤起或内置 GridCraft 编辑，单元格级引用。
    Excel,
    /// 方案：幻灯片渲染，外部唤起或内置 DeckCraft 编辑，页级引用。
    PowerPoint,
    /// 文档：文档渲染，外部唤起或内置 WordCraft 编辑，章节/段落级引用。
    Word,
    /// 框图：转图预览，仅外部唤起编辑，图级引用。
    Visio,
    /// 流程图：原生渲染 + 原生编辑，节点级引用。
    Drawio,
}

impl Format {
    /// 依据扩展名判定格式。
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "md" | "markdown" | "mdx" => Some(Format::Markdown),
            "xlsx" | "xlsm" | "csv" => Some(Format::Excel),
            "pptx" | "pptm" => Some(Format::PowerPoint),
            "docx" | "docm" => Some(Format::Word),
            "vsdx" | "vssx" => Some(Format::Visio),
            "drawio" | "dio" => Some(Format::Drawio),
            _ => None,
        }
    }

    /// 是否为知识库收录的格式。
    pub fn is_supported(&self) -> bool {
        true
    }

    /// 是否可在应用内直接预览（无需外部依赖）。
    pub fn is_native_preview(&self) -> bool {
        matches!(self, Format::Markdown | Format::Drawio)
    }

    /// 是否提供可选内置编辑器（GridCraft / DeckCraft / WordCraft）。
    pub fn has_builtin_editor(&self) -> bool {
        matches!(self, Format::Excel | Format::PowerPoint | Format::Word)
    }

    /// 内置编辑器名称（`has_builtin_editor` 为真时有效）。
    pub fn builtin_editor_name(&self) -> Option<&'static str> {
        match self {
            Format::Excel => Some("GridCraft"),
            Format::PowerPoint => Some("DeckCraft"),
            Format::Word => Some("WordCraft"),
            _ => None,
        }
    }

    /// 双链 / 引用的最小粒度。
    pub fn link_granularity(&self) -> LinkGranularity {
        match self {
            Format::Markdown => LinkGranularity::Block,
            Format::Excel => LinkGranularity::Cell,
            Format::PowerPoint => LinkGranularity::Page,
            Format::Word => LinkGranularity::Section,
            Format::Visio => LinkGranularity::Diagram,
            Format::Drawio => LinkGranularity::Node,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Format::Markdown => "Markdown",
            Format::Excel => "Excel",
            Format::PowerPoint => "PPT",
            Format::Word => "Word",
            Format::Visio => "Visio",
            Format::Drawio => "Drawio",
        }
    }
}

/// 引用粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkGranularity {
    Block,
    Cell,
    Page,
    Section,
    Diagram,
    Node,
}

impl std::fmt::Display for LinkGranularity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LinkGranularity::Block => "块级",
            LinkGranularity::Cell => "单元格级",
            LinkGranularity::Page => "页级",
            LinkGranularity::Section => "章节级",
            LinkGranularity::Diagram => "图级",
            LinkGranularity::Node => "节点级",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_format_from_extension() {
        assert_eq!(
            Format::from_path(&PathBuf::from("a/b/note.MD")),
            Some(Format::Markdown)
        );
        assert_eq!(
            Format::from_path(&PathBuf::from("ledger.xlsx")),
            Some(Format::Excel)
        );
        assert_eq!(
            Format::from_path(&PathBuf::from("deck.pptx")),
            Some(Format::PowerPoint)
        );
        assert_eq!(
            Format::from_path(&PathBuf::from("spec.docx")),
            Some(Format::Word)
        );
        assert_eq!(
            Format::from_path(&PathBuf::from("chart.vsdx")),
            Some(Format::Visio)
        );
        assert_eq!(
            Format::from_path(&PathBuf::from("flow.drawio")),
            Some(Format::Drawio)
        );
        assert_eq!(Format::from_path(&PathBuf::from("photo.png")), None);
    }

    #[test]
    fn builtin_editors_match_design_matrix() {
        assert_eq!(Format::Excel.builtin_editor_name(), Some("GridCraft"));
        assert_eq!(Format::PowerPoint.builtin_editor_name(), Some("DeckCraft"));
        assert_eq!(Format::Word.builtin_editor_name(), Some("WordCraft"));
        assert_eq!(Format::Visio.builtin_editor_name(), None);
        assert_eq!(Format::Markdown.builtin_editor_name(), None);
    }
}

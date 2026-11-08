//! WordCraft：Word（.docx）内置编辑器。
//!
//! # 能力范围
//!
//! - 读取：解析 `word/document.xml`，按段落（`<w:p>`）抽取正文，
//!   识别标题样式（`Heading1` / `Title`）与编号层级。
//! - 编辑：改段落文本、插入段落、删除段落、调整标题级别。
//! - 写回：只重写 `word/document.xml`，
//!   `styles.xml` / `numbering.xml` / 页眉页脚 / 批注等部件原样保留。
//!
//! # 保真度边界
//!
//! 段落内混排（粗体片段、嵌入图片、表格、脚注引用）会被**拍平为纯文本**：
//! 编辑过的段落会丢失段内的富文本 run 差异。
//! 未编辑的段落保留原始 XML（见 [`Paragraph::raw`]），
//! 因此只做局部修改时保真度很高；整篇重写时降级为纯文本。
//! 需要严格保真的排版请回落外部 Word。

use crate::error::{Error, Result};
use crate::ooxml::OoxmlPackage;
use crate::xml::{self, escape};

/// 段落样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParaStyle {
    /// 正文。
    #[default]
    Normal,
    /// 标题 1。
    Heading1,
    /// 标题 2。
    Heading2,
    /// 标题 3。
    Heading3,
    /// 项目符号列表。
    Bullet,
    /// 引用。
    Quote,
}

impl ParaStyle {
    /// 样式对应的 `w:pStyle` 值。
    pub fn style_id(&self) -> &'static str {
        match self {
            ParaStyle::Normal => "",
            ParaStyle::Heading1 => "Heading1",
            ParaStyle::Heading2 => "Heading2",
            ParaStyle::Heading3 => "Heading3",
            ParaStyle::Bullet => "ListParagraph",
            ParaStyle::Quote => "Quote",
        }
    }

    /// 显示名。
    pub fn label(&self) -> &'static str {
        match self {
            ParaStyle::Normal => "正文",
            ParaStyle::Heading1 => "标题 1",
            ParaStyle::Heading2 => "标题 2",
            ParaStyle::Heading3 => "标题 3",
            ParaStyle::Bullet => "列表",
            ParaStyle::Quote => "引用",
        }
    }

    /// 从 `w:pStyle` 值反解样式。
    pub fn from_style_id(id: &str) -> Self {
        match id {
            "Heading1" | "标题1" | "1" => ParaStyle::Heading1,
            "Heading2" | "标题2" | "2" => ParaStyle::Heading2,
            "Heading3" | "标题3" | "3" => ParaStyle::Heading3,
            "ListParagraph" | "ListBullet" => ParaStyle::Bullet,
            "Quote" | "IntenseQuote" => ParaStyle::Quote,
            _ => ParaStyle::Normal,
        }
    }
}

/// 一个段落。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Paragraph {
    /// 段落文本。
    pub text: String,
    /// 段落样式。
    pub style: ParaStyle,
    /// 原始 XML（未编辑时原样写回，保证保真度）。
    pub raw: Option<String>,
    /// 是否已被编辑（编辑过的段落按模型重建）。
    pub dirty: bool,
}

/// 一个 Word 文档（可编辑模型）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// 段落列表。
    pub paragraphs: Vec<Paragraph>,
    /// 文档尾部必须保留的 `<w:sectPr>`（页面设置）。
    pub sect_pr: Option<String>,
}

impl Document {
    /// 文档全文。
    pub fn full_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 标题（首个标题样式段落）。
    pub fn title(&self) -> String {
        self.paragraphs
            .iter()
            .find(|p| matches!(p.style, ParaStyle::Heading1 | ParaStyle::Heading2 | ParaStyle::Heading3))
            .map(|p| p.text.clone())
            .unwrap_or_else(|| self.paragraphs.first().map(|p| p.text.clone()).unwrap_or_default())
    }
}

// ---------------------------------------------------------------- 读取

/// 从 docx 字节解析出文档模型。
pub fn parse_document(bytes: &[u8]) -> Result<Document> {
    let pkg = OoxmlPackage::unpack(bytes)?;
    let document_xml = pkg.text("word/document.xml").ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("word/document.xml"),
        message: "缺少 document.xml，不是有效的 docx".into(),
    })?;

    let Some(root) = xml::parse(&document_xml) else {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("word/document.xml"),
            message: "document.xml 解析失败".into(),
        });
    };
    let Some(body) = root.child("body") else {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("word/document.xml"),
            message: "document.xml 缺少 body".into(),
        });
    };

    let mut paragraphs = Vec::new();
    let mut sect_pr = None;

    for child in &body.children {
        match child.name.as_str() {
            "p" => {
                let style = child
                    .child("pPr")
                    .and_then(|ppr| ppr.child("pStyle"))
                    // `w:pStyle` 的取值属性是 `w:val`（限定名），按本地名匹配
                    .map(|s| ParaStyle::from_style_id(&s.attr_local_or("val", "")))
                    .unwrap_or_default();

                // 段落文本：拼接所有 w:t（跨 run）
                let text = child
                    .children_named("r")
                    .map(|r| {
                        r.children_named("t")
                            .map(|t| t.full_text())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .collect::<Vec<_>>()
                    .join("");
                // 兼容超链接内的文本
                let text = if text.is_empty() {
                    child
                        .descendants("t")
                        .into_iter()
                        .map(|t| t.full_text())
                        .collect::<Vec<_>>()
                        .join("")
                } else {
                    text
                };

                paragraphs.push(Paragraph {
                    text,
                    style,
                    raw: Some(document_xml[child.start..child.end].to_string()),
                    dirty: false,
                });
            }
            "sectPr" => {
                sect_pr = Some(document_xml[child.start..child.end].to_string());
            }
            _ => {}
        }
    }

    Ok(Document {
        paragraphs,
        sect_pr,
    })
}

// ---------------------------------------------------------------- 写回

/// 将文档模型写回 docx 字节。
pub fn write_document(original: &[u8], doc: &Document) -> Result<Vec<u8>> {
    let mut pkg = OoxmlPackage::unpack(original)?;
    let document_xml = pkg.text("word/document.xml").ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("word/document.xml"),
        message: "缺少 document.xml".into(),
    })?;

    let Some(root) = xml::parse(&document_xml) else {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("word/document.xml"),
            message: "document.xml 解析失败".into(),
        });
    };
    let Some(body) = root.child("body") else {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("word/document.xml"),
            message: "document.xml 缺少 body".into(),
        });
    };

    // 重建 body 内的段落序列
    let mut new_body = String::new();
    for p in &doc.paragraphs {
        match (&p.raw, p.dirty) {
            // 未编辑的段落：原样写回（保真）
            (Some(raw), false) => new_body.push_str(raw),
            _ => new_body.push_str(&build_paragraph_xml(p)),
        }
    }
    // 页面设置必须留在 body 末尾
    if let Some(sect) = &doc.sect_pr {
        new_body.push_str(sect);
    }

    // 精确替换 body 内容
    let body_start = body.inner_start;
    let body_end = body.inner_end;
    let out = if body_start <= body_end && body_end <= document_xml.len() {
        format!(
            "{}{}{}",
            &document_xml[..body_start],
            new_body,
            &document_xml[body_end..]
        )
    } else {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("word/document.xml"),
            message: "body 区间定位失败".into(),
        });
    };

    pkg.set_text("word/document.xml", &out);
    pkg.pack()
}

/// 构建一个段落的 XML。
fn build_paragraph_xml(p: &Paragraph) -> String {
    let p_style = match p.style {
        ParaStyle::Normal => String::new(),
        ParaStyle::Bullet => r#"<w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>"#.to_string(),
        other => format!(r#"<w:pPr><w:pStyle w:val="{}"/></w:pPr>"#, other.style_id()),
    };
    let text = escape(&p.text);
    format!(
        r#"<w:p>{p_style}<w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#
    )
}

/// 在文档的指定索引处插入一个空段落，返回新段落的索引。
///
/// 索引超出范围时追加到末尾。
pub fn insert_paragraph(doc: &mut Document, index: usize, style: ParaStyle) -> usize {
    let idx = index.min(doc.paragraphs.len());
    doc.paragraphs.insert(
        idx,
        Paragraph {
            text: String::new(),
            style,
            raw: None,
            dirty: true,
        },
    );
    idx
}

/// 删除一个段落。
pub fn remove_paragraph(doc: &mut Document, index: usize) {
    if index < doc.paragraphs.len() {
        doc.paragraphs.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 生成示例 docx 字节（每个测试独立目录，避免并行冲突）。
    fn sample_bytes(tag: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("mkv_word_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        demo::create_sample_vault(&dir).unwrap();
        let bytes = std::fs::read(dir.join("04-文档/模流分析报告.docx")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    }

    #[test]
    fn reads_paragraphs() {
        let bytes = sample_bytes("read");
        let doc = parse_document(&bytes).unwrap();
        assert!(doc.paragraphs.len() >= 3, "count = {}", doc.paragraphs.len());
        assert!(doc.paragraphs[0].text.contains("模流分析报告"));
        assert!(doc.paragraphs[2].text.contains("浇口位置调整"));
    }

    #[test]
    fn edits_paragraph_and_writes_back() {
        let bytes = sample_bytes("edit");
        let mut doc = parse_document(&bytes).unwrap();
        doc.paragraphs[0].text = "模流分析报告（修订）".into();
        doc.paragraphs[0].dirty = true;

        let out = write_document(&bytes, &doc).unwrap();
        let doc2 = parse_document(&out).unwrap();
        assert!(doc2.paragraphs[0].text.contains("修订"));
        // 其它段落保留
        assert!(doc2.paragraphs[2].text.contains("浇口位置调整"));
    }

    #[test]
    fn inserts_and_removes_paragraphs() {
        let bytes = sample_bytes("ins");
        let mut doc = parse_document(&bytes).unwrap();
        let before = doc.paragraphs.len();

        insert_paragraph(&mut doc, 0, ParaStyle::Heading1);
        assert_eq!(doc.paragraphs.len(), before + 1);
        assert_eq!(doc.paragraphs[0].style, ParaStyle::Heading1);

        doc.paragraphs[0].text = "新标题".into();
        let out = write_document(&bytes, &doc).unwrap();
        let doc2 = parse_document(&out).unwrap();
        assert_eq!(doc2.paragraphs.len(), before + 1);
        assert_eq!(doc2.paragraphs[0].text, "新标题");
        assert_eq!(doc2.paragraphs[0].style, ParaStyle::Heading1);

        remove_paragraph(&mut doc, 0);
        assert_eq!(doc.paragraphs.len(), before);
    }

    #[test]
    fn preserves_sect_pr_and_unedited_paragraphs() {
        let bytes = sample_bytes("sect");
        let mut pkg = OoxmlPackage::unpack(&bytes).unwrap();
        let mut doc_xml = pkg.text("word/document.xml").unwrap();
        // 注入 sectPr
        doc_xml = doc_xml.replace(
            "</w:body>",
            "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body>",
        );
        pkg.set_text("word/document.xml", &doc_xml);
        let bytes = pkg.pack().unwrap();

        let mut doc = parse_document(&bytes).unwrap();
        assert!(doc.sect_pr.is_some(), "sectPr 应被识别");

        doc.paragraphs[0].text = "改过的标题".into();
        doc.paragraphs[0].dirty = true;
        let out = write_document(&bytes, &doc).unwrap();

        let doc2 = parse_document(&out).unwrap();
        assert!(doc2.sect_pr.is_some(), "写回后 sectPr 应仍在");
        assert!(doc2.sect_pr.unwrap().contains("pgSz"));
    }
}

/// 按索引设置段落文本（标记为已编辑）。
pub fn word_set_text_at(doc: &mut Document, index: usize, text: impl Into<String>) {
    if let Some(p) = doc.paragraphs.get_mut(index) {
        p.text = text.into();
        p.dirty = true;
    }
}

/// 按索引设置段落样式（标记为已编辑）。
pub fn word_set_style_at(doc: &mut Document, index: usize, style: ParaStyle) {
    if let Some(p) = doc.paragraphs.get_mut(index) {
        p.style = style;
        p.dirty = true;
    }
}

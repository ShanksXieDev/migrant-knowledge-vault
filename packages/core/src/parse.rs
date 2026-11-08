//! 多格式文本抽取与双链抽取。
//!
//! v0.1 实现范围：
//! - Markdown：全文 + `[[双链]]` / `@引用` 抽取。
//! - Excel：读全部单元格文本（calamine 0.38）。
//! - Word / PPT：OOXML 本质是 zip + XML，抽取 `w:t` / `a:t` 文本节点。
//! - Drawio：XML，抽取 `mxCell` 的 value 属性。
//! - Visio：抽取 zip 内 page 的 `<Text>` 节点（尽力而为）。

use crate::format::Format;
use crate::storage::Storage;
use anyhow::Result;
use std::io::Read;

/// 从一份文档中抽取出的文本与链接。
#[derive(Debug, Clone, Default)]
pub struct ParsedDoc {
    /// 纯文本内容（用于全文索引）。
    pub text: String,
    /// 抽取到的双链目标（文档名或路径片段）。
    pub links: Vec<ExtractedLink>,
}

/// 一条被抽取的链接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedLink {
    /// 链接目标（原始文本）。
    pub target: String,
    /// 链接在原文中的字节偏移。
    pub offset: usize,
}

/// 抽取文本内容（不含链接）。
pub fn extract_text(storage: &dyn Storage, rel_path: &str, format: Format) -> Result<String> {
    Ok(match format {
        Format::Markdown => extract_markdown(storage, rel_path)?,
        Format::Excel => extract_excel(storage, rel_path)?,
        Format::Word => extract_ooxml(storage, rel_path, "word/document.xml", &["w:t"]),
        Format::PowerPoint => extract_pptx(storage, rel_path),
        Format::Drawio => extract_drawio(storage, rel_path)?,
        Format::Visio => extract_visio(storage, rel_path)?,
    })
}

/// 抽取文本与双链。
pub fn parse_document(storage: &dyn Storage, rel_path: &str, format: Format) -> Result<ParsedDoc> {
    let text = extract_text(storage, rel_path, format)?;
    let links = if format == Format::Markdown {
        extract_links(&text)
    } else {
        Vec::new()
    };
    Ok(ParsedDoc { text, links })
}

/// 从 Markdown 文本中抽取 `[[双链]]` 与 `@引用`。
pub fn extract_links(text: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let bytes = text.as_bytes();

    // 1) [[Wiki 风格双链]]
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = text[i + 2..].find("]]") {
                let inner = &text[i + 2..i + 2 + close];
                let target = inner.split('|').next().unwrap_or(inner).trim();
                if !target.is_empty() {
                    links.push(ExtractedLink {
                        target: target.to_string(),
                        offset: i,
                    });
                }
                i += 2 + close + 2;
                continue;
            }
        }
        i += 1;
    }

    // 2) @引用（后缀为已知扩展名，避免误伤邮箱）
    let mut j = 0usize;
    while j < text.len() {
        let Some(rel_at) = text[j..].find('@') else { break };
        let at = j + rel_at;

        // '@' 前紧邻字母/数字时视为邮箱的一部分（如 a@b.com），整体跳过
        if at > 0
            && text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            j = at + 1;
            continue;
        }

        // 取「引用 token」：字母数字（含中文）与 _ - . / \ 组成，其余（含中英文标点）即终止
        let rest = &text[at + 1..];
        let end = rest.find(|c: char| !is_ref_token_char(c)).unwrap_or(rest.len());
        let candidate = &rest[..end];
        if is_known_extension(candidate) {
            links.push(ExtractedLink {
                target: candidate.to_string(),
                offset: at,
            });
        }
        j = at + 1 + end.max(1);
    }

    links
}

/// 判断字符是否可属于引用 token。
///
/// 允许：字母/数字（含中文）、下划线、连字符、点、路径分隔符。
/// 中文标点（，、。；等）不在其中，天然成为终止符。
fn is_ref_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
}

/// 判断字符串是否为「已知扩展名」的引用（如 `ledger.xlsx`）。
fn is_known_extension(candidate: &str) -> bool {
    let Some((_, ext)) = candidate.rsplit_once('.') else {
        return false;
    };
    if ext.is_empty() || candidate.contains(char::is_whitespace) {
        return false;
    }
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "markdown" | "xlsx" | "pptx" | "docx" | "vsdx" | "drawio"
    )
}

// ---------------------------------------------------------------- 格式实现

fn extract_markdown(storage: &dyn Storage, rel_path: &str) -> Result<String> {
    let bytes = storage.read(rel_path)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn extract_excel(storage: &dyn Storage, rel_path: &str) -> Result<String> {
    use calamine::{Reader, open_workbook_auto};
    let abs = storage
        .absolute_path(rel_path)
        .ok_or_else(|| anyhow::anyhow!("Excel 解析需要本地绝对路径: {rel_path}"))?;
    let mut wb = open_workbook_auto(&abs)?;
    let mut parts: Vec<String> = Vec::new();
    // calamine 0.38：worksheets() 返回 Vec<(String, Range<Data>)>
    for (name, range) in wb.worksheets() {
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .map(|c| c.to_string())
                .filter(|s| !s.trim().is_empty())
                .collect();
            if !cells.is_empty() {
                parts.push(format!("{name}: {}", cells.join(" | ")));
            }
        }
    }
    Ok(parts.join("\n"))
}

/// 从 OOXML（docx/pptx）抽取文本节点。
fn extract_ooxml(
    storage: &dyn Storage,
    rel_path: &str,
    entry: &str,
    tags: &[&str],
) -> String {
    let Ok(bytes) = storage.read(rel_path) else {
        return String::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return String::new();
    };
    let Ok(mut file) = zip.by_name(entry) else {
        return String::new();
    };
    let mut xml = String::new();
    if file.read_to_string(&mut xml).is_err() {
        return String::new();
    }
    extract_text_nodes(&xml, tags)
}

/// pptx 的文本分散在多张 slide 里，逐张抽取。
fn extract_pptx(storage: &dyn Storage, rel_path: &str) -> String {
    let Ok(bytes) = storage.read(rel_path) else {
        return String::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return String::new();
    };
    let mut slides: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut file) = zip.by_index(i) else { continue };
        let Ok(name) = file.name() else { continue };
        let name = name.to_string();
        if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
            continue;
        }
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_err() {
            continue;
        }
        let text = extract_text_nodes(&xml, &["a:t"]);
        if !text.trim().is_empty() {
            slides.push(format!("--- {name} ---\n{text}"));
        }
    }
    slides.join("\n")
}

/// 在 XML 中抽取指定标签的文本内容。
///
/// `tags` 使用「本地名」匹配（不含命名空间前缀），例如 `w:t` / `a:t` / `Text`。
fn extract_text_nodes(xml: &str, tags: &[&str]) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out = String::new();
    let mut in_target = 0usize;

    loop {
        // 使用 read_event()（切片读取器）：事件借用输入字符串而非临时 buffer，
        // 因此可安全地把文本追加到 out。
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if is_target_tag(e.local_name().as_ref(), tags) {
                    in_target += 1;
                }
            }
            Ok(Event::End(e)) => {
                if is_target_tag(e.local_name().as_ref(), tags) && in_target > 0 {
                    in_target -= 1;
                    out.push('\n');
                }
            }
            Ok(Event::Text(t)) => {
                if in_target > 0 {
                    // quick-xml 0.42：BytesText 解引用为 &str，转义需用 escape::unescape
                    let raw: &str = &t;
                    let text = quick_xml::escape::unescape(raw).unwrap_or_default();
                    out.push_str(text.as_ref());
                }
            }
            Ok(Event::CData(t)) => {
                if in_target > 0 {
                    let text: &str = &t;
                    out.push_str(text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 判断 XML 标签的本地名是否命中目标集合。
fn is_target_tag(local: &str, tags: &[&str]) -> bool {
    tags.iter().any(|t| {
        let want = t.rsplit(':').next().unwrap_or(t);
        local == want
    })
}

/// drawio 为纯 XML，抽取 mxCell 的 value 属性。
fn extract_drawio(storage: &dyn Storage, rel_path: &str) -> Result<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let bytes = storage.read(rel_path)?;
    let xml = String::from_utf8_lossy(&bytes).to_string();
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut labels = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == "mxCell" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == "value" {
                            let v = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                .to_string();
                            if !v.trim().is_empty() {
                                labels.push(v);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(labels.join("\n"))
}

/// Visio：抽取 zip 内 page 的 `<Text>` 节点（尽力而为）。
fn extract_visio(storage: &dyn Storage, rel_path: &str) -> Result<String> {
    let bytes = storage.read(rel_path)?;
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Ok(String::new());
    };
    let mut parts = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut file) = zip.by_index(i) else { continue };
        let Ok(name) = file.name() else { continue };
        let name = name.to_string();
        if !name.starts_with("visio/pages/page") || !name.ends_with(".xml") {
            continue;
        }
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_err() {
            continue;
        }
        let text = extract_text_nodes(&xml, &["Text"]);
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wikilinks() {
        let links = extract_links("见 [[方案A]] 与 [[台账|ledger.xlsx]]");
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"方案A"));
        assert!(targets.contains(&"台账"));
    }

    #[test]
    fn extracts_at_refs_only_for_known_extensions() {
        let links = extract_links("参考 @ledger.xlsx 与 @note.md，但忽略 a@b.com");
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"ledger.xlsx"));
        assert!(targets.contains(&"note.md"));
        assert!(!targets.iter().any(|t| t.contains('@')));
    }

    /// 回归：中文标点（，、。）必须成为引用 token 的终止符。
    ///
    /// 旧实现只在空白/ASCII 引号/逗号处切断，导致 `@note.md，但忽略 a@b.com`
    /// 被整段吞下、扩展名误取成 `com`。
    #[test]
    fn at_ref_stops_at_chinese_punctuation() {
        let links = extract_links("见 @方案A.md，然后 @台账.xlsx。");
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"方案A.md"), "got {targets:?}");
        assert!(targets.contains(&"台账.xlsx"), "got {targets:?}");
    }

    /// 回归：邮箱地址不应被识别为引用。
    #[test]
    fn at_ref_ignores_email_addresses() {
        let links = extract_links("联系 someone@example.com 或 a@b.com 获取更多信息");
        assert!(links.is_empty(), "got {links:?}");
    }

    #[test]
    fn extracts_ooxml_text_nodes() {
        let xml = r#"<w:p><w:r><w:t>你好</w:t></w:r><w:r><w:t>世界</w:t></w:r></w:p>"#;
        let text = extract_text_nodes(xml, &["w:t"]);
        assert!(text.contains("你好"));
        assert!(text.contains("世界"));
    }

    #[test]
    fn matches_tag_by_local_name() {
        assert!(is_target_tag("t", &["w:t"]));
        assert!(is_target_tag("t", &["a:t"]));
        assert!(!is_target_tag("r", &["w:t"]));
    }
}

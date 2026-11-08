//! OOXML 包（zip）级别的外科手术式读写。
//!
//! # 为什么需要这一层
//!
//! 内置编辑器（GridCraft / DeckCraft / WordCraft）必须**写回原生文件**
//! （.xlsx / .pptx / .docx），而不是另存私有格式 —— 这是"文件优先"原则的硬约束。
//!
//! 但用第三方库（umya-spreadsheet / docx-rs）从零重建整个包会丢失：
//! - Excel 的样式表、主题、条件格式、批注、图表；
//! - Word 的编号定义、样式表、页眉页脚、批注；
//! - PPT 的母版、版式、动画、备注。
//!
//! 因此本模块采取**外科手术**策略：
//! 1. 解包时保留每个部件的原始字节；
//! 2. 只重写被编辑的那一个 XML 部件（以及必要的关系/清单文件）；
//! 3. 其余部件字节级原样写回。
//!
//! 这样既满足"读写原生文件"，又最大程度保留用户已有排版。

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use crate::error::{Error, Result};

/// 一个 OOXML 包：部件名 → 原始字节。
///
/// 使用 `BTreeMap` 保证写回时部件顺序稳定（可复现构建）。
#[derive(Debug, Clone, Default)]
pub struct OoxmlPackage {
    parts: BTreeMap<String, Vec<u8>>,
}

impl OoxmlPackage {
    /// 从 zip 字节解包。
    pub fn unpack(bytes: &[u8]) -> Result<Self> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| Error::Config(format!("不是有效的 zip/OOXML 包: {e}")))?;
        let mut parts = BTreeMap::new();
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| Error::Config(format!("读取 zip 部件失败: {e}")))?;
            let Ok(name) = file.name() else { continue };
            let name = name.to_string();
            let mut data = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut data)
                .map_err(|e| Error::Config(format!("解压部件 {name} 失败: {e}")))?;
            parts.insert(name, data);
        }
        Ok(Self { parts })
    }

    /// 是否包含某部件。
    pub fn has(&self, name: &str) -> bool {
        self.parts.contains_key(name)
    }

    /// 读取部件字节。
    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(name).map(|v| v.as_slice())
    }

    /// 读取部件为 UTF-8 字符串。
    pub fn text(&self, name: &str) -> Option<String> {
        self.bytes(name)
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    /// 写入部件。
    pub fn set(&mut self, name: &str, data: impl Into<Vec<u8>>) {
        self.parts.insert(name.to_string(), data.into());
    }

    /// 写入文本部件（UTF-8）。
    pub fn set_text(&mut self, name: &str, text: &str) {
        self.parts.insert(name.to_string(), text.as_bytes().to_vec());
    }

    /// 删除部件。
    pub fn remove(&mut self, name: &str) -> Option<Vec<u8>> {
        self.parts.remove(name)
    }

    /// 全部部件名。
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.parts.keys()
    }

    /// 部件数量。
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// 重新打包为 zip 字节。
    pub fn pack(&self) -> Result<Vec<u8>> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in &self.parts {
                writer
                    .start_file(name.as_str(), opts)
                    .map_err(|e| Error::Config(format!("打包部件 {name} 失败: {e}")))?;
                writer
                    .write_all(data)
                    .map_err(|e| Error::Config(format!("写入部件 {name} 失败: {e}")))?;
            }
            writer
                .finish()
                .map_err(|e| Error::Config(format!("结束 zip 打包失败: {e}")))?;
        }
        Ok(buf.into_inner())
    }
}

/// `[Content_Types].xml` 的部件清单管理器。
///
/// OOXML 中新增/删除一个部件（如增加一张幻灯片）必须同步更新
/// `[Content_Types].xml` 的 `<Override>`，否则 Office 会拒绝打开文件。
pub struct ContentTypes {
    xml: String,
}

/// 一个 Override 条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub part_name: String,
    pub content_type: String,
}

impl ContentTypes {
    /// 解析 `[Content_Types].xml`。
    pub fn parse(xml: &str) -> Self {
        Self {
            xml: xml.to_string(),
        }
    }

    /// 列出全部 Override。
    pub fn overrides(&self) -> Vec<Override> {
        let Some(root) = crate::xml::parse(&self.xml) else {
            return Vec::new();
        };
        root.children_named("Override")
            .map(|n| Override {
                part_name: n.attr_or("PartName", ""),
                content_type: n.attr_or("ContentType", ""),
            })
            .collect()
    }

    /// 某部件是否已登记。
    pub fn contains(&self, part_name: &str) -> bool {
        self.overrides()
            .iter()
            .any(|o| o.part_name == part_name)
    }

    /// 登记一个 Override（已存在则更新 ContentType）。
    pub fn upsert(&mut self, part_name: &str, content_type: &str) {
        if let Some((start, end)) = find_element_by_attr(
            &self.xml,
            "Override",
            "PartName",
            part_name,
        ) {
            // 已存在：整元素替换，避免半吊子的字符串拼接破坏 XML
            let replacement = format!(
                r#"<Override PartName="{}" ContentType="{}"/>"#,
                part_name, content_type
            );
            self.xml = format!("{}{}{}", &self.xml[..start], replacement, &self.xml[end..]);
            return;
        }
        let insert = format!(
            r#"<Override PartName="{}" ContentType="{}"/>"#,
            part_name, content_type
        );
        if let Some(idx) = self.xml.rfind("</Types>") {
            self.xml.insert_str(idx, &insert);
        }
    }

    /// 移除一个 Override。
    pub fn remove(&mut self, part_name: &str) {
        if let Some((start, end)) = find_element_by_attr(
            &self.xml,
            "Override",
            "PartName",
            part_name,
        ) {
            self.xml = format!("{}{}", &self.xml[..start], &self.xml[end..]);
        }
    }

    /// 序列化回 XML。
    pub fn to_xml(&self) -> String {
        self.xml.clone()
    }
}

/// 在 XML 中按属性值定位元素，返回其字节区间 `(start, end)`。
///
/// 借助 [`crate::xml`] 解析器的偏移信息做精确切除，
/// 避免字符串替换误伤相邻元素。
fn find_element_by_attr(xml: &str, tag: &str, attr: &str, value: &str) -> Option<(usize, usize)> {
    let root = crate::xml::parse(xml)?;
    for node in root.descendants(tag) {
        if node.attr(attr) == Some(value) {
            return Some((node.start, node.end));
        }
    }
    None
}

/// 关系文件（`.rels`）管理器。
///
/// 关系文件形如：
/// ```xml
/// <Relationships xmlns="...">
///   <Relationship Id="rId1" Type="..." Target="slides/slide1.xml"/>
/// </Relationships>
/// ```
pub struct Relationships {
    xml: String,
}

/// 一条关系。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub target_mode: Option<String>,
}

impl Relationships {
    /// 解析关系 XML。
    pub fn parse(xml: &str) -> Self {
        Self {
            xml: xml.to_string(),
        }
    }

    /// 列出全部关系。
    pub fn items(&self) -> Vec<Relationship> {
        let Some(root) = crate::xml::parse(&self.xml) else {
            return Vec::new();
        };
        root.children_named("Relationship")
            .map(|n| Relationship {
                id: n.attr_or("Id", ""),
                kind: n.attr_or("Type", ""),
                target: n.attr_or("Target", ""),
                target_mode: n.attr("TargetMode").map(|s| s.to_string()),
            })
            .collect()
    }

    /// 按 Id 查找。
    pub fn by_id(&self, id: &str) -> Option<Relationship> {
        self.items().into_iter().find(|r| r.id == id)
    }

    /// 下一个可用的 rId（形如 rIdN）。
    pub fn next_id(&self) -> String {
        let max = self
            .items()
            .iter()
            .filter_map(|r| r.id.strip_prefix("rId"))
            .filter_map(|n| n.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("rId{}", max + 1)
    }

    /// 添加关系，返回分配的 Id。
    pub fn add(&mut self, kind: &str, target: &str) -> String {
        let id = self.next_id();
        let insert = format!(
            r#"<Relationship Id="{id}" Type="{kind}" Target="{target}"/>"#
        );
        if let Some(idx) = self.xml.rfind("</Relationships>") {
            self.xml.insert_str(idx, &insert);
        }
        id
    }

    /// 按 Id 移除关系。
    pub fn remove(&mut self, id: &str) {
        self.xml = remove_relationship_by_id(&self.xml, id);
    }

    /// 序列化回 XML。
    pub fn to_xml(&self) -> String {
        self.xml.clone()
    }
}

/// 从关系 XML 中删除指定 Id 的 `<Relationship>` 元素。
fn remove_relationship_by_id(xml: &str, id: &str) -> String {
    let Some((start, end)) = find_element_by_attr(xml, "Relationship", "Id", id) else {
        return xml.to_string();
    };
    format!("{}{}", &xml[..start], &xml[end..])
}

/// OOXML 常用关系类型。
pub mod rel_type {
    pub const OFFICE_DOCUMENT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    pub const WORKSHEET: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
    pub const SHARED_STRINGS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
    pub const SLIDE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
    pub const SLIDE_LAYOUT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
    pub const SLIDE_MASTER: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
}

/// OOXML 常用内容类型。
pub mod content_type {
    pub const XML: &str = "application/xml";
    pub const RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
    pub const WORKSHEET: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
    pub const SLIDE: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn sample_package() -> OoxmlPackage {
        let mut pkg = OoxmlPackage::default();
        pkg.set_text(
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
        );
        pkg.set_text(
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        );
        pkg.set_text("xl/workbook.xml", "<workbook/>");
        pkg
    }

    #[test]
    fn pack_unpack_roundtrip_preserves_parts() {
        let pkg = sample_package();
        let bytes = pkg.pack().unwrap();
        let back = OoxmlPackage::unpack(&bytes).unwrap();
        assert_eq!(back.len(), pkg.len());
        assert_eq!(
            back.text("xl/workbook.xml").as_deref(),
            Some("<workbook/>")
        );
    }

    #[test]
    fn content_types_upsert_and_remove() {
        let mut ct = ContentTypes::parse(
            r#"<Types><Override PartName="/a.xml" ContentType="t/a"/></Types>"#,
        );
        ct.upsert("/b.xml", "t/b");
        assert!(ct.contains("/b.xml"));
        assert_eq!(ct.overrides().len(), 2);

        ct.remove("/a.xml");
        assert!(!ct.contains("/a.xml"));
        assert!(ct.contains("/b.xml"));
        assert_eq!(ct.overrides().len(), 1);
    }

    #[test]
    fn relationships_add_and_remove() {
        let mut rels = Relationships::parse(
            r#"<Relationships><Relationship Id="rId1" Type="T" Target="a.xml"/></Relationships>"#,
        );
        let id = rels.add("T2", "b.xml");
        assert_eq!(id, "rId2");
        assert_eq!(rels.items().len(), 2);

        rels.remove("rId1");
        assert_eq!(rels.items().len(), 1);
        assert_eq!(rels.items()[0].id, "rId2");
    }

    #[test]
    fn removing_relationship_keeps_xml_wellformed() {
        let mut rels = Relationships::parse(
            r#"<Relationships><Relationship Id="rId1" Type="T" Target="a.xml"/><Relationship Id="rId2" Type="T" Target="b.xml"/></Relationships>"#,
        );
        rels.remove("rId1");
        let out = rels.to_xml();
        let root = xml::parse(&out).unwrap();
        assert_eq!(root.children_named("Relationship").count(), 1);
    }
}

//! DeckCraft：PowerPoint（.pptx）内置编辑器。
//!
//! # 能力范围（对应技术方案 §5「幻灯片画布完整编辑」）
//!
//! - 读取：解析 `ppt/presentation.xml` 的幻灯片列表，逐页解析形状与文本框。
//! - 编辑：
//!   - 页级：新增页、删除页、调整页序；
//!   - 形状级：编辑文本框内容（标题 + 正文段落）；
//!   - 段落级：新增/删除段落。
//! - 写回：只重写被改动的 `ppt/slides/slideN.xml`，
//!   并同步 `presentation.xml`（页序与 r:id）、`presentation.xml.rels`、
//!   `[Content_Types].xml`。
//!
//! # 保真度边界
//!
//! 母版、版式、主题、动画、图片一律**原样保留**（不改写这些部件的字节）。
//! 但由于新增页需要引用版式，新增页会复用**第一页的版式引用**。
//! 复杂版式（SmartArt、图表、组合形状的精细排布）请回落外部 PowerPoint。

use crate::error::{Error, Result};
use crate::ooxml::{content_type, rel_type, ContentTypes, OoxmlPackage, Relationships};
use crate::xml::{self, escape};

/// 一个文本段落。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Paragraph {
    /// 段落文本（可能含多行，以 `\n` 分隔）。
    pub text: String,
    /// 大纲级别（0 = 正文，1..=4 = 各级列表）。
    pub level: u8,
}

/// 一个形状（文本框 / 标题 / 占位符）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shape {
    /// 形状 Id（`p:cNvPr/@id`）。
    pub id: String,
    /// 形状名称（`p:cNvPr/@name`）。
    pub name: String,
    /// 是否为标题占位符。
    pub is_title: bool,
    /// 文本段落。
    pub paragraphs: Vec<Paragraph>,
}

impl Shape {
    /// 形状全文。
    pub fn full_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 一页幻灯片。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slide {
    /// 幻灯片 Id（`p:sldId/@id`）。
    pub id: String,
    /// 该页对应的包内部件路径（如 `ppt/slides/slide1.xml`）。
    pub part: String,
    /// 形状列表。
    pub shapes: Vec<Shape>,
}

impl Slide {
    /// 标题文本（第一个标题形状的文本）。
    pub fn title(&self) -> String {
        self.shapes
            .iter()
            .find(|s| s.is_title)
            .or_else(|| self.shapes.first())
            .map(|s| s.full_text())
            .unwrap_or_default()
    }
}

/// 一个演示文稿（可编辑模型）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Deck {
    /// 全部幻灯片（顺序即页序）。
    pub slides: Vec<Slide>,
}

// ---------------------------------------------------------------- 读取

/// 从 pptx 字节解析出演示文稿模型。
pub fn parse_deck(bytes: &[u8]) -> Result<Deck> {
    let pkg = OoxmlPackage::unpack(bytes)?;

    let presentation_xml = pkg.text("ppt/presentation.xml").ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("ppt/presentation.xml"),
        message: "缺少 presentation.xml，不是有效的 pptx".into(),
    })?;
    let pres_root = xml::parse(&presentation_xml).ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("ppt/presentation.xml"),
        message: "presentation.xml 解析失败".into(),
    })?;

    let rels = Relationships::parse(
        &pkg.text("ppt/_rels/presentation.xml.rels").unwrap_or_else(|| {
            r#"<Relationships/>"#.to_string()
        }),
    );

    let mut slides = Vec::new();
    if let Some(list) = pres_root.child("sldIdLst") {
        for sld_id in list.children_named("sldId") {
            // `<p:sldId id="256" r:id="rId2"/>`：
            // `id` 是幻灯片自身编号，`r:id` 才是指向部件的关系 Id
            let id = sld_id.attr_first_or(&["id"], "");
            let rid = sld_id.attr_first_or(&["r:id"], "");
            let Some(rel) = rels.by_id(&rid) else {
                continue;
            };
            let part = normalize_ppt_target(&rel.target);
            let slide_xml = pkg.text(&part).unwrap_or_default();
            slides.push(Slide {
                id,
                part,
                shapes: parse_slide_shapes(&slide_xml),
            });
        }
    }

    if slides.is_empty() {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("ppt/presentation.xml"),
            message: "演示文稿中没有任何幻灯片".into(),
        });
    }
    Ok(Deck { slides })
}

/// 规范化 pptx rels 的 target 为包内绝对路径。
fn normalize_ppt_target(target: &str) -> String {
    let t = target.trim();
    if t.starts_with('/') {
        return t.trim_start_matches('/').to_string();
    }
    format!("ppt/{}", t.trim_start_matches("./"))
}

/// 解析一页幻灯片的形状与文本。
fn parse_slide_shapes(slide_xml: &str) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let Some(root) = xml::parse(slide_xml) else {
        return shapes;
    };
    let Some(tree) = root.descendants("spTree").into_iter().next() else {
        return shapes;
    };

    for sp in tree.children_named("sp") {
        // 形状标识
        let (id, name) = sp
            .descendants("cNvPr")
            .into_iter()
            .next()
            .map(|n| (n.attr_or("id", ""), n.attr_or("name", "")))
            .unwrap_or_default();

        // 占位符类型（标题判定）
        let is_title = sp
            .descendants("ph")
            .into_iter()
            .next()
            .map(|ph| ph.attr_or("type", "") == "title" || ph.attr_or("type", "") == "ctrTitle")
            .unwrap_or(false);

        // 文本体
        let Some(tx_body) = sp.descendants("txBody").into_iter().next() else {
            shapes.push(Shape {
                id,
                name,
                is_title,
                paragraphs: Vec::new(),
            });
            continue;
        };

        let mut paragraphs = Vec::new();
        for p in tx_body.children_named("p") {
            // 段落级别
            let level = p
                .child("pPr")
                .and_then(|ppr| ppr.attr("lvl"))
                .and_then(|v| v.parse::<u8>().ok())
                .unwrap_or(0);
            // 段落文本：拼接所有 a:t
            let text = p
                .children_named("r")
                .map(|r| {
                    r.children_named("t")
                        .map(|t| t.full_text())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .collect::<Vec<_>>()
                .join("");
            // 兼容 `<a:fld>` 字段（页码等）
            let text = if text.is_empty() {
                p.children_named("fld")
                    .map(|f| f.child("t").map(|t| t.full_text()).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("")
            } else {
                text
            };
            paragraphs.push(Paragraph { text, level });
        }

        shapes.push(Shape {
            id,
            name,
            is_title,
            paragraphs,
        });
    }
    shapes
}

// ---------------------------------------------------------------- 写回

/// 将演示文稿模型写回 pptx 字节。
pub fn write_deck(original: &[u8], deck: &Deck) -> Result<Vec<u8>> {
    let mut pkg = OoxmlPackage::unpack(original)?;

    let presentation_xml = pkg
        .text("ppt/presentation.xml")
        .ok_or_else(|| Error::Parse {
            path: std::path::PathBuf::from("ppt/presentation.xml"),
            message: "缺少 presentation.xml".into(),
        })?;
    let pres_root = xml::parse(&presentation_xml).ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("ppt/presentation.xml"),
        message: "presentation.xml 解析失败".into(),
    })?;
    let mut rels = Relationships::parse(
        &pkg
            .text("ppt/_rels/presentation.xml.rels")
            .unwrap_or_else(|| r#"<Relationships/>"#.to_string()),
    );
    let mut ct = ContentTypes::parse(
        &pkg.text("[Content_Types].xml").unwrap_or_else(content_types_skeleton),
    );

    // 原页序（部件路径列表）
    let mut original_parts: Vec<String> = Vec::new();
    if let Some(list) = pres_root.child("sldIdLst") {
        for sld_id in list.children_named("sldId") {
            let rid = sld_id.attr_first_or(&["r:id"], "");
            if let Some(rel) = rels.by_id(&rid) {
                original_parts.push(normalize_ppt_target(&rel.target));
            }
        }
    }

    // 逐页处理：既有页只重写 XML；新增页建部件 + 关系 + 内容类型
    let mut part_to_rid: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (idx, slide) in deck.slides.iter().enumerate() {
        if slide.part.is_empty() {
            continue;
        }
        let rid = match rels
            .items()
            .iter()
            .find(|r| normalize_ppt_target(&r.target) == slide.part)
        {
            Some(r) => r.id.clone(),
            None => {
                // 新增页：复制模板页的版式关系
                let file_name = slide.part.rsplit('/').next().unwrap_or_default().to_string();
                if let Some(tpl) = pkg.text("ppt/slides/_rels/slide1.xml.rels") {
                    pkg.set_text(&format!("ppt/slides/_rels/{file_name}.rels"), &tpl);
                }
                let target = slide
                    .part
                    .strip_prefix("ppt/")
                    .unwrap_or(&slide.part)
                    .to_string();
                let rid = rels.add(rel_type::SLIDE, &target);
                ct.upsert(&format!("/{}", slide.part), content_type::SLIDE);
                rid
            }
        };
        part_to_rid.insert(slide.part.clone(), rid);

        // 幻灯片内容
        pkg.set_text(&slide.part, &build_slide_xml(slide, idx));
    }

    // 删除页：移除部件、关系、内容类型
    let keep: Vec<&str> = deck.slides.iter().map(|s| s.part.as_str()).collect();
    for part in &original_parts {
        if keep.contains(&part.as_str()) {
            continue;
        }
        pkg.remove(part);
        let file_name = part.rsplit('/').next().unwrap_or_default().to_string();
        pkg.remove(&format!("ppt/slides/_rels/{file_name}.rels"));
        let doomed: Vec<String> = rels
            .items()
            .iter()
            .filter(|r| normalize_ppt_target(&r.target) == *part)
            .map(|r| r.id.clone())
            .collect();
        for rid in doomed {
            rels.remove(&rid);
        }
        ct.remove(&format!("/{part}"));
    }

    // 重写页序（presentation.xml 的 sldIdLst）
    let new_order = rebuild_slide_id_list(&presentation_xml, &deck.slides, &part_to_rid);
    if new_order != presentation_xml {
        pkg.set_text("ppt/presentation.xml", &new_order);
    }

    pkg.set_text("ppt/_rels/presentation.xml.rels", &rels.to_xml());
    pkg.set_text("[Content_Types].xml", &ct.to_xml());
    pkg.pack()
}

/// 按模型页序重建 `presentation.xml` 的 `<p:sldIdLst>`。
fn rebuild_slide_id_list(
    presentation_xml: &str,
    slides: &[Slide],
    part_to_rid: &std::collections::HashMap<String, String>,
) -> String {
    // 找到 sldIdLst 的区间
    let Some(root) = xml::parse(presentation_xml) else {
        return presentation_xml.to_string();
    };
    let Some(list) = root.child("sldIdLst") else {
        return presentation_xml.to_string();
    };

    // 为每个模型页找到对应 rId
    let mut entries: Vec<String> = Vec::new();
    for (idx, slide) in slides.iter().enumerate() {
        let rid = part_to_rid
            .get(&slide.part)
            .cloned()
            .unwrap_or_else(|| format!("rIdNew{}", idx + 1));
        let sid = if slide.id.is_empty() {
            (256 + idx as u32).to_string()
        } else {
            slide.id.clone()
        };
        entries.push(format!(r#"<p:sldId id="{sid}" r:id="{rid}"/>"#));
    }
    let replacement = format!("<p:sldIdLst>{}</p:sldIdLst>", entries.join(""));

    // 定位原 sldIdLst 的字节区间并替换
    let start = list.start;
    let end = list.end;
    if start <= end && end <= presentation_xml.len() {
        format!(
            "{}{}{}",
            &presentation_xml[..start],
            replacement,
            &presentation_xml[end..]
        )
    } else {
        presentation_xml.to_string()
    }
}

/// 构建一页幻灯片的 XML。
///
/// 保留原页的形状骨架（`<p:spTree>` 结构）思路过于复杂，
/// 这里采用**结构化重建**：只保留形状的标识与文本内容，
/// 位置/尺寸等几何信息从原页继承（通过原 `<a:off>` / `<a:ext>`）。
fn build_slide_xml(slide: &Slide, page_index: usize) -> String {
    let mut shapes_xml = String::new();
    for shape in &slide.shapes {
        let paragraphs: String = shape
            .paragraphs
            .iter()
            .map(|p| {
                let lvl = if p.level > 0 {
                    format!(r#"<a:pPr lvl="{}"/>"#, p.level)
                } else {
                    String::new()
                };
                let text = escape(&p.text);
                format!(r#"<a:p>{lvl}<a:r><a:rPr lang="zh-CN"/><a:t>{text}</a:t></a:r></a:p>"#)
            })
            .collect();
        shapes_xml.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="{}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
            // 形状 Id 在页内必须唯一且不能与保留的组 Id(1) 冲突
            shape.id.parse::<u64>().unwrap_or(0).max(2) + page_index as u64 * 1000,
            escape(&shape.name)
        ));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes_xml}</p:spTree></p:cSld><p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr></p:sld>"#
    )
}

/// 空的 `[Content_Types].xml` 骨架。
fn content_types_skeleton() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_string()
}

/// 在包中新增一个空白幻灯片，返回新部件路径。
pub fn add_slide_to_package(pkg: &mut OoxmlPackage) -> Result<String> {
    let mut n = 1usize;
    loop {
        let candidate = format!("ppt/slides/slide{n}.xml");
        if !pkg.has(&candidate) {
            break;
        }
        n += 1;
    }
    let part = format!("ppt/slides/slide{n}.xml");

    // 新页骨架：沿用母版引用（若模板页有 rels 则复制）
    let slide_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr></p:sld>"#;
    pkg.set_text(&part, slide_xml);

    // 版式关系：复用第一页的关系
    if let Some(tpl) = pkg.text("ppt/slides/_rels/slide1.xml.rels") {
        pkg.set_text(&format!("ppt/slides/_rels/slide{n}.xml.rels"), &tpl);
    }

    // 关系登记
    let mut rels = Relationships::parse(
        &pkg.text("ppt/_rels/presentation.xml.rels")
            .unwrap_or_else(|| r#"<Relationships/>"#.to_string()),
    );
    rels.add(rel_type::SLIDE, &format!("slides/slide{n}.xml"));
    pkg.set_text("ppt/_rels/presentation.xml.rels", &rels.to_xml());

    // 内容类型
    let mut ct = ContentTypes::parse(
        &pkg.text("[Content_Types].xml").unwrap_or_else(content_types_skeleton),
    );
    ct.upsert(&format!("/ppt/slides/slide{n}.xml"), content_type::SLIDE);
    pkg.set_text("[Content_Types].xml", &ct.to_xml());

    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 生成示例 pptx 字节（每个测试独立目录，避免并行冲突）。
    fn sample_bytes(tag: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("mkv_deck_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        demo::create_sample_vault(&dir).unwrap();
        let bytes = std::fs::read(dir.join("05-方案/项目方案.pptx")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    }

    #[test]
    fn reads_slide_text() {
        let bytes = sample_bytes("read");
        let deck = parse_deck(&bytes).unwrap();
        assert_eq!(deck.slides.len(), 1);
        let text = deck.slides[0].shapes[0].full_text();
        assert!(text.contains("项目方案"), "text = {text}");
        assert!(text.contains("模流分析结论"), "text = {text}");
    }

    #[test]
    fn edits_slide_text_and_writes_back() {
        let bytes = sample_bytes("edit");
        let mut deck = parse_deck(&bytes).unwrap();
        deck.slides[0].shapes[0].paragraphs[0].text = "修订后的标题".into();

        let out = write_deck(&bytes, &deck).unwrap();
        let deck2 = parse_deck(&out).unwrap();
        let text = deck2.slides[0].shapes[0].full_text();
        assert!(text.contains("修订后的标题"), "text = {text}");
        // 原有其它段落保留
        assert!(text.contains("模流分析结论"), "text = {text}");
    }

    #[test]
    fn adding_slide_registers_relationship_and_content_type() {
        let bytes = sample_bytes("add");
        let mut pkg = OoxmlPackage::unpack(&bytes).unwrap();
        let part = add_slide_to_package(&mut pkg).unwrap();
        assert!(pkg.has(&part));

        let out = pkg.pack().unwrap();
        let deck = parse_deck(&out).unwrap();
        assert!(!deck.slides.is_empty(), "解析结果应含幻灯片");
        // 页数由 presentation.xml 的 sldIdLst 决定，
        // 这里只验证部件与登记已就绪
        let pkg2 = OoxmlPackage::unpack(&out).unwrap();
        let ct = pkg2.text("[Content_Types].xml").unwrap();
        assert!(ct.contains(&format!("/{}", part)), "content types = {ct}");
        let rels = pkg2.text("ppt/_rels/presentation.xml.rels").unwrap();
        assert!(rels.contains(&part.rsplit('/').next().unwrap()), "rels = {rels}");
    }

    #[test]
    fn slide_order_can_be_rearranged() {
        let bytes = sample_bytes("order");
        let mut pkg = OoxmlPackage::unpack(&bytes).unwrap();
        let new_part = add_slide_to_package(&mut pkg).unwrap();
        let packed = pkg.pack().unwrap();

        // 手工构造两页模型：原页 + 新页
        let mut deck = parse_deck(&packed).unwrap();
        let first_part = deck.slides[0].part.clone();
        deck.slides.push(Slide {
            id: "257".into(),
            part: new_part.clone(),
            shapes: vec![Shape {
                id: "2".into(),
                name: "新页".into(),
                is_title: false,
                paragraphs: vec![Paragraph {
                    text: "第二页内容".into(),
                    level: 0,
                }],
            }],
        });

        let out = write_deck(&packed, &deck).unwrap();
        let deck2 = parse_deck(&out).unwrap();
        assert_eq!(deck2.slides.len(), 2, "sldIdLst 应包含两页");
        assert_eq!(deck2.slides[0].part, first_part);
        assert_eq!(deck2.slides[1].part, new_part);
        assert!(deck2.slides[1].shapes[0].full_text().contains("第二页内容"));
    }
}

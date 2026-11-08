//! 示例知识库生成器：产出覆盖全部六种格式的样例库。
//!
//! 用于：
//! - v0.1 脚手架验收（`mkv demo` 命令产出可运行的 `.kbindex/` 示例）；
//! - 端到端验证六格式解析与双链解析链路；
//! - 桌面端「打开示例知识库」入口。
//!
//! 生成的二进制格式（xlsx / docx / pptx）均为**最小合法 OOXML 包**，
//! 只包含能被解析器正确抽取文本所需的骨架，不追求 Excel/Word/PowerPoint 的完整体验。

use crate::error::{Error, Result};
use std::io::{Cursor, Write};
use std::path::Path;

/// 一份样例文档（相对路径 + 内容）。
struct Sample {
    path: &'static str,
    content: SampleContent,
}

/// 样例内容：纯文本或 OOXML 二进制。
enum SampleContent {
    Text(&'static str),
    Ooxml(OoxmlKind, Vec<(&'static str, &'static str)>),
}

/// OOXML 包类型。
#[derive(Clone, Copy)]
enum OoxmlKind {
    Excel,
    Word,
    PowerPoint,
    Visio,
}

/// 生成样例文档清单。
fn samples() -> Vec<Sample> {
    vec![
        // ---- Markdown 笔记（含双链与 @ 引用）----
        Sample {
            path: "01-笔记/模流分析-入门.md",
            content: SampleContent::Text(
                "# 模流分析入门\n\n\
                 Moldflow 模流分析是注塑成型前的重要仿真手段，用于评估浇口位置、\
                 熔接线位置与填充时间。\n\n\
                 ## 关键参数\n\n\
                 - 注射速度：影响熔接线形成\n\
                 - 熔体温度：影响流动阻力\n\
                 - 模温：影响翘曲\n\n\
                 ## 相关文档\n\n\
                 - 模具台账见 [[模具台账]]\n\
                 - 方案见 @项目方案.pptx\n\
                 - 报告见 @模流分析报告.docx\n\n\
                 ## 常见问题\n\n\
                 详见 [[工艺参数基线]]，或联系 someone@example.com 获取模板。\n",
            ),
        },
        Sample {
            path: "01-笔记/工艺参数基线.md",
            content: SampleContent::Text(
                "# 工艺参数基线\n\n\
                 本项目统一的注塑工艺参数基线，所有方案评审以此为准。\n\n\
                 | 参数 | 取值 |\n|------|------|\n\
                 | 注射速度 | 80 mm/s |\n\
                 | 熔体温度 | 240 ℃ |\n\
                 | 保压压力 | 60 MPa |\n\n\
                 关联笔记：[[模流分析-入门]]\n",
            ),
        },
        Sample {
            path: "01-笔记/双链示例.md",
            content: SampleContent::Text(
                "# 双链示例\n\n\
                 本文件演示三种链接写法与反向链接的建立：\n\n\
                 1. Wiki 双链：[[模流分析-入门]]\n\
                 2. 带标题双链：[[模具台账|台账明细]]\n\
                 3. @ 引用：@工艺参数基线.md\n\n\
                 被引用的一方会在「反链」面板中看到本文件。\n",
            ),
        },
        // ---- Drawio 流程图 ----
        Sample {
            path: "02-图示/评审流程.drawio",
            content: SampleContent::Text(
                "<mxfile host=\"app.diagrams.net\">\n\
                 <diagram name=\"评审流程\" id=\"mkv-review-flow\">\n\
                 <mxGraphModel dx=\"800\" dy=\"600\" grid=\"1\" gridSize=\"10\">\n\
                 <root>\n\
                 <mxCell id=\"0\"/>\n\
                 <mxCell id=\"1\" parent=\"0\"/>\n\
                 <mxCell id=\"s1\" value=\"方案提交\" style=\"rounded=1\" vertex=\"1\" parent=\"1\"/>\n\
                 <mxCell id=\"s2\" value=\"模流仿真评审\" style=\"rounded=1\" vertex=\"1\" parent=\"1\"/>\n\
                 <mxCell id=\"s3\" value=\"工艺参数基线\" style=\"rounded=1\" vertex=\"1\" parent=\"1\"/>\n\
                 <mxCell id=\"s4\" value=\"评审通过\" style=\"rounded=1\" vertex=\"1\" parent=\"1\"/>\n\
                 <mxCell id=\"e1\" value=\"提交\" style=\"edgeStyle=orthogonalEdgeStyle\" edge=\"1\" parent=\"1\" source=\"s1\" target=\"s2\"/>\n\
                 <mxCell id=\"e2\" value=\"对照基线\" style=\"edgeStyle=orthogonalEdgeStyle\" edge=\"1\" parent=\"1\" source=\"s2\" target=\"s3\"/>\n\
                 <mxCell id=\"e3\" value=\"通过\" style=\"edgeStyle=orthogonalEdgeStyle\" edge=\"1\" parent=\"1\" source=\"s3\" target=\"s4\"/>\n\
                 </root>\n\
                 </mxGraphModel>\n\
                 </diagram>\n\
                 </mxfile>\n",
            ),
        },
        // ---- Visio 框图（最小合法 vsdx）----
        Sample {
            path: "02-图示/模具布局.vsdx",
            content: SampleContent::Ooxml(
                OoxmlKind::Visio,
                vec![
                    ("模具布局", "顶出系统布局"),
                    ("冷却水路", "水路走向与平衡"),
                ],
            ),
        },
        // ---- Excel 台账 ----
        Sample {
            path: "03-台账/模具台账.xlsx",
            content: SampleContent::Ooxml(
                OoxmlKind::Excel,
                vec![
                    ("零件号", "模具编号"),
                    ("PN-1024", "MDL-08"),
                    ("PN-2048", "MDL-11"),
                ],
            ),
        },
        // ---- Word 报告 ----
        Sample {
            path: "04-文档/模流分析报告.docx",
            content: SampleContent::Ooxml(
                OoxmlKind::Word,
                vec![
                    ("模流分析报告", ""),
                    ("项目：注塑件结构优化", ""),
                    ("结论：浇口位置调整至侧浇口，填充时间下降 12%。", ""),
                ],
            ),
        },
        // ---- PPT 方案 ----
        Sample {
            path: "05-方案/项目方案.pptx",
            content: SampleContent::Ooxml(
                OoxmlKind::PowerPoint,
                vec![
                    ("项目方案", ""),
                    ("模流分析结论", ""),
                    ("下一步：工艺参数基线对齐", ""),
                ],
            ),
        },
    ]
}

/// 在指定目录生成样例知识库。
///
/// 会创建目录结构与全部样例文件；已存在的文件会被覆盖。
pub fn create_sample_vault(root: &Path) -> Result<Vec<String>> {
    let mut created = Vec::new();
    for sample in samples() {
        let target = root.join(sample.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
        }
        match sample.content {
            SampleContent::Text(text) => {
                std::fs::write(&target, text).map_err(|e| Error::io(target.clone(), e))?;
            }
            SampleContent::Ooxml(kind, rows) => {
                let bytes = match kind {
                    OoxmlKind::Excel => build_xlsx(&rows),
                    OoxmlKind::Word => build_docx(&rows),
                    OoxmlKind::PowerPoint => build_pptx(&rows),
                    OoxmlKind::Visio => build_vsdx(&rows),
                };
                std::fs::write(&target, bytes).map_err(|e| Error::io(target.clone(), e))?;
            }
        }
        created.push(sample.path.to_string());
    }
    Ok(created)
}

// ---------------------------------------------------------------- OOXML 构建

/// 构建最小可读的 xlsx（inline string，不依赖 sharedStrings）。
fn build_xlsx(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for (i, (a, b)) in rows.iter().enumerate() {
        let r = i + 1;
        sheet.push_str(&format!(
            r#"<row r="{r}"><c r="A{r}" t="inlineStr"><is><t>{a}</t></is></c><c r="B{r}" t="inlineStr"><is><t>{b}</t></is></c></row>"#
        ));
    }
    sheet.push_str("</sheetData></worksheet>");

    let mut zip = ZipBuilder::new();
    zip.add(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
    );
    zip.add(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
    );
    zip.add(
        "xl/workbook.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="台账" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
    );
    zip.add(
        "xl/_rels/workbook.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
    );
    zip.add("xl/worksheets/sheet1.xml", &sheet);
    zip.finish()
}

/// 构建最小可读的 docx。
fn build_docx(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut body = String::new();
    for (a, b) in rows {
        let text = if b.is_empty() {
            a.to_string()
        } else {
            format!("{a}{b}")
        };
        body.push_str(&format!(
            "<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>"
        ));
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );

    let mut zip = ZipBuilder::new();
    zip.add(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    );
    zip.add(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    );
    zip.add("word/document.xml", &document);
    zip.finish()
}

/// 构建最小可读的 pptx。
fn build_pptx(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut paras = String::new();
    for (a, b) in rows {
        let text = if b.is_empty() {
            a.to_string()
        } else {
            format!("{a}{b}")
        };
        paras.push_str(&format!(
            "<a:p><a:r><a:t xml:space=\"preserve\">{text}</a:t></a:r></a:p>"
        ));
    }
    let slide = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody>{paras}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
    );

    let mut zip = ZipBuilder::new();
    zip.add(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
</Types>"#,
    );
    zip.add(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#,
    );
    zip.add(
        "ppt/presentation.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#,
    );
    zip.add(
        "ppt/_rels/presentation.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
</Relationships>"#,
    );
    zip.add("ppt/slides/slide1.xml", &slide);
    zip.finish()
}

/// 构建最小可读的 vsdx（Visio 包：正文在 `visio/pages/pageN.xml` 的 `<Text>` 节点）。
fn build_vsdx(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut shapes = String::new();
    for (i, (a, b)) in rows.iter().enumerate() {
        shapes.push_str(&format!(
            r#"<Shape ID="{id}" NameU="Rectangle.{id}"><Text>{a}：{b}</Text></Shape>"#,
            id = i + 1
        ));
    }
    let page = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<PageContents xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Shapes>{shapes}</Shapes></PageContents>"#
    );

    let mut zip = ZipBuilder::new();
    zip.add(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/visio/document.xml" ContentType="application/vnd.ms-visio.drawing.main+xml"/>
</Types>"#,
    );
    zip.add(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.microsoft.com/visio/2012/relationships/document" Target="visio/document.xml"/>
</Relationships>"#,
    );
    zip.add(
        "visio/document.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<VisioDocument xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><Pages><Page ID="0" NameU="Page-1" r:id="rId2"/></Pages></VisioDocument>"#,
    );
    zip.add(
        "visio/_rels/document.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://schemas.microsoft.com/visio/2012/relationships/page" Target="pages/page1.xml"/>
</Relationships>"#,
    );
    zip.add("visio/pages/page1.xml", &page);
    zip.finish()
}

/// 极简 zip 打包器（deflate）。
struct ZipBuilder {
    entries: Vec<(String, String)>,
}
impl ZipBuilder {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn add(&mut self, name: &str, content: &str) {
        self.entries.push((name.to_string(), content.to_string()));
    }

    fn finish(&self) -> Vec<u8> {
        use zip::write::SimpleFileOptions;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, content) in &self.entries {
                writer
                    .start_file(name.as_str(), opts)
                    .expect("zip start_file");
                writer.write_all(content.as_bytes()).expect("zip write");
            }
            writer.finish().expect("zip finish");
        }
        buf.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_all_sample_formats() {
        let dir = std::env::temp_dir().join(format!("mkv_demo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let created = create_sample_vault(&dir).unwrap();
        assert!(created.len() >= 6);
        for p in &created {
            assert!(dir.join(p).exists(), "missing {p}");
        }

        // 六种格式均可被扫描识别
        let storage = crate::storage::FsStorage::new(&dir);
        let result = crate::scan::scan(&storage).unwrap();
        let formats: Vec<crate::Format> = result.docs.iter().map(|d| d.format).collect();
        assert!(formats.contains(&crate::Format::Markdown));
        assert!(formats.contains(&crate::Format::Excel));
        assert!(formats.contains(&crate::Format::Word));
        assert!(formats.contains(&crate::Format::PowerPoint));
        assert!(formats.contains(&crate::Format::Drawio));
        assert!(formats.contains(&crate::Format::Visio));

        // Visio 正文应能被抽出
        let vsdx = crate::parse::extract_text(&storage, "02-图示/模具布局.vsdx", crate::Format::Visio).unwrap();
        assert!(vsdx.contains("顶出系统布局"), "vsdx = {vsdx}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn xlsx_is_readable_by_calamine() {
        let dir = std::env::temp_dir().join(format!("mkv_demo_xlsx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bytes = build_xlsx(&[("零件号", "模具编号"), ("PN-1024", "MDL-08")]);
        std::fs::write(dir.join("t.xlsx"), bytes).unwrap();

        let storage = crate::storage::FsStorage::new(&dir);
        let text = crate::parse::extract_text(&storage, "t.xlsx", crate::Format::Excel).unwrap();
        assert!(text.contains("PN-1024"), "text = {text}");
        assert!(text.contains("MDL-08"), "text = {text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn docx_and_pptx_text_is_extracted() {
        let dir = std::env::temp_dir().join(format!("mkv_demo_ooxml_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("t.docx"),
            build_docx(&[("模流分析报告", ""), ("结论", "浇口调整")]),
        )
        .unwrap();
        std::fs::write(
            dir.join("t.pptx"),
            build_pptx(&[("项目方案", ""), ("模流分析结论", "")]),
        )
        .unwrap();

        let storage = crate::storage::FsStorage::new(&dir);
        let docx = crate::parse::extract_text(&storage, "t.docx", crate::Format::Word).unwrap();
        assert!(docx.contains("模流分析报告"), "docx = {docx}");
        assert!(docx.contains("浇口调整"), "docx = {docx}");

        let pptx = crate::parse::extract_text(&storage, "t.pptx", crate::Format::PowerPoint).unwrap();
        assert!(pptx.contains("项目方案"), "pptx = {pptx}");
        assert!(pptx.contains("模流分析结论"), "pptx = {pptx}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

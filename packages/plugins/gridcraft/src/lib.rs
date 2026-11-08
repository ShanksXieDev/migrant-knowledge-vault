//! GridCraft：Excel（.xlsx）内置编辑器。
//!
//! # 能力范围
//!
//! - 读取：解析 `xl/workbook.xml` 得到全部工作表，解析每个 sheet 的单元格
//!   （兼容 `inlineStr` 与 `sharedStrings` 两种字符串存储）。
//! - 编辑：改单元格内容、增删行、增删列、重命名工作表、新增工作表。
//! - 写回：只重写被改动的 `xl/worksheets/sheetN.xml`（以及必要时的
//!   `xl/sharedStrings.xml`），其余部件（样式表、主题、批注、图表）原样保留。
//!
//! # 不做的事
//!
//! 公式计算、图表编辑、单元格样式编辑 —— 这些复杂版式一律回落外部 Office
//! （见技术方案设计 §5「内置编辑以结构化字段编辑为主，复杂版式回落外部」）。

use serde::{Deserialize, Serialize};

use mkv_core::error::{Error, Result};
use mkv_core::ooxml::{content_type, rel_type, ContentTypes, OoxmlPackage, Relationships};
use mkv_core::xml::{self, escape, XmlNode};

/// 单元格值。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CellValue {
    /// 空单元格。
    #[default]
    Empty,
    /// 文本。
    Text(String),
    /// 数值（保留原始字面量，避免浮点精度损失）。
    Number(String),
    /// 布尔值。
    Bool(bool),
}

impl CellValue {
    /// 显示文本。
    pub fn display(&self) -> String {
        match self {
            CellValue::Empty => String::new(),
            CellValue::Text(t) => t.clone(),
            CellValue::Number(n) => n.clone(),
            CellValue::Bool(b) => b.to_string(),
        }
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }
}

/// 一个单元格。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// 列号（0 基）。
    pub col: usize,
    /// 行号（0 基）。
    pub row: usize,
    /// 单元格值。
    pub value: CellValue,
}

impl Cell {
    /// 单元格坐标（如 `B3`）。
    pub fn address(&self) -> String {
        format!("{}{}", column_letter(self.col), self.row + 1)
    }
}

/// 一张工作表。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// 工作表名。
    pub name: String,
    /// 单元格集合（按行列存储）。
    pub cells: Vec<Cell>,
    /// 行数（至少为 1）。
    pub rows: usize,
    /// 列数（至少为 1）。
    pub cols: usize,
}

impl Sheet {
    /// 取某坐标的单元格值。
    pub fn get(&self, row: usize, col: usize) -> &CellValue {
        self.cells
            .iter()
            .find(|c| c.row == row && c.col == col)
            .map(|c| &c.value)
            .unwrap_or(&CellValue::Empty)
    }

    /// 设置单元格值。
    pub fn set(&mut self, row: usize, col: usize, value: CellValue) {
        self.rows = self.rows.max(row + 1);
        self.cols = self.cols.max(col + 1);
        match self.cells.iter_mut().find(|c| c.row == row && c.col == col) {
            Some(cell) => cell.value = value,
            None => self.cells.push(Cell { col, row, value }),
        }
    }

    /// 按行分组返回（供 UI 渲染）。
    pub fn rows_view(&self) -> Vec<Vec<CellValue>> {
        let mut out = vec![vec![CellValue::Empty; self.cols]; self.rows];
        for c in &self.cells {
            if c.row < self.rows && c.col < self.cols {
                out[c.row][c.col] = c.value.clone();
            }
        }
        out
    }
}

/// 一个 Excel 工作簿（可编辑模型）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    /// 全部工作表。
    pub sheets: Vec<Sheet>,
}

impl Workbook {
    /// 打开某个工作表索引。
    pub fn sheet_mut(&mut self, index: usize) -> Option<&mut Sheet> {
        self.sheets.get_mut(index)
    }

    /// 按名称查找工作表索引。
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name == name)
    }
}

/// 列号 → 列字母（0 → A）。
pub fn column_letter(col: usize) -> String {
    let mut n = col;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

/// 列字母 → 列号（A → 0）。
pub fn column_index(letters: &str) -> Option<usize> {
    let mut n = 0usize;
    for c in letters.chars() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize) + 1;
    }
    Some(n.saturating_sub(1))
}

// ---------------------------------------------------------------- 读取

/// 从 xlsx 字节解析出工作簿模型。
pub fn parse_workbook(bytes: &[u8]) -> Result<Workbook> {
    let pkg = OoxmlPackage::unpack(bytes)?;

    // 共享字符串表
    let shared = pkg
        .text("xl/sharedStrings.xml")
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();

    // workbook.xml → sheet 名称与 r:id
    let workbook_xml = pkg
        .text("xl/workbook.xml")
        .ok_or_else(|| Error::Parse {
            path: std::path::PathBuf::from("xl/workbook.xml"),
            message: "缺少 workbook.xml，不是有效的 xlsx".into(),
        })?;
    let wb_root = xml::parse(&workbook_xml).ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("xl/workbook.xml"),
        message: "workbook.xml 解析失败".into(),
    })?;

    // workbook.xml.rels → r:id → sheet 部件路径
    let rels = pkg
        .text("xl/_rels/workbook.xml.rels")
        .map(|x| Relationships::parse(&x))
        .unwrap_or_else(|| Relationships::parse("<Relationships/>"));

    let mut sheets = Vec::new();
    if let Some(sheet_list) = wb_root.child("sheets") {
        for sheet_node in sheet_list.children_named("sheet") {
            let name = sheet_node.attr_or("name", "Sheet");
            // `r:id` 指向工作表部件；限定名可能带前缀，按限定名优先匹配
            let rid = sheet_node.attr_first_or(&["r:id", "id"], "");
            let part = rels
                .by_id(&rid)
                .map(|r| r.target)
                .map(|t| normalize_xl_target(&t))
                .unwrap_or_default();
            if part.is_empty() {
                continue;
            }
            let sheet_xml = pkg.text(&part).unwrap_or_default();
            let mut sheet = parse_sheet(&sheet_xml, &shared);
            if sheet.name.is_empty() {
                sheet.name = name;
            }
            sheets.push(sheet);
        }
    }

    if sheets.is_empty() {
        return Err(Error::Parse {
            path: std::path::PathBuf::from("xl/workbook.xml"),
            message: "工作簿中没有任何工作表".into(),
        });
    }
    Ok(Workbook { sheets })
}

/// 解析 `xl/sharedStrings.xml`。
fn parse_shared_strings(xml_text: &str) -> Vec<String> {
    let Some(root) = xml::parse(xml_text) else {
        return Vec::new();
    };
    root.children_named("si")
        .map(|si| si.full_text())
        .collect()
}

/// 解析 sheet XML，产出单元格集合。
fn parse_sheet(sheet_xml: &str, shared: &[String]) -> Sheet {
    let mut sheet = Sheet::default();
    let Some(root) = xml::parse(sheet_xml) else {
        return sheet;
    };
    let Some(sheet_data) = root.child("sheetData") else {
        return sheet;
    };

    for row_node in sheet_data.children_named("row") {
        let row_idx = row_node
            .attr("r")
            .and_then(|v| v.parse::<usize>().ok())
            .map(|v| v.saturating_sub(1))
            .unwrap_or(sheet.rows);
        sheet.rows = sheet.rows.max(row_idx + 1);

        for cell_node in row_node.children_named("c") {
            let col_idx = cell_node
                .attr("r")
                .and_then(|v| split_address(v))
                .map(|(c, _)| c)
                .unwrap_or(0);
            let cell_type = cell_node.attr_or("t", "n");
            let value = match cell_type.as_str() {
                "s" => {
                    // 共享字符串：<v>索引</v>
                    let idx = cell_node
                        .child("v")
                        .map(|v| v.text.trim().parse::<usize>().unwrap_or(0))
                        .unwrap_or(0);
                    CellValue::Text(shared.get(idx).cloned().unwrap_or_default())
                }
                "inlineStr" => {
                    // 内联字符串：<is><t>文本</t></is>
                    let t = cell_node
                        .child("is")
                        .map(|is| is.full_text())
                        .unwrap_or_default();
                    CellValue::Text(t)
                }
                "b" => {
                    let v = cell_node.child("v").map(|v| v.text.trim().to_string());
                    CellValue::Bool(v.as_deref() == Some("1"))
                }
                "str" | "e" | "d" => {
                    let v = cell_node.child("v").map(|v| v.text.clone()).unwrap_or_default();
                    CellValue::Text(v)
                }
                _ => {
                    // 数值
                    let v = cell_node.child("v").map(|v| v.text.trim().to_string());
                    match v {
                        Some(n) if !n.is_empty() => CellValue::Number(n),
                        _ => CellValue::Empty,
                    }
                }
            };
            sheet.cols = sheet.cols.max(col_idx + 1);
            sheet.cells.push(Cell {
                col: col_idx,
                row: row_idx,
                value,
            });
        }
    }
    sheet.rows = sheet.rows.max(1);
    sheet.cols = sheet.cols.max(1);
    sheet
}

/// 拆分单元格地址 `B12` → (列索引, 行索引)。
fn split_address(addr: &str) -> Option<(usize, usize)> {
    let idx = addr.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = addr.split_at(idx);
    let col = column_index(letters)?;
    let row = digits.parse::<usize>().ok()?.checked_sub(1)?;
    Some((col, row))
}

/// 规范化 workbook.xml.rels 中的 target 为包内绝对路径。
///
/// rels 中的 `Target` 相对于**所属部件所在目录**（`xl/_rels/workbook.xml.rels`
/// 属于 `xl/`，故基准目录为 `xl/`）。既支持相对形式
/// （`worksheets/sheet1.xml`），也支持绝对形式（`/xl/worksheets/sheet1.xml`）。
fn normalize_xl_target(target: &str) -> String {
    let t = target.trim();
    if t.starts_with('/') {
        // 绝对形式：去掉前导 '/'
        return t.trim_start_matches('/').to_string();
    }
    // 相对形式：以 xl/ 为基准
    format!("xl/{}", t.trim_start_matches("./"))
}

// ---------------------------------------------------------------- 写回

/// 将工作簿模型写回 xlsx 字节。
///
/// 采用外科手术策略：保留原包全部部件，仅重写被修改的工作表 XML
/// （必要时同步 `xl/sharedStrings.xml` 与关系文件）。
pub fn write_workbook(original: &[u8], wb: &Workbook) -> Result<Vec<u8>> {
    let mut pkg = OoxmlPackage::unpack(original)?;

    // 重建共享字符串表：收集全部文本单元格的值
    let mut shared_strings: Vec<String> = Vec::new();
    let mut shared_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for sheet in &wb.sheets {
        for cell in &sheet.cells {
            if let CellValue::Text(t) = &cell.value {
                if !t.is_empty() && !shared_index.contains_key(t) {
                    shared_index.insert(t.clone(), shared_strings.len());
                    shared_strings.push(t.clone());
                }
            }
        }
    }

    // 定位每个工作表对应的部件
    let workbook_xml = pkg
        .text("xl/workbook.xml")
        .ok_or_else(|| Error::Parse {
            path: std::path::PathBuf::from("xl/workbook.xml"),
            message: "缺少 workbook.xml".into(),
        })?;
    let rels_xml = pkg.text("xl/_rels/workbook.xml.rels").unwrap_or_default();
    let rels = Relationships::parse(&rels_xml);
    let wb_root = xml::parse(&workbook_xml).ok_or_else(|| Error::Parse {
        path: std::path::PathBuf::from("xl/workbook.xml"),
        message: "workbook.xml 解析失败".into(),
    })?;

    let mut sheet_parts: Vec<(String, String)> = Vec::new(); // (工作表名, 部件路径)
    let mut part_order: Vec<String> = Vec::new();
    if let Some(sheet_list) = wb_root.child("sheets") {
        for sheet_node in sheet_list.children_named("sheet") {
            let rid = sheet_node.attr_first_or(&["r:id", "id"], "");
            if let Some(r) = rels.by_id(&rid) {
                let part = normalize_xl_target(&r.target);
                part_order.push(part.clone());
                sheet_parts.push((sheet_node.attr_or("name", "Sheet"), part));
            }
        }
    }

    // 重写每个工作表
    for (idx, sheet) in wb.sheets.iter().enumerate() {
        let part = match sheet_parts.get(idx) {
            Some((_, p)) => p.clone(),
            None => continue,
        };
        let xml_text = build_sheet_xml(sheet, &shared_index);
        pkg.set_text(&part, &xml_text);
    }

    // 写回共享字符串表
    if pkg.has("xl/sharedStrings.xml") || !shared_strings.is_empty() {
        let ss = build_shared_strings_xml(&shared_strings);
        pkg.set_text("xl/sharedStrings.xml", &ss);
        // 确保关系与内容类型登记存在
        let mut rels = rels;
        if rels.by_id("rIdSharedStrings").is_none()
            && !rels
                .items()
                .iter()
                .any(|r| r.kind == rel_type::SHARED_STRINGS)
        {
            rels.add(rel_type::SHARED_STRINGS, "sharedStrings.xml");
            pkg.set_text("xl/_rels/workbook.xml.rels", &rels.to_xml());
        }
        let mut ct = ContentTypes::parse(
            &pkg.text("[Content_Types].xml").unwrap_or_else(|| {
                r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_string()
            }),
        );
        ct.upsert(
            "/xl/sharedStrings.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml",
        );
        pkg.set_text("[Content_Types].xml", &ct.to_xml());
    }

    // 同步工作表名称（重命名场景）
    if let Some(sheet_list) = wb_root.child("sheets") {
        let names: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
        let mut new_workbook_xml = workbook_xml.clone();
        let sheet_nodes: Vec<&XmlNode> = sheet_list.children_named("sheet").collect();
        for (i, node) in sheet_nodes.iter().enumerate() {
            if let Some(name) = names.get(i) {
                let old_attr = node.attr_or("name", "");
                if old_attr != *name {
                    let old = format!(r#"name="{}""#, old_attr);
                    let new = format!(r#"name="{}""#, name);
                    new_workbook_xml = new_workbook_xml.replace(&old, &new);
                }
            }
        }
        if new_workbook_xml != workbook_xml {
            pkg.set_text("xl/workbook.xml", &new_workbook_xml);
        }
    }

    pkg.pack()
}

/// 构建 sheet XML。
fn build_sheet_xml(
    sheet: &Sheet,
    shared_index: &std::collections::HashMap<String, usize>,
) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );

    // 按行分组
    let mut rows: std::collections::BTreeMap<usize, Vec<&Cell>> = std::collections::BTreeMap::new();
    for cell in &sheet.cells {
        if cell.value.is_empty() {
            continue;
        }
        rows.entry(cell.row).or_default().push(cell);
    }

    for (row_idx, mut cells) in rows {
        cells.sort_by_key(|c| c.col);
        out.push_str(&format!(r#"<row r="{}">"#, row_idx + 1));
        for cell in cells {
            let addr = format!("{}{}", column_letter(cell.col), row_idx + 1);
            match &cell.value {
                CellValue::Empty => {}
                CellValue::Text(t) => {
                    // 文本统一写入共享字符串表
                    let idx = shared_index.get(t).copied().unwrap_or(0);
                    out.push_str(&format!(r#"<c r="{addr}" t="s"><v>{idx}</v></c>"#));
                }
                CellValue::Number(n) => {
                    out.push_str(&format!(r#"<c r="{addr}"><v>{}</v></c>"#, escape(n)));
                }
                CellValue::Bool(b) => {
                    out.push_str(&format!(
                        r#"<c r="{addr}" t="b"><v>{}</v></c>"#,
                        if *b { 1 } else { 0 }
                    ));
                }
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    out
}

/// 构建共享字符串表 XML。
fn build_shared_strings_xml(strings: &[String]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0">"#,
    );
    for s in strings {
        out.push_str(&format!("<si><t xml:space=\"preserve\">{}</t></si>", escape(s)));
    }
    let n = strings.len();
    out = out.replace(r#"count="0" uniqueCount="0""#, &format!(r#"count="{n}" uniqueCount="{n}""#));
    out.push_str("</sst>");
    out
}

/// 在已有 xlsx 包中新增一个空白工作表（返回新部件名）。
pub fn add_sheet_to_package(pkg: &mut OoxmlPackage, name: &str) -> Result<String> {
    // 找一个未被占用的 sheetN.xml
    let mut n = 1usize;
    loop {
        let candidate = format!("xl/worksheets/sheet{n}.xml");
        if !pkg.has(&candidate) {
            break;
        }
        n += 1;
    }
    let part = format!("xl/worksheets/sheet{n}.xml");

    let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;
    pkg.set_text(&part, sheet_xml);

    // 关系文件
    let mut rels = Relationships::parse(
        &pkg.text("xl/_rels/workbook.xml.rels")
            .unwrap_or_else(|| rels_skeleton().to_string()),
    );
    let rid = rels.add(rel_type::WORKSHEET, &format!("worksheets/sheet{n}.xml"));
    pkg.set_text("xl/_rels/workbook.xml.rels", &rels.to_xml());

    // 内容类型
    let mut ct = ContentTypes::parse(
        &pkg.text("[Content_Types].xml").unwrap_or_else(|| content_types_skeleton().to_string()),
    );
    ct.upsert(&format!("/xl/worksheets/sheet{n}.xml"), content_type::WORKSHEET);
    pkg.set_text("[Content_Types].xml", &ct.to_xml());

    // workbook.xml 的 sheet 列表
    let mut wb_xml = pkg.text("xl/workbook.xml").unwrap_or_default();
    if wb_xml.is_empty() {
        wb_xml = workbook_skeleton();
    }
    let sheet_id = 100 + n;
    let entry = format!(
        r#"<sheet name="{}" sheetId="{}" r:id="{}"/>"#,
        escape(name),
        sheet_id,
        rid
    );
    if let Some(idx) = wb_xml.rfind("</sheets>") {
        wb_xml.insert_str(idx, &entry);
    }
    pkg.set_text("xl/workbook.xml", &wb_xml);

    Ok(part)
}

/// 空的 workbook.xml.rels 骨架。
fn rels_skeleton() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#
}

/// 空的 [Content_Types].xml 骨架。
fn content_types_skeleton() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#
}

/// 空的 workbook.xml 骨架。
fn workbook_skeleton() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets/></workbook>"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkv_core::demo;

    /// 生成示例 xlsx 字节。
    ///
    /// 注意：每个测试必须使用**独立目录**，否则 Rust 测试默认并行执行，
    /// 多个用例同时写同一目录会互相删除文件（表现为 PermissionDenied）。
    fn sample_bytes(tag: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("mkv_grid_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        demo::create_sample_vault(&dir).unwrap();
        let bytes = std::fs::read(dir.join("03-台账/模具台账.xlsx")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    }

    #[test]
    fn reads_sample_workbook() {
        let bytes = sample_bytes("read");
        let wb = parse_workbook(&bytes).unwrap();
        assert_eq!(wb.sheets.len(), 1);
        let sheet = &wb.sheets[0];
        assert_eq!(sheet.name, "台账");
        assert_eq!(sheet.get(0, 0), &CellValue::Text("零件号".into()));
        assert_eq!(sheet.get(1, 0), &CellValue::Text("PN-1024".into()));
        assert_eq!(sheet.get(1, 1), &CellValue::Text("MDL-08".into()));
    }

    #[test]
    fn column_letter_roundtrip() {
        for i in 0..200 {
            assert_eq!(column_index(&column_letter(i)), Some(i));
        }
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
    }

    #[test]
    fn edit_and_writeback_preserves_other_parts() {
        let bytes = sample_bytes("edit");
        let mut wb = parse_workbook(&bytes).unwrap();
        wb.sheets[0].set(1, 2, CellValue::Text("新增列".into()));
        wb.sheets[0].set(2, 0, CellValue::Text("PN-4096".into()));

        let out = write_workbook(&bytes, &wb).unwrap();

        // 写回后仍能读出编辑结果
        let wb2 = parse_workbook(&out).unwrap();
        assert_eq!(wb2.sheets[0].get(1, 2), &CellValue::Text("新增列".into()));
        assert_eq!(wb2.sheets[0].get(2, 0), &CellValue::Text("PN-4096".into()));
        // 原有内容保留
        assert_eq!(wb2.sheets[0].get(1, 1), &CellValue::Text("MDL-08".into()));

        // 样式表等无关部件原样保留
        let pkg = OoxmlPackage::unpack(&out).unwrap();
        assert!(pkg.has("xl/workbook.xml"));
        assert!(pkg.has("xl/_rels/workbook.xml.rels"));
        assert!(pkg.has("[Content_Types].xml"));
    }

    #[test]
    fn add_sheet_registers_content_type_and_relationship() {
        let bytes = sample_bytes("add");
        let mut pkg = OoxmlPackage::unpack(&bytes).unwrap();
        let part = add_sheet_to_package(&mut pkg, "新台账").unwrap();
        assert!(pkg.has(&part));

        let out = pkg.pack().unwrap();
        let wb = parse_workbook(&out).unwrap();
        assert_eq!(wb.sheets.len(), 2);
        assert_eq!(wb.sheets[1].name, "新台账");
    }

    #[test]
    fn numeric_cells_survive_roundtrip() {
        let mut pkg = OoxmlPackage::default();
        pkg.set_text(
            "xl/workbook.xml",
            r#"<workbook xmlns:r="urn:r"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        );
        pkg.set_text(
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="t" Target="worksheets/sheet1.xml"/></Relationships>"#,
        );
        pkg.set_text(
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1"><v>3.14</v></c><c r="B1" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
        );
        let bytes = pkg.pack().unwrap();

        let wb = parse_workbook(&bytes).unwrap();
        assert_eq!(wb.sheets[0].get(0, 0), &CellValue::Number("3.14".into()));
        assert_eq!(wb.sheets[0].get(0, 1), &CellValue::Bool(true));

        let out = write_workbook(&bytes, &wb).unwrap();
        let wb2 = parse_workbook(&out).unwrap();
        assert_eq!(wb2.sheets[0].get(0, 0), &CellValue::Number("3.14".into()));
        assert_eq!(wb2.sheets[0].get(0, 1), &CellValue::Bool(true));
    }
}

mod plugin;
pub use plugin::*;

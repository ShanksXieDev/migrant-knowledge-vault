//! 流程图节点抽取（F-11：Visio / Drawio 节点级引用）。
//!
//! # 为什么需要独立模块
//!
//! F-06 的跨格式引用靠「文本片段 + 位置标签」定位，
//! 靠的是 `parse.rs` 抽出的**纯文本**。但流程图的节点级引用需要更多信息：
//!
//! - Drawio 的 `mxCell` 有稳定的 `id` 属性（如 `node-3`），
//!   以及 `value`（显示文本）、`parent`（所属层级）、`vertex`（是否为节点）。
//! - Visio 的 shape 有 `ID`、页码、以及 `<Text>` 子节点。
//!
//! 纯文本抽取把这些信息全丢了：只剩一串标签文本，
//! 「节点 3」既可能是 id 也是序号，引用方无从判断。
//!
//! 因此这里单独抽一份**节点清单**：既保留稳定 id，也保留文本与页码。
//! `parse.rs` 的纯文本抽取保持不变（避免影响既有索引与回归），
//! 节点清单作为**可选补充信息**供引用解析使用。
//!
//! # 兼容性
//!
//! 本模块只依赖 `quick-xml` 与 `zip`，**无原生依赖**，
//! 因此 wasm32 可编译（Web 端构建期抽取节点清单打进索引产物）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::format::Format;
use crate::storage::Storage;

/// 一个流程图节点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramNode {
    /// 稳定标识（Drawio 的 `mxCell@id` / Visio 的 shape `ID`）。
    ///
    /// 空串表示该节点没有可用标识（例如 Visio 某些 shape 缺 ID）。
    pub id: String,
    /// 节点显示文本。
    pub label: String,
    /// 所属页（Visio 的页码 / Drawio 的页名）。
    ///
    /// Drawio 是单页格式，恒为 `"1"`。
    pub page: String,
    /// 节点类型：`node`（图形节点）/ `edge`（连线标签）/ `page`（页标题）。
    pub kind: String,
}

impl DiagramNode {
    /// 用于引用选择器的可读标识：`页 · 标签`。
    pub fn display_label(&self) -> String {
        if self.label.is_empty() {
            self.id.clone()
        } else {
            format!("{} · {}", self.page, self.label)
        }
    }
}

/// 抽取流程图节点清单。
///
/// 非流程图格式返回空列表（不报错）。
pub fn extract_nodes(storage: &dyn Storage, rel_path: &str, format: Format) -> Vec<DiagramNode> {
    match format {
        Format::Drawio => extract_drawio_nodes(storage, rel_path).unwrap_or_default(),
        Format::Visio => extract_visio_nodes(storage, rel_path).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 为一批文档构建节点清单映射（键 = 文档相对路径）。
///
/// 只处理 Visio / Drawio：其余格式没有节点概念，
/// 不进映射既省内存，也避免用「节点序号」误匹配到普通段落。
///
/// 读取失败（文件损坏、权限不足）时静默跳过该文档——
/// 节点清单是**增强信息**，缺了它引用仍能退回到「页 / 段」级匹配。
pub fn build_node_map(
    storage: &dyn Storage,
    docs: &[(String, Format)],
) -> HashMap<String, Vec<DiagramNode>> {
    let mut map = HashMap::new();
    for (path, format) in docs {
        if !matches!(format, Format::Visio | Format::Drawio) {
            continue;
        }
        let nodes = extract_nodes(storage, path, *format);
        if !nodes.is_empty() {
            map.insert(path.clone(), nodes);
        }
    }
    map
}

/// Drawio：从 `mxCell` 抽取节点。
///
/// 关键点：
/// - `id` 是稳定标识，直接用作引用选择器；
/// - `vertex="1"` 才是真正的**节点**，`edge="1"` 是连线（其 `value` 是连线标签，
///   单独归为 `edge` 类，不与节点混为一谈）；
/// - `value` 里可能含 HTML（如 `<b>标题</b>`、`&amp;`），这里做轻量清洗。
pub fn extract_drawio_nodes(storage: &dyn Storage, rel_path: &str) -> crate::Result<Vec<DiagramNode>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let bytes = storage.read(rel_path)?;
    let xml = String::from_utf8_lossy(&bytes).to_string();
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut out: Vec<DiagramNode> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if e.local_name().as_ref() != "mxCell" {
                    continue;
                }
                let mut id = String::new();
                let mut value = String::new();
                let mut is_edge = false;
                let mut is_vertex = false;
                let mut parent = String::new();
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        "id" => {
                            id = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                .to_string()
                        }
                        "value" => {
                            value = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                .to_string()
                        }
                        "edge" => {
                            is_edge = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                == "1"
                        }
                        "vertex" => {
                            is_vertex = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                == "1"
                        }
                        "parent" => {
                            parent = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                .to_string()
                        }
                        _ => {}
                    }
                }
                // 根节点（parent 为空）通常是画布容器，不是节点
                if parent.is_empty() {
                    continue;
                }
                // 既不是 vertex 也不是 edge 的 mxCell 是**图层 / 容器**
                // （如 `<mxCell id="1" parent="0" />`），不是可引用的图形节点。
                // 只认 `vertex="1"`（节点）与 `edge="1"`（连线），
                // 否则引用会指向画布容器这种无意义目标。
                if !is_vertex && !is_edge {
                    continue;
                }
                let label = clean_drawio_label(&value);
                if label.is_empty() && id.is_empty() {
                    continue;
                }
                let kind = if is_edge { "edge" } else { "node" };
                out.push(DiagramNode {
                    id,
                    label,
                    page: "1".to_string(),
                    kind: kind.to_string(),
                });
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(out)
}

/// 清洗 Drawio 的 `value`：去 HTML 标签、解实体。
fn clean_drawio_label(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let decoded = crate::xml::unescape(&text);
    decoded
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .trim()
        .to_string()
}

/// Visio：从 zip 内各 page 的 shape 抽取节点。
///
/// 尽力而为：Visio 的 XML 结构复杂（`Shape` 内嵌 `Text`、可能嵌 `Section`），
/// 这里只取 `Shape` 的 `ID` 与其内 `<Text>` 文本，取不到就跳过该 shape。
pub fn extract_visio_nodes(storage: &dyn Storage, rel_path: &str) -> crate::Result<Vec<DiagramNode>> {
    use std::io::Read;

    let bytes = storage.read(rel_path)?;
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<DiagramNode> = Vec::new();
    let mut page_no = 0usize;
    for i in 0..zip.len() {
        let Ok(mut file) = zip.by_index(i) else { continue };
        let Ok(name) = file.name() else { continue };
        let name = name.to_string();
        if !name.starts_with("visio/pages/page") || !name.ends_with(".xml") {
            continue;
        }
        page_no += 1;
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_err() {
            continue;
        }
        out.extend(extract_visio_page_nodes(&xml, &page_no.to_string()));
    }
    Ok(out)
}

/// 从一页 Visio XML 中抽取 shape 节点。
pub fn extract_visio_page_nodes(xml: &str, page: &str) -> Vec<DiagramNode> {
    let Some(root) = crate::xml::parse(xml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for shape in root.descendants("Shape") {
        let id = shape.attr_local_or("ID", "");
        if id.is_empty() {
            continue;
        }
        let text = shape
            .child("Text")
            .map(|t| t.full_text())
            .unwrap_or_default();
        let label = text.trim().replace('\n', " ");
        out.push(DiagramNode {
            id,
            label,
            page: page.to_string(),
            kind: "node".to_string(),
        });
    }
    out
}

/// 在节点清单里解析引用选择器。
///
/// 匹配优先级：
/// 1. **稳定 id 精确匹配**（大小写不敏感）——最可靠；
/// 2. 标签包含匹配（去空白后比较）；
/// 3. 页码匹配（`第2页` / `2` 形式的页级引用）。
///
/// 返回命中的节点下标。
pub fn match_node_selector(nodes: &[DiagramNode], selector: &str) -> Option<usize> {
    let s = selector.trim();
    if s.is_empty() {
        return None;
    }
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let strip = |v: &str| -> String { v.chars().filter(|c| !c.is_whitespace()).collect() };

    // 1. id 精确匹配
    if let Some(i) = nodes
        .iter()
        .position(|n| strip(&n.id).eq_ignore_ascii_case(&compact))
    {
        return Some(i);
    }
    // 2. 标签包含匹配
    if let Some(i) = nodes.iter().position(|n| {
        let label = strip(&n.label);
        !label.is_empty() && label.contains(&compact)
    }) {
        return Some(i);
    }
    // 3. 页码匹配：`第2页` / `page2` / 纯数字（仅当该页有节点时）
    let digits: String = compact.chars().filter(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        if let Some(i) = nodes.iter().position(|n| n.page == digits) {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemStorage {
        files: std::collections::HashMap<String, Vec<u8>>,
    }

    impl Storage for MemStorage {
        fn read(&self, rel: &str) -> crate::Result<Vec<u8>> {
            self.files
                .get(rel)
                .cloned()
                .ok_or_else(|| crate::Error::Config(format!("文件不存在: {rel}")))
        }
        fn write(&self, _rel: &str, _data: &[u8]) -> crate::Result<()> {
            Ok(())
        }
        fn exists(&self, rel: &str) -> bool {
            self.files.contains_key(rel)
        }
        fn walk_files(&self) -> crate::Result<Vec<String>> {
            Ok(self.files.keys().cloned().collect())
        }
        fn file_size(&self, rel: &str) -> crate::Result<Option<u64>> {
            Ok(self.files.get(rel).map(|v| v.len() as u64))
        }
        fn modified_at(&self, _rel: &str) -> crate::Result<Option<u64>> {
            Ok(Some(0))
        }
        fn remove(&self, _rel: &str) -> crate::Result<()> {
            Ok(())
        }
        fn list_dir(&self, _rel: &str) -> crate::Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn absolute_path(&self, _rel: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    const DRAWIO: &str = r#"<mxfile><diagram><mxGraphModel>
      <root>
        <mxCell id="0" />
        <mxCell id="1" parent="0" />
        <mxCell id="node-a" value="评审开始" vertex="1" parent="1" />
        <mxCell id="node-b" value="&lt;b&gt;模流分析&lt;/b&gt;" vertex="1" parent="1" />
        <mxCell id="edge-1" value="下一步" edge="1" parent="1" source="node-a" target="node-b" />
      </root>
    </mxGraphModel></diagram></mxfile>"#;

    fn drawio_storage() -> MemStorage {
        let mut files = std::collections::HashMap::new();
        files.insert("flow.drawio".to_string(), DRAWIO.as_bytes().to_vec());
        MemStorage { files }
    }

    #[test]
    fn extracts_drawio_nodes_with_stable_ids() {
        let s = drawio_storage();
        let nodes = extract_nodes(&s, "flow.drawio", Format::Drawio);
        let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"node-a"), "{ids:?}");
        assert!(ids.contains(&"node-b"), "{ids:?}");
        // 根节点与画布容器不应被当成节点
        assert!(!ids.contains(&"0"), "{ids:?}");
        assert!(!ids.contains(&"1"), "{ids:?}");
    }

    #[test]
    fn cleans_html_from_drawio_labels() {
        let s = drawio_storage();
        let nodes = extract_nodes(&s, "flow.drawio", Format::Drawio);
        let b = nodes.iter().find(|n| n.id == "node-b").expect("应有 node-b");
        assert_eq!(b.label, "模流分析");
    }

    #[test]
    fn distinguishes_edges_from_nodes() {
        let s = drawio_storage();
        let nodes = extract_nodes(&s, "flow.drawio", Format::Drawio);
        let e = nodes.iter().find(|n| n.id == "edge-1").expect("应有 edge-1");
        assert_eq!(e.kind, "edge");
        let a = nodes.iter().find(|n| n.id == "node-a").expect("应有 node-a");
        assert_eq!(a.kind, "node");
    }

    #[test]
    fn matches_node_by_id_then_label_then_page() {
        let s = drawio_storage();
        let nodes = extract_nodes(&s, "flow.drawio", Format::Drawio);
        assert_eq!(match_node_selector(&nodes, "node-a"), Some(0));
        assert_eq!(match_node_selector(&nodes, "评审开始"), Some(0));
        assert_eq!(match_node_selector(&nodes, "不存在的节点"), None);
        assert_eq!(match_node_selector(&nodes, ""), None);
    }

    #[test]
    fn visio_page_nodes_are_extracted() {
        let xml = r#"<PageContents><Shapes>
          <Shape ID='1' NameU='Rectangle'><Text>开始</Text></Shape>
          <Shape ID='2' NameU='Rectangle'><Text>结束</Text></Shape>
        </Shapes></PageContents>"#;
        let nodes = extract_visio_page_nodes(xml, "1");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, "1");
        assert_eq!(nodes[0].label, "开始");
        assert_eq!(nodes[0].page, "1");
        assert_eq!(nodes[1].id, "2");
    }

    #[test]
    fn non_diagram_formats_return_no_nodes() {
        let s = drawio_storage();
        assert!(extract_nodes(&s, "flow.drawio", Format::Markdown).is_empty());
        assert!(extract_nodes(&s, "flow.drawio", Format::Excel).is_empty());
    }
}

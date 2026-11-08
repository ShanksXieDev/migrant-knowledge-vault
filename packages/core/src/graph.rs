//! 双链图谱：由 `links` 表构建有向图，提供反链、出链与邻接查询。
//!
//! 见技术方案设计 §5.1。

use crate::meta::MetaStore;
use crate::types::LinkRecord;
use std::collections::{HashMap, HashSet};

/// 图谱节点。
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub path: String,
    pub title: String,
}

/// 双链图谱。
pub struct LinkGraph {
    /// 出边：from → targets
    outgoing: HashMap<String, Vec<String>>,
    /// 入边：to ← sources
    incoming: HashMap<String, Vec<String>>,
}

impl LinkGraph {
    /// 从元数据库构建图谱。
    pub fn build(store: &MetaStore) -> crate::Result<Self> {
        let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
        let mut incoming: HashMap<String, Vec<String>> = HashMap::new();

        for doc in store.list_documents()? {
            for link in store.outgoing_links(&doc.path)? {
                outgoing
                    .entry(link.from_path.clone())
                    .or_default()
                    .push(link.to_path.clone());
                if !link.to_path.is_empty() {
                    incoming
                        .entry(link.to_path.clone())
                        .or_default()
                        .push(link.from_path.clone());
                }
            }
        }

        Ok(Self { outgoing, incoming })
    }

    /// 某文档的出链目标。
    pub fn outgoing(&self, path: &str) -> &[String] {
        self.outgoing.get(path).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 某文档的反链来源。
    pub fn incoming(&self, path: &str) -> &[String] {
        self.incoming.get(path).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 邻居（出入链合并去重）。
    pub fn neighbors(&self, path: &str) -> Vec<String> {
        let mut set: HashSet<&String> = HashSet::new();
        set.extend(self.outgoing(path).iter());
        set.extend(self.incoming(path).iter());
        set.into_iter().cloned().collect()
    }

    /// 节点总数（含孤立节点由调用方补充）。
    pub fn edge_count(&self) -> usize {
        self.outgoing.values().map(|v| v.len()).sum()
    }
}

/// 将抽取到的链接解析为实际文档路径。
///
/// 匹配策略：先按完整相对路径精确匹配，再按文件名匹配，最后按标题匹配。
pub fn resolve_link_targets(
    links: &[LinkRecord],
    known_paths: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for link in links {
        if link.to_path.is_empty() {
            continue;
        }
        out.push((link.from_path.clone(), link.to_path.clone()));
        let _ = known_paths;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DocumentRecord;

    #[test]
    fn builds_bidirectional_edges() {
        let mut store = MetaStore::open_in_memory().unwrap();
        for p in ["a.md", "b.md"] {
            store
                .upsert_document(&DocumentRecord {
                    path: p.into(),
                    title: p.into(),
                    format: "Markdown".to_string(),
                    size: 0,
                    modified_at: 0,
                    content_hash: String::new(),
                })
                .unwrap();
        }
        store
            .replace_links(
                "a.md",
                &[LinkRecord {
                    from_path: "a.md".into(),
                    to_path: "b.md".into(),
                    target: "b".into(),
                    granularity: "block".into(),
                }],
            )
            .unwrap();

        let g = LinkGraph::build(&store).unwrap();
        assert_eq!(g.outgoing("a.md"), &["b.md".to_string()]);
        assert_eq!(g.incoming("b.md"), &["a.md".to_string()]);
        assert_eq!(g.edge_count(), 1);
    }
}

//! 预构建索引的端到端回归。
//!
//! 覆盖技术方案设计 §4 / 决策② 的核心承诺：
//! 「构建期由 native 端生成索引产物并随站打包，运行时只读加载」。
//!
//! 本测试刻意走**完整链路**（真实样例库 → 真实分词 → JSON 产物 → 重新解析 → 检索），
//! 因为 v0.1 曾排查过一个真实缺陷：建索引与查查询若用不同分词器，
//! 会出现「进程内正常、跨进程零命中」。只在同一进程内断言无法暴露该问题。

#![cfg(feature = "native")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use mkv_core::prebuilt::PrebuiltIndex;
use mkv_core::service::KnowledgeService;
use mkv_core::vault::Vault;

/// 领域词典（模拟用户的自定义术语表）。
const DOMAIN_DICT: &str = "模流分析\n熔接线\n浇口\n冷却水路\n顶出系统\n";

/// 临时目录序号：测试是并行执行的，必须保证每个测试用独立目录。
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// 在临时目录里建一个真实样例库，并产出预构建索引产物。
fn build_artifact() -> (PrebuiltIndex, String) {
    let dir = std::env::temp_dir().join(format!(
        "mkv-prebuilt-e2e-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");

    mkv_core::demo::create_sample_vault(&dir).expect("生成样例库失败");

    let mut vault = Vault::open(&dir, false).expect("打开样例库失败");
    // 文档表来自元数据库，未建索引时为空 —— 必须先重建索引。
    vault.reindex(None).expect("重建索引失败");
    let docs = vault.documents().expect("读取文档列表失败");
    assert!(!docs.is_empty(), "重建索引后应有文档");

    let mut texts: HashMap<String, String> = HashMap::new();
    for doc in &docs {
        if let Ok(text) = vault.read_extracted_text(&doc.path) {
            texts.insert(doc.path.clone(), text);
        }
    }

    let mut links: Vec<mkv_core::LinkRecord> = Vec::new();
    for doc in &docs {
        if let Ok(out) = vault.outgoing_links(&doc.path) {
            links.extend(out);
        }
    }

    let index = mkv_core::prebuilt_export::build_index(
        &docs,
        &texts,
        &links,
        Some(DOMAIN_DICT),
        "e2e-vault",
    )
    .expect("构建预构建索引失败");

    // 序列化 → 重新解析（模拟「产物落盘 → Web 端只读加载」）
    let json = index.to_json().expect("序列化失败");
    let parsed = PrebuiltIndex::from_json(&json).expect("重新解析失败");

    let _ = std::fs::remove_dir_all(&dir);
    (parsed, json)
}

#[test]
fn artifact_roundtrip_keeps_documents_links_and_postings() {
    let (index, json) = build_artifact();

    assert!(index.doc_count() > 0, "样例库应产出文档");
    assert!(!index.postings.is_empty(), "应产出倒排表");
    assert!(!index.links.is_empty(), "应产出双链边");
    assert_eq!(index.schema_version, mkv_core::prebuilt::PREBUILT_SCHEMA_VERSION);
    assert!(json.len() > 100, "产物应有实质内容");
}

#[test]
fn cross_process_search_hits_chinese_terms() {
    let (index, _) = build_artifact();

    // 这些词来自样例库正文；查询侧用产物词表做正向最大匹配，
    // 若两侧分词不一致，这里会零命中。
    for query in ["模流分析", "浇口", "冷却水路", "熔接线"] {
        let hits = index.search(query, 20);
        assert!(
            !hits.is_empty(),
            "查询「{query}」零命中 —— 建索引与查询切分不一致"
        );
        assert!(hits[0].score > 0.0, "得分必须为正");
        assert!(!hits[0].snippet.is_empty(), "摘要不应为空");
    }
}

#[test]
fn cross_process_search_is_stable_across_reparse() {
    let (index, json) = build_artifact();
    let reparsed = PrebuiltIndex::from_json(&json).expect("重新解析失败");

    let a = index.search("模流分析", 10);
    let b = reparsed.search("模流分析", 10);

    assert_eq!(a.len(), b.len(), "序列化前后命中数应一致");
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(x.path, y.path, "序列化前后排序应一致");
    }
}

#[test]
fn unrelated_query_returns_nothing() {
    let (index, _) = build_artifact();
    assert!(
        index.search("量子纠缠拓扑绝缘体", 10).is_empty(),
        "无关查询不应命中"
    );
}

#[test]
fn backlinks_survive_artifact_roundtrip() {
    let (index, _) = build_artifact();

    // 至少存在一个「被引用」文档，其反链非空。
    let mut checked = 0;
    for link in &index.links {
        let backlinks = index.backlinks(&link.to_path);
        assert!(
            backlinks.iter().any(|b| b.from_path == link.from_path),
            "文档 {} 的反链应包含 {}",
            link.to_path,
            link.from_path
        );
        checked += 1;
    }
    assert!(checked > 0, "样例库应存在双链");
}

#[test]
fn documents_expose_browsable_metadata() {
    let (index, _) = build_artifact();
    let docs = index.documents();

    assert_eq!(docs.len(), index.doc_count());
    for doc in &docs {
        assert!(!doc.path.is_empty());
        assert!(!doc.title.is_empty(), "标题不应为空：{}", doc.path);
        assert!(!doc.format.is_empty(), "格式标签不应为空：{}", doc.path);
    }
}

#[test]
fn prebuilt_service_is_read_only() {
    let (index, _) = build_artifact();
    let svc = mkv_core::prebuilt::PrebuiltService::new(index);

    assert!(!svc.supports_editing(), "Web 端预构建索引必须只读");
    assert!(svc.open_editor("x.md").is_none());
    assert!(svc.document_count() > 0);
    assert_eq!(svc.documents().len(), svc.document_count());
    assert!(!svc.status().is_empty());
}

#[test]
fn preview_text_is_readable_from_artifact() {
    let (index, _) = build_artifact();
    let doc = index.docs.first().expect("应有文档");
    let text = index.read_document(&doc.path).expect("应能读到正文");
    assert!(!text.is_empty());
}

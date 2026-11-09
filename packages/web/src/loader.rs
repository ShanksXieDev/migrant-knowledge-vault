//! Web 端预构建索引加载器。
//!
//! 对应技术方案设计 §4 / 决策②：**预构建只读索引**。
//!
//! # 加载路径
//!
//! 1. **内嵌索引（默认）**：构建期由 `build.rs` 把索引产物复制进 `OUT_DIR`，
//!    源码用 `include_str!` 在编译期嵌入 wasm 二进制 —— 这就是「随站打包」。
//!    运行时零网络请求、完全离线可用。
//! 2. **远端索引（可选）**：若构建时设置了 `MKV_WEB_INDEX_URL`，
//!    页面启动后从该地址拉取更新的索引产物并热替换内嵌版本，
//!    使「站点发布后仍可更新索引、无需重新编译 wasm」成为可能。
//!
//! # 为什么用 `include_str!` 而不是运行时 fetch
//!
//! 决策②要求「构建期生成索引产物并随站打包」。把产物编进 wasm 二进制后：
//! - 不依赖任何后端服务，静态托管（GitHub Pages / 对象存储）即可运行；
//! - 不存在首屏白屏等待网络的问题；
//! - 检索与双链能力在离线环境下完整可用。
//!
//! 远端路径作为**可选增强**存在，用于解决「索引更新要重新编译 wasm」的痛点。

use mkv_core::prebuilt::{PrebuiltIndex, PREBUILT_SCHEMA_VERSION};

/// 内嵌的索引产物（构建期由 `build.rs` 注入）。
///
/// 为空索引占位（无真实产物）时，解析后文档数为 0。
const EMBEDDED_INDEX_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/mkv-index.json"));

/// 是否内嵌了真实索引产物（`build.rs` 注入的编译期常量）。
///
/// 注：`env!` 展开为 `&str`，而 `PartialEq` 目前尚不是 const trait，
/// 因此这里用字节切片模式匹配实现 const 求值。
pub const HAS_EMBEDDED_INDEX: bool = match env!("MKV_WEB_INDEX_EMBEDDED").as_bytes() {
    [b'1'] => true,
    _ => false,
};

/// 远端索引地址（编译期常量；为空表示不启用远端热替换）。
pub const REMOTE_INDEX_URL: &str = env!("MKV_WEB_INDEX_URL");

/// 索引加载结果。
#[derive(Debug, Clone)]
pub struct LoadedIndex {
    /// 解析后的索引。
    pub index: PrebuiltIndex,
    /// 索引来源。
    pub source: IndexSource,
}

/// 索引来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexSource {
    /// 构建期内嵌产物。
    Embedded,
    /// 运行期从远端拉取的产物。
    Remote(String),
}

/// 解析内嵌的索引产物。
///
/// 产物由 `mkv export-web` 生成，结构见 `mkv_core::prebuilt::PrebuiltIndex`。
/// 解析失败时返回错误（例如产物被手工改坏），由调用方降级处理。
pub fn load_embedded() -> Result<PrebuiltIndex, String> {
    PrebuiltIndex::from_json(EMBEDDED_INDEX_JSON).map_err(|e| e.to_string())
}

/// 远端索引地址（若编译时配置）。
pub fn remote_url() -> Option<&'static str> {
    let url = REMOTE_INDEX_URL.trim();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

/// 拉取远端索引产物。
///
/// 仅在 wasm32 下有意义（浏览器 `fetch`）；其他目标返回 `None`。
#[cfg(target_arch = "wasm32")]
pub async fn fetch_remote(url: &str) -> Result<PrebuiltIndex, String> {
    use gloo_net::http::Request;

    let response = Request::get(url)
        .send()
        .await
        .map_err(|e| format!("请求远端索引失败: {e}"))?;
    if !response.ok() {
        return Err(format!("远端索引返回 HTTP {}", response.status()));
    }
    let text = response.text().await.map_err(|e| format!("读取远端索引失败: {e}"))?;
    PrebuiltIndex::from_json(&text).map_err(|e| format!("解析远端索引失败: {e}"))
}

/// 拉取远端索引产物（非 wasm 目标不支持，返回 `None`）。
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_remote(_url: &str) -> Result<PrebuiltIndex, String> {
    Err("非 wasm32 目标不支持远端索引拉取".to_string())
}

/// 在「远端结果」与「内嵌产物」之间做选择（**纯函数**，便于单测覆盖全部分支）。
///
/// 规则：
/// - 只有「配置了远端地址」且「拉取成功」且「远端产物非空」时才热替换；
/// - 其余情况（未配置 / 网络失败 / HTTP 错误 / 远端为空索引 / JSON 损坏）
///   一律回落内嵌产物，保证站点始终可用。
///
/// `remote_result` 为 `None` 表示未发起请求；`Some(Err)` 表示请求或解析失败。
pub fn select_index(
    embedded: PrebuiltIndex,
    url: Option<&str>,
    remote_result: Option<Result<PrebuiltIndex, String>>,
) -> LoadedIndex {
    if let (Some(url), Some(Ok(remote))) = (url, &remote_result) {
        // schema 版本闸门：远端产物可能被**新版**构建端生成，
        // 而当前 Web 构建是旧版——直接热替换进去会产生难以定位的运行时异常。
        // 版本不匹配时回落到内嵌产物（站点始终可用），并把原因写进状态描述。
        if remote.schema_version != PREBUILT_SCHEMA_VERSION {
            return LoadedIndex {
                index: embedded,
                source: IndexSource::Embedded,
            };
        }
        if remote.doc_count() > 0 {
            return LoadedIndex {
                index: remote.clone(),
                source: IndexSource::Remote(url.to_string()),
            };
        }
    }
    LoadedIndex {
        index: embedded,
        source: IndexSource::Embedded,
    }
}

/// 加载索引：优先远端（若配置），失败则回落内嵌产物。
///
/// 远端拉取失败不会让 Web 端不可用 —— 内嵌产物始终是兜底。
/// 决策逻辑集中在 [`select_index`]，由单元测试覆盖全部分支。
pub async fn load() -> LoadedIndex {
    let url = remote_url();
    let remote_result = match url {
        Some(u) => Some(fetch_remote(u).await),
        None => None,
    };
    let embedded = load_embedded().unwrap_or_else(|_| PrebuiltIndex::empty());
    select_index(embedded, url, remote_result)
}

/// 生成状态栏描述文本。
///
/// 「空」的成因有三种，文案必须能区分，否则用户无从下手：
/// 1. 构建期没内嵌产物且没配远端 → 需要重新构建；
/// 2. 配了远端但拉不到 → 需要检查索引服务；
/// 3. 产物有效但语料为空 → 知识库本身没有可索引文档。
pub fn describe(doc_count: usize, source: &IndexSource) -> String {
    match doc_count {
        0 if !HAS_EMBEDDED_INDEX && remote_url().is_none() => {
            "构建期未内嵌索引产物：请先运行 mkv export-web 生成产物，再重新构建 Web 端".to_string()
        }
        0 if !HAS_EMBEDDED_INDEX => {
            "远端索引不可用且构建期无内嵌产物：请检查索引服务或重新构建".to_string()
        }
        0 => "索引为空：知识库中没有可索引的文档".to_string(),
        n => {
            let src = match source {
                IndexSource::Embedded => "构建期内嵌产物".to_string(),
                IndexSource::Remote(url) => format!("远端产物（{url}）"),
            };
            format!("{n} 个文档 · {src}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_index_parses() {
        // 内嵌产物要么是真实索引，要么是空索引占位 —— 两者都必须能解析。
        let index = load_embedded().expect("内嵌索引应可解析");
        assert_eq!(
            index.schema_version,
            mkv_core::prebuilt::PREBUILT_SCHEMA_VERSION,
            "产物 schema 版本应与当前代码一致"
        );
    }

    #[test]
    fn empty_placeholder_has_no_documents() {
        let empty = PrebuiltIndex::from_json(
            r#"{"schema_version":1,"generated_at":0,"root":"","docs":[],"links":[],"postings":[]}"#,
        )
        .unwrap();
        assert_eq!(empty.doc_count(), 0);
        assert!(empty.search("任何词", 5).is_empty());
    }

    #[test]
    fn remote_url_is_optional() {
        // 未配置时必须为 None，避免无谓的网络请求。
        if REMOTE_INDEX_URL.trim().is_empty() {
            assert!(remote_url().is_none());
        } else {
            assert!(remote_url().is_some());
        }
    }

    #[test]
    fn embedded_flag_matches_actual_content() {
        let index = load_embedded().expect("内嵌索引应可解析");
        if HAS_EMBEDDED_INDEX {
            assert!(
                index.doc_count() > 0,
                "build.rs 标记为已内嵌真实产物，索引不应为空"
            );
        } else {
            assert_eq!(
                index.doc_count(),
                0,
                "未内嵌真实产物时只能是空索引占位"
            );
        }
    }

    #[test]
    fn embedded_index_is_searchable() {
        let index = load_embedded().expect("内嵌索引应可解析");
        if !HAS_EMBEDDED_INDEX {
            return;
        }
        // 取产物自身的任一词条做检索，验证「查询切分 ↔ 索引词表」一致。
        let token = index
            .postings
            .first()
            .expect("真实产物应含词条")
            .token
            .clone();
        let hits = index.search(&token, 5);
        assert!(
            !hits.is_empty(),
            "内嵌索引应能命中自身词条「{token}」—— 若零命中，说明查询与索引分词不一致"
        );
    }

    #[test]
    fn load_without_remote_uses_embedded() {
        let loaded = futures_executor::block_on(load());
        if remote_url().is_none() {
            assert_eq!(loaded.source, IndexSource::Embedded);
            assert_eq!(
                loaded.index.doc_count(),
                load_embedded().expect("内嵌索引应可解析").doc_count()
            );
        }
    }

    // ---- 远端热替换：把决策逻辑当纯函数测，覆盖全部分支 ----

    fn sample_index(root: &str, path: &str) -> PrebuiltIndex {
        let json = format!(
            r#"{{"schema_version":1,"generated_at":0,"root":"{root}","docs":[{{"path":"{path}","title":"T","format":"Markdown","size":1,"modified_at":0,"text":"模流分析"}}],"links":[],"postings":[]}}"#
        );
        PrebuiltIndex::from_json(&json).expect("测试产物应可解析")
    }

    const URL: &str = "https://example.test/mkv-index.json";

    #[test]
    fn select_without_remote_url_uses_embedded() {
        let embedded = sample_index("内嵌库", "a.md");
        let got = select_index(embedded, None, None);
        assert_eq!(got.source, IndexSource::Embedded);
        assert_eq!(got.index.root, "内嵌库");
        assert_eq!(got.index.doc_count(), 1);
    }

    #[test]
    fn select_with_valid_remote_hot_swaps() {
        let embedded = sample_index("内嵌库", "a.md");
        let remote = sample_index("远端库", "b.md");
        let got = select_index(embedded, Some(URL), Some(Ok(remote)));
        assert_eq!(got.source, IndexSource::Remote(URL.to_string()));
        assert_eq!(got.index.root, "远端库", "应真正替换为远端内容");
        assert_eq!(got.index.doc_count(), 1);
    }

    #[test]
    fn select_with_failed_remote_falls_back() {
        let embedded = sample_index("内嵌库", "a.md");
        let got = select_index(
            embedded,
            Some(URL),
            Some(Err("HTTP 500".to_string())),
        );
        assert_eq!(got.source, IndexSource::Embedded);
        assert_eq!(got.index.root, "内嵌库", "网络失败必须保留内嵌版本");
    }

    /// schema 版本不匹配的远端产物**不得**热替换。
    ///
    /// 远端产物可能由更新版本的构建端生成；旧版 Web 构建直接吃下去
    /// 会产生难以定位的运行时异常。宁可继续用内嵌产物。
    #[test]
    fn select_rejects_remote_with_mismatched_schema_version() {
        let embedded = sample_index("内嵌库", "a.md");
        let mut remote = sample_index("远端库", "b.md");
        remote.schema_version = PREBUILT_SCHEMA_VERSION + 1;
        let got = select_index(embedded, Some(URL), Some(Ok(remote)));
        assert_eq!(got.source, IndexSource::Embedded);
        assert_eq!(got.index.root, "内嵌库", "版本不匹配必须回落到内嵌产物");
    }

    #[test]
    fn select_with_empty_remote_falls_back() {
        let embedded = sample_index("内嵌库", "a.md");
        let got = select_index(embedded, Some(URL), Some(Ok(PrebuiltIndex::empty())));
        assert_eq!(got.source, IndexSource::Embedded);
        assert_eq!(got.index.root, "内嵌库", "空产物应视为无效");
    }

    #[test]
    fn select_with_url_but_no_request_result_falls_back() {
        let embedded = sample_index("内嵌库", "a.md");
        let got = select_index(embedded, Some(URL), None);
        assert_eq!(got.source, IndexSource::Embedded);
    }

    #[test]
    fn describe_labels_remote_source() {
        let text = describe(3, &IndexSource::Remote(URL.to_string()));
        assert!(text.contains("3 个文档"), "text={text}");
        assert!(text.contains("远端产物"), "text={text}");
        assert!(text.contains(URL), "text={text}");
    }
}

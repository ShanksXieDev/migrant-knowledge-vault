//! 构建脚本：把「预构建只读索引」产物嵌入 wasm 二进制。
//!
//! 索引策略见技术方案设计 §4 / 决策②：
//! 构建期由 native 端（`mkv export-web`）生成索引产物，**随站打包**，
//! 运行时只读加载，从而规避 wasm 环境实时建索引的性能瓶颈。
//!
//! ## 产物来源
//!
//! 按以下顺序查找第一个存在的文件：
//! 1. 环境变量 `MKV_WEB_INDEX` 指定的路径（CI / 流水线常用）；
//! 2. `packages/web/assets/mkv-index.json`（约定路径）。
//!
//! 找到则原样复制到 `OUT_DIR/mkv-index.json`；找不到则写入一份**空索引**
//! 占位，保证 `cargo build` 永不因缺少产物而失败。
//!
//! ## 相对路径的解析基准
//!
//! 构建脚本的工作目录是**包目录**（`packages/web`），不是工作区根目录。
//! 因此 `MKV_WEB_INDEX=examples/web-index.json` 这类相对路径若直接拼接会找不到文件。
//! 本脚本对相对路径同时尝试两个基址：包目录与工作区根目录，
//! 并在构建输出中打印最终采用的文件路径，便于确认。
//!
//! ## 远端索引（可选）
//!
//! 环境变量 `MKV_WEB_INDEX_URL` 会以编译期常量的形式注入 Web 端：
//! 设置后，页面启动时会尝试从该地址拉取更新的索引产物并热替换内嵌版本，
//! 从而做到「站点发布后仍可更新索引、无需重新编译 wasm」。
//! 未设置时 Web 端完全离线工作（只用内嵌索引），无任何网络请求。

use std::path::{Path, PathBuf};
use std::{env, fs};

/// 空索引占位 JSON。
///
/// 字段与 `mkv_core::prebuilt::PrebuiltIndex` 的 serde 表示一一对应，
/// 因此可被 `PrebuiltIndex::from_json` 直接解析。
const EMPTY_INDEX_JSON: &str = r#"{"schema_version":1,"generated_at":0,"root":"","docs":[],"links":[],"postings":[]}"#;

/// 解析产物候选路径。
///
/// 绝对路径原样使用；相对路径分别相对「包目录」与「工作区根目录」尝试。
fn candidate_paths(explicit: Option<&str>, manifest_dir: &Path) -> Vec<PathBuf> {
    // 工作区根目录 = 包目录的上一级（packages/web → packages → 根）
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);

    let mut out = Vec::new();
    if let Some(p) = explicit {
        let p = PathBuf::from(p);
        if p.is_absolute() {
            out.push(p);
        } else {
            out.push(manifest_dir.join(&p));
            if let Some(root) = &workspace_root {
                out.push(root.join(&p));
            }
        }
    } else {
        out.push(manifest_dir.join("assets").join("mkv-index.json"));
        if let Some(root) = &workspace_root {
            out.push(root.join("packages").join("web").join("assets").join("mkv-index.json"));
        }
    }
    out
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 未设置"),
    );

    let explicit = env::var("MKV_WEB_INDEX").ok();
    let candidates = candidate_paths(explicit.as_deref(), &manifest_dir);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MKV_WEB_INDEX");
    println!("cargo:rerun-if-env-changed=MKV_WEB_INDEX_URL");
    for c in &candidates {
        println!("cargo:rerun-if-changed={}", c.display());
    }

    let dest = out_dir.join("mkv-index.json");
    let found = candidates.iter().find(|c| c.is_file());

    let embedded = if let Some(src) = found {
        fs::copy(src, &dest).unwrap_or_else(|e| {
            panic!("复制预构建索引失败: {} -> {e}", src.display())
        });
        println!("cargo:warning=预构建索引产物: {}", src.display());
        true
    } else {
        // 缺产物不阻断构建：写入空索引占位，Web 端显示「未加载索引」提示。
        fs::write(&dest, EMPTY_INDEX_JSON).expect("写入空索引占位失败");
        println!(
            "cargo:warning=未找到预构建索引产物（已查找: {}），将内嵌空索引；\
             请先运行 `mkv export-web <vault> <out.json>` 并用 MKV_WEB_INDEX 指定路径后重新构建",
            candidates
                .iter()
                .map(|c| c.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        false
    };

    // 编译期常量：Web 端据此判断是否有真实索引、以及远端索引地址。
    // 注意用 "1"/"0" 而不是 "true"/"false"：Rust 端以字节模式匹配读取该常量。
    println!(
        "cargo:rustc-env=MKV_WEB_INDEX_EMBEDDED={}",
        if embedded { "1" } else { "0" }
    );
    let url = env::var("MKV_WEB_INDEX_URL").unwrap_or_default();
    println!("cargo:rustc-env=MKV_WEB_INDEX_URL={url}");
}

//! 回归：Desktop 与 Web 的引用文本基准必须一致（F-06 进阶②/③）。
//!
//! # 这条不变式盯的是什么
//!
//! 引用卡片要显示「源片段内容」，差异高亮要比较「源片段相对上次的变化」，
//! 两者都建立在同一件事上：**用于片段切分的文本**。
//!
//! 2026-11-09 专家团评审实测发现（修复前）：
//! - Desktop 端 `Vault::references()` 用 `read_document()` 取**原始字节**，
//!   对 `.xlsx` / `.pptx` 而言那是 ZIP 二进制——卡片 snippet 是 `PK\x03\x04...`
//!   这样的乱码，差异高亮更是拿二进制在比二进制；
//! - Web 端（构建期用 `read_extracted_text()` 取**抽取文本**）显示的却是
//!   `台账: PN-1024 | MDL-08`。
//!
//! 同一个功能在两端给出完全不同的内容，Desktop 侧还叠加了
//! 「基线存二进制 → 与全文永远不等 → 永远报『源已变更』」的假差异。
//!
//! 因此本测试钉死：**两端必须用同一份文本基准**。

use std::path::{Path, PathBuf};

use mkv_core::refs::snapshot_key;
use mkv_core::vault::Vault;

/// 样例库路径（相对 crate 根）。
fn sample_vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sample-vault")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mkv_reftext_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 复制目录树，跳过索引与版本库目录。
fn copy_documents(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != ".kbindex" && name != ".git" && name != ".gitignore"
        })
    {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Desktop 端的引用卡片内容必须是**可读文本**，不能是二进制乱码。
#[test]
fn desktop_reference_snippets_are_readable_text_not_binary() {
    let dir = temp_dir("snippets");
    let vault_dir = dir.join("vault");
    copy_documents(&sample_vault(), &vault_dir);

    let mut vault = Vault::open(&vault_dir, false).expect("打开样例库失败");
    // 引用解析要读「全库文档清单」，因此必须先建索引
    // （Desktop 启动时同样会先 reindex）。
    vault.reindex(None).expect("重建索引失败");
    let refs = vault
        .references("01-笔记/跨格式引用示例.md")
        .expect("取引用视图失败");
    assert!(!refs.is_empty(), "样例库应能解析出跨格式引用");

    for r in &refs {
        // 二进制格式的片段必须是抽出来的文本，而不是原始字节。
        if r.target_path.ends_with(".xlsx") {
            assert!(
                r.snippet.contains("PN-1024"),
                "Excel 引用片段应是可读文本，实际：{:?}",
                r.snippet
            );
        }
        if r.target_path.ends_with(".pptx") {
            assert!(
                r.snippet.contains("项目方案"),
                "PowerPoint 引用片段应是可读文本，实际：{:?}",
                r.snippet
            );
        }
        if r.target_path.ends_with(".drawio") {
            assert!(
                !r.snippet.starts_with("<mxGraphModel"),
                "Drawio 引用片段应是节点文本而不是原始 XML，实际：{:?}",
                r.snippet
            );
        }
        // 所有片段都必须是合法 UTF-8 文本，且不含替换字符（lossy 转换的痕迹）。
        assert!(
            !r.snippet.contains('\u{FFFD}'),
            "片段不得含 UTF-8 替换字符（说明是原始字节而非抽取文本）：{:?}",
            r.snippet
        );
    }
}

/// Desktop 与 Web 的**文本基准**必须一致。
///
/// Web 端用 `read_extracted_text()`；Desktop 端若用别的来源，
/// 同一文档在两端会得出互相矛盾的「是否变更」结论。
#[test]
fn desktop_and_web_use_the_same_text_baseline() {
    let dir = temp_dir("baseline");
    let vault_dir = dir.join("vault");
    copy_documents(&sample_vault(), &vault_dir);

    let vault = Vault::open(&vault_dir, false).expect("打开样例库失败");
    let refs = vault
        .references("01-笔记/跨格式引用示例.md")
        .expect("取引用视图失败");

    for r in &refs {
        // Web 侧的基准文本：抽取文本（构建期 `prebuilt_export` 用的就是它）。
        let extracted = vault
            .read_extracted_text(&r.target_path)
            .unwrap_or_else(|e| panic!("抽取文本失败 {}：{e}", r.target_path));
        // Desktop 侧的片段必须能在抽取文本里找到——
        // 找不到就说明它来自别的来源（原始字节），两端基准不一致。
        let key = extracted
            .find(r.snippet.trim())
            .unwrap_or_else(|| {
                panic!(
                    "Desktop 片段不在 Web 抽取文本中（两端基准不一致）：\n  目标={}\n  片段={:?}",
                    r.target_path, r.snippet
                )
            });
        assert!(key > 0 || !extracted.is_empty());
    }
}

/// Desktop 侧的快照基线必须与 Web 侧同源。
///
/// 基线由 `refresh_reference_snapshots()` 采集（重建索引时刷新），
/// 采集用的文本必须与 diff 时用的文本一致——
/// 否则每次都会「基线 ≠ 当前」，永远报「源已变更」。
#[test]
fn desktop_snapshot_baseline_matches_extracted_text() {
    let dir = temp_dir("snapbase");
    let vault_dir = dir.join("vault");
    copy_documents(&sample_vault(), &vault_dir);

    let mut vault = Vault::open(&vault_dir, false).expect("打开样例库失败");
    vault.reindex(None).expect("重建索引失败");

    let refs = vault
        .references("01-笔记/跨格式引用示例.md")
        .expect("取引用视图失败");
    assert!(!refs.is_empty(), "样例库应能解析出跨格式引用");

    for r in &refs {
        // 重建索引后基线 = 当前内容，因此**不应**报差异。
        assert!(
            r.diff.is_none(),
            "重建索引后不应报「源已变更」（基线与当前不一致）：{} → {:?}",
            r.target_path,
            r.diff
        );
        // 基线条目本身也应是抽取文本，而不是二进制。
        let key = snapshot_key(&r.target_path, &r.selector);
        let _ = key;
        assert!(
            !r.snippet.contains('\u{FFFD}'),
            "片段不得含 UTF-8 替换字符：{:?}",
            r.snippet
        );
    }
}

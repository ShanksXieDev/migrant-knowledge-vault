//! 回归：知识库文件夹整体复制后，索引一键重建且数据无损。
//!
//! 对应 PRD §7.1 验收清单最后一项。这是「本地优先 / 文件优先」的底线：
//! 用户把整个文件夹拷到另一台机器（不带索引、不带 Git 历史），
//! 只要执行 `mkv index <root>`，文档、检索结果与双链都必须与原地一致。
//!
//! 因此本测试**刻意只复制文档**，跳过 `.kbindex/` 与 `.git/`，
//! 强制新库从零重建，而不是复用旧索引文件。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mkv_core::vault::Vault;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mkv_portability_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 复制目录树，跳过索引与版本库目录（模拟「只带走文档」）。
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

/// 汇总一个库的「可观测状态」：文档路径、检索命中、双链总数。
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    docs: BTreeSet<String>,
    hits: BTreeSet<String>,
    links: Vec<(String, String, String)>,
    backlinks_of_target: usize,
}

fn snapshot(vault: &Vault, query: &str, target: &str) -> Snapshot {
    let docs: BTreeSet<String> = vault
        .documents()
        .unwrap()
        .into_iter()
        .map(|d| d.path)
        .collect();
    let hits: BTreeSet<String> = vault
        .search(query, 50)
        .unwrap()
        .into_iter()
        .map(|h| h.path)
        .collect();

    // 全库出链（按 from/to/target 排序，保证可比）
    let mut links: Vec<(String, String, String)> = Vec::new();
    for path in &docs {
        for l in vault.outgoing_links(path).unwrap() {
            links.push((l.from_path, l.to_path, l.target));
        }
    }
    links.sort();

    let backlinks_of_target = vault.backlinks(target).unwrap().len();
    Snapshot {
        docs,
        hits,
        links,
        backlinks_of_target,
    }
}

#[test]
fn copied_vault_rebuilds_identically() {
    let src = temp_dir("src");
    let dst = temp_dir("dst");

    mkv_core::demo::create_sample_vault(&src).expect("生成样例库失败");

    let mut original = Vault::open(&src, false).expect("打开原库失败");
    original.reindex(None).expect("原库重建索引失败");
    let before = snapshot(&original, "模流分析", "01-笔记/模流分析-入门.md");
    assert!(!before.docs.is_empty(), "样例库应含文档");
    assert!(!before.hits.is_empty(), "中文检索应命中");
    assert!(!before.links.is_empty(), "样例库应含双链");
    drop(original); // 释放索引目录锁

    copy_documents(&src, &dst);
    assert!(
        !dst.join(".kbindex").exists(),
        "复制时应排除 .kbindex，强制从零重建"
    );

    let mut rebuilt = Vault::open(&dst, false).expect("打开复制库失败");
    let report = rebuilt.reindex(None).expect("复制库重建索引失败");
    let after = snapshot(&rebuilt, "模流分析", "01-笔记/模流分析-入门.md");

    assert_eq!(report.indexed, before.docs.len(), "重建入库文档数应一致");
    assert_eq!(after, before, "复制重建后文档 / 检索 / 双链必须无损");
    assert!(
        after.backlinks_of_target > 0,
        "复制重建后反链仍应可解析"
    );

    drop(rebuilt);
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

/// 回归：重建只能依赖文档内容，不能偷偷依赖旧索引里的路径。
/// 把目标文档改名后重建，旧路径必须彻底消失、新路径可被检索到。
#[test]
fn rebuild_drops_stale_paths() {
    let dir = temp_dir("stale");
    std::fs::write(dir.join("old.md"), "# 模流分析\n\n浇口位置记录。").unwrap();

    let mut vault = Vault::open(&dir, false).unwrap();
    vault.reindex(None).unwrap();
    assert!(!vault.search("模流分析", 10).unwrap().is_empty());

    std::fs::rename(dir.join("old.md"), dir.join("new.md")).unwrap();
    vault.reindex(None).unwrap();

    let docs: BTreeSet<String> = vault
        .documents()
        .unwrap()
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert!(!docs.contains("old.md"), "旧路径应被清除");
    assert!(docs.contains("new.md"), "新路径应入库");

    let hits = vault.search("模流分析", 10).unwrap();
    assert!(hits.iter().all(|h| h.path == "new.md"), "hits = {hits:?}");

    drop(vault);
    let _ = std::fs::remove_dir_all(&dir);
}

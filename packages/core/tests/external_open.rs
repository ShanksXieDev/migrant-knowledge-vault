//! F-08 外部唤起：路径安全与「系统默认应用」路由的回归测试。
//!
//! 这里只做**纯构造**（`external_open_target`），不真的启动 Office / Visio，
//! 因此可在 CI 的 Linux runner 上稳定跑；实际进程唤起由手工验收。

use mkv_core::editor::EditorTarget;
use mkv_core::vault::Vault;
use mkv_core::Error;

fn sample_vault(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mkv_external_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    mkv_core::demo::create_sample_vault(&dir).unwrap();
    dir
}

#[test]
fn external_target_resolves_to_real_file_inside_vault() {
    let dir = sample_vault("ok");
    let vault = Vault::open(&dir, false).unwrap();

    let target = vault
        .external_open_target("03-台账/模具台账.xlsx")
        .expect("存在的受支持文档应可构造外部唤起目标");
    match target {
        EditorTarget::External { program, path } => {
            assert!(program.is_empty(), "默认必须使用系统默认应用");
            assert!(path.exists(), "目标文件应真实存在: {}", path.display());
            assert!(path.starts_with(&dir), "不得解析到知识库之外");
        }
        _ => panic!("expected external target"),
    }

    drop(vault);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn external_target_rejects_path_escape_and_missing_file() {
    let dir = sample_vault("bad");
    let vault = Vault::open(&dir, false).unwrap();

    // 路径逃逸：`FsStorage` 本身会拒绝，这里再守一层明确报错
    let err = vault.external_open_target("../outside.xlsx").unwrap_err();
    assert!(
        matches!(err, Error::Config(_)),
        "路径逃逸应被拒绝，实际: {err:?}"
    );

    // 不支持的格式
    let err = vault.external_open_target("notes.txt").unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "实际: {err:?}");

    // 文件不存在
    let err = vault
        .external_open_target("01-笔记/不存在的笔记.md")
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "实际: {err:?}");

    drop(vault);
    let _ = std::fs::remove_dir_all(&dir);
}

//! Migrant Knowledge Vault 命令行工具。
//!
//! v0.1 提供基础运维能力：初始化知识库、重建索引、全文检索、
//! 查看双链、Git 提交与历史，以及一键生成示例知识库。
//!
//! 用法：
//! ```text
//! mkv init   <root>              # 初始化知识库（.kbindex/ + Git 仓库）
//! mkv demo   <root>              # 生成示例知识库并建立索引
//! mkv index  <root>              # 重建全文索引与元数据
//! mkv search <root> <query>      # 全文检索（中文分词）
//! mkv links  <root> <path>       # 查看某文档的反链与出链
//! mkv docs   <root>              # 列出全部文档
//! mkv commit <root> -m <msg>     # 提交一次 Git 版本
//! mkv log    <root>              # 查看提交历史
//! mkv dict   <root> <dict.txt>   # 载入自定义领域词典并重建索引
//! mkv export-web <root> <out.json> [dict.txt]  # 生成 Web 端预构建只读索引
//! mkv watch  <root> [选项]       # 监听文件变化，增量更新索引与双链
//! ```

use anyhow::{Context, Result};
use mkv_core::vault::Vault;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if let Err(e) = run() {
        eprintln!("[mkv] 错误: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // CLI 也会用到内置编辑器路由（测试 / 运维），启动时注入插件。
    let _ = mkv_editors::install();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        print_help();
        return Ok(());
    };
    let rest = &args[1..];

    match cmd {
        "init" => cmd_init(root_arg(rest)?, true),
        "demo" => cmd_demo(root_arg(rest)?),
        "index" => cmd_index(root_arg(rest)?),
        "search" => cmd_search(root_arg(rest)?, query_arg(rest)?),
        "links" => cmd_links(root_arg(rest)?, path_arg(rest)?),
        "docs" => cmd_docs(root_arg(rest)?),
        "commit" => cmd_commit(root_arg(rest)?, rest),
        "log" => cmd_log(root_arg(rest)?),
        "dict" => cmd_dict(root_arg(rest)?, path_arg(rest)?),
        "export-web" => cmd_export_web(root_arg(rest)?, rest),
        "watch" => cmd_watch(root_arg(rest)?, rest),
        "editors" => cmd_editors(rest),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("[mkv] 未知命令: {other}");
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    println!(
        r#"Migrant Knowledge Vault CLI

用法：
  mkv init   <root>              初始化知识库（.kbindex/ + Git 仓库）
  mkv demo   <root>              生成示例知识库并建立索引
  mkv index  <root>              重建全文索引与元数据
  mkv search <root> <query>      全文检索（中文分词）
  mkv links  <root> <path>       查看某文档的反链与出链
  mkv docs   <root>              列出全部文档
  mkv commit <root> -m <msg>     提交一次 Git 版本
  mkv log    <root>              查看提交历史
  mkv dict   <root> <dict.txt>   载入自定义领域词典并重建索引
  mkv export-web <root> <out.json> [dict.txt]
                               生成 Web 端预构建只读索引产物
  mkv watch  <root> [选项]       监听文件变化，增量更新索引与双链
       --debounce <ms>          去抖窗口（默认 250）
       --auto-commit            每批变更后自动 Git 提交（默认关闭）
  mkv editors plugins           列出已安装的编辑器插件与对齐的上游 rev
  mkv editors check <root>      检测三个上游 Craft 仓库是否有新提交
  mkv editors confirm <root>    无 GUI 时确认升级全部待更新插件
  mkv editors skip <root>       跳过当前检测到的上游版本
  mkv editors upgrade <root>    同步用户已确认的上游 rev 并清除待重建标记
"#
    );
}

/// 编辑器插件运维：查看插件、检测上游、应用用户已确认的升级。
fn cmd_editors(rest: &[String]) -> Result<()> {
    use mkv_core::editor_updates as updates;

    let sub = rest.first().map(|s| s.as_str()).unwrap_or("plugins");
    match sub {
        "plugins" => {
            let registry = mkv_core::plugin::registry()
                .read()
                .map_err(|_| anyhow::anyhow!("插件注册表锁已损坏"))?;
            println!("已安装的编辑器插件：");
            for d in registry.descriptors() {
                let short = &d.upstream_rev[..d.upstream_rev.len().min(12)];
                println!(
                    "  {:<10} {:<6} backend={:<8} upstream={}@{} contract=v{}",
                    d.id, d.format_key, d.backend, d.upstream_repo, short, d.contract_version
                );
            }
            Ok(())
        }
        "check" => {
            let root = root_arg(&rest[1..])?;
            let lock = updates::load_state(&root);
            let remote = updates::detect_remote_revisions(&lock);
            let plan = updates::plan_updates(&lock, &remote);
            println!("上游检测（{} 个插件）：", lock.plugins.len());
            if plan.items.is_empty() {
                println!("  全部与锁文件一致，或离线导致探测失败（不视为错误）");
            }
            for item in &plan.items {
                println!(
                    "  {:<10} {} -> {}{}",
                    item.id,
                    &item.from_rev[..12.min(item.from_rev.len())],
                    &item.to_rev[..12.min(item.to_rev.len())],
                    if item.skipped { "（已跳过）" } else { "" }
                );
            }
            println!("可升级插件：{} 个", plan.actionable_ids().len());
            Ok(())
        }
        "confirm" | "skip" => {
            let root = root_arg(&rest[1..])?;
            let mut lock = updates::load_state(&root);
            let remote = updates::detect_remote_revisions(&lock);
            let plan = updates::plan_updates(&lock, &remote);
            let ids = plan.actionable_ids();
            if ids.is_empty() {
                println!("没有待确认的上游更新。");
                return Ok(());
            }
            let choice = if sub == "confirm" {
                updates::UpdateChoice::Upgrade { ids: ids.clone() }
            } else {
                updates::UpdateChoice::Skip { ids: ids.clone() }
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| format!("unix:{}", d.as_secs()))
                .unwrap_or_else(|_| "unix:0".to_string());
            let n = updates::apply_choice(&mut lock, &plan, &choice, &now);
            updates::save_state(&root, &lock)?;
            if sub == "confirm" {
                println!(
                    "已确认升级 {n} 个插件；运行 mkv editors upgrade 同步源码，再重建并重启应用。"
                );
            } else {
                println!("已跳过 {n} 个上游版本，之后不再提示这些版本。");
            }
            Ok(())
        }
        "upgrade" => {
            let root = root_arg(&rest[1..])?;
            let mut lock = updates::load_state(&root);
            let pending: Vec<String> = lock
                .pending
                .iter()
                .map(|p| p.plugin_id.clone())
                .collect();
            if pending.is_empty() {
                println!("没有待应用的升级；请先在应用启动时确认版本更新。");
                return Ok(());
            }
            let script = std::env::current_dir()?.join("scripts/sync-editors.sh");
            if !script.exists() {
                println!(
                    "未找到同步脚本 {}；请在仓库根目录手动运行：",
                    script.display()
                );
                println!("  bash scripts/sync-editors.sh --apply --root <知识库目录>");
                return Ok(());
            }
            let status = std::process::Command::new("bash")
                .arg(&script)
                .arg("--apply")
                .arg("--root")
                .arg(&root)
                .status()
                .context("执行上游同步脚本失败")?;
            if !status.success() {
                anyhow::bail!("同步脚本失败（退出码 {status}），待重建标记保持原样");
            }
            let cleared = updates::clear_pending(&mut lock, &pending);
            updates::save_state(&root, &lock)?;
            println!(
                "已同步并清除 {cleared} 个待重建标记；下一步：cargo build --workspace 重建插件并重启应用。"
            );
            Ok(())
        }
        other => {
            anyhow::bail!("未知的 editors 子命令: {other}（可用 plugins / check / upgrade）")
        }
    }
}

// ------------------------------------------------------------------ 参数

fn root_arg(rest: &[String]) -> Result<PathBuf> {
    rest.first()
        .map(PathBuf::from)
        .context("缺少知识库路径参数 <root>")
}

fn query_arg(rest: &[String]) -> Result<String> {
    rest.get(1)
        .cloned()
        .context("缺少检索词参数 <query>")
}

fn path_arg(rest: &[String]) -> Result<String> {
    rest.get(1)
        .cloned()
        .context("缺少文件路径参数 <path>")
}

// ------------------------------------------------------------------ 子命令

fn cmd_init(root: PathBuf, init_git: bool) -> Result<()> {
    let vault = Vault::open(&root, init_git).context("初始化知识库失败")?;
    println!("[mkv] 知识库已就绪: {}", root.display());
    if let Some(dir) = vault.kbindex_dir() {
        println!("[mkv] 索引目录: {}", dir.display());
    }
    Ok(())
}

fn cmd_demo(root: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&root).with_context(|| format!("创建目录失败: {}", root.display()))?;
    let created = mkv_core::demo::create_sample_vault(&root).context("生成示例文档失败")?;
    println!("[mkv] 已生成 {} 个示例文档:", created.len());
    for p in &created {
        println!("      - {p}");
    }

    let mut vault = Vault::open(&root, true).context("打开知识库失败")?;
    let report = vault.reindex(None).context("建立索引失败")?;
    println!(
        "[mkv] 索引完成：扫描 {} / 入库 {} / 跳过 {} / 链接 {} 条 / 用时 {} ms",
        report.scanned,
        report.indexed,
        report.skipped,
        report.links_found,
        report.elapsed_ms
    );

    if let Some(dir) = vault.kbindex_dir() {
        println!("[mkv] .kbindex/ 已落盘: {}", dir.display());
    }
    Ok(())
}

/// 索引过期提示（领域词典被手工改动时分词规则与索引不一致）。
fn warn_if_stale(vault: &Vault) {
    if vault.is_index_stale() {
        eprintln!(
            "[mkv] 警告：领域词典已变更，现有索引与其不一致，检索结果可能失真。\n\
             [mkv]      请执行 `mkv index <root>` 重建索引。"
        );
    }
}

fn cmd_index(root: PathBuf) -> Result<()> {
    let mut vault = Vault::open(&root, false).context("打开知识库失败")?;
    let report = vault.reindex(None).context("重建索引失败")?;    println!(
        "[mkv] 索引完成：扫描 {} / 入库 {} / 跳过 {} / 链接 {} 条 / 用时 {} ms",
        report.scanned,
        report.indexed,
        report.skipped,
        report.links_found,
        report.elapsed_ms
    );
    Ok(())
}

fn cmd_search(root: PathBuf, query: String) -> Result<()> {
    let vault = Vault::open(&root, false).context("打开知识库失败")?;
    warn_if_stale(&vault);
    let hits = vault.search(&query, 20).context("检索失败")?;
    if hits.is_empty() {
        println!("[mkv] 无匹配结果");
        return Ok(());
    }
    println!("[mkv] 命中 {} 条：", hits.len());
    for h in hits {
        println!(
            "  {:>6.3}  [{}] {}  ({})",
            h.score, h.format, h.path, h.title
        );
        if !h.snippet.is_empty() {
            println!("         {}", h.snippet);
        }
    }
    Ok(())
}

fn cmd_links(root: PathBuf, path: String) -> Result<()> {
    let vault = Vault::open(&root, false).context("打开知识库失败")?;
    let back = vault.backlinks(&path).context("查询反链失败")?;
    let out = vault.outgoing_links(&path).context("查询出链失败")?;

    println!("[mkv] 反链（谁引用了 {path}）：{} 条", back.len());
    for l in &back {
        println!("  ← {}  →  {}", l.from_path, l.target);
    }
    println!("[mkv] 出链（{path} 引用了谁）：{} 条", out.len());
    for l in &out {
        let resolved = if l.to_path.is_empty() {
            "（未解析）".to_string()
        } else {
            l.to_path.clone()
        };
        println!("  → {}  ⇒  {}", l.target, resolved);
    }
    Ok(())
}

fn cmd_docs(root: PathBuf) -> Result<()> {
    let vault = Vault::open(&root, false).context("打开知识库失败")?;
    warn_if_stale(&vault);
    let docs = vault.documents().context("读取文档列表失败")?;
    println!("[mkv] 共 {} 个文档：", docs.len());
    for d in &docs {
        println!("  {:<12} {:<8} {}", d.format, d.size, d.path);
    }
    Ok(())
}

fn cmd_commit(root: PathBuf, rest: &[String]) -> Result<()> {
    let mut message = "update".to_string();
    let mut author = "mkv".to_string();
    let mut i = 1; // rest[0] 是 root
    while i < rest.len() {
        match rest[i].as_str() {
            "-m" | "--message" => {
                message = rest.get(i + 1).cloned().unwrap_or(message);
                i += 2;
            }
            "-a" | "--author" => {
                author = rest.get(i + 1).cloned().unwrap_or(author);
                i += 2;
            }
            _ => i += 1,
        }
    }

    let vault = Vault::open(&root, true).context("打开知识库失败")?;
    let c = vault.commit(&message, &author).context("提交失败")?;
    println!("[mkv] 已提交 {}  {}", c.short_id, c.message);
    Ok(())
}

fn cmd_log(root: PathBuf) -> Result<()> {
    let vault = Vault::open(&root, false).context("打开知识库失败")?;
    let logs = vault.log(20).context("读取历史失败")?;
    if logs.is_empty() {
        println!("[mkv] 暂无提交记录");
        return Ok(());
    }
    for l in logs {
        println!("{}  {}  {}", l.short_id, l.author, l.message);
    }
    Ok(())
}

fn cmd_dict(root: PathBuf, dict_path: String) -> Result<()> {
    let text = std::fs::read_to_string(&dict_path)
        .with_context(|| format!("读取词典失败: {dict_path}"))?;
    let mut vault = Vault::open(&root, false).context("打开知识库失败")?;
    let added = vault.apply_domain_dict(&text).context("应用词典失败")?;
    println!("[mkv] 已载入 {added} 个领域词条并重建索引");
    Ok(())
}

/// 生成 Web 端预构建只读索引产物。
///
/// 流程：刷新索引 → 读取文档正文与双链 → 用 jieba（+ 领域词典）分词
/// → 汇总倒排表 → 写 JSON。
/// 分词规则与运行时索引完全一致，保证 Web 端查询能正确命中。
fn cmd_export_web(root: PathBuf, rest: &[String]) -> Result<()> {
    let out_path = rest
        .get(1)
        .map(PathBuf::from)
        .context("缺少输出路径参数 <out.json>")?;
    let dict_path = rest.get(2).map(PathBuf::from);

    let mut vault = Vault::open(&root, false).context("打开知识库失败")?;

    // 领域词典（可选）：先应用到运行时索引，
    // 保证「库内索引」与「导出产物」使用同一套分词规则。
    let dict_text = match &dict_path {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .with_context(|| format!("读取词典失败: {}", p.display()))?;
            let added = vault
                .apply_domain_dict(&text)
                .context("应用领域词典失败")?;
            println!("[mkv] 已应用领域词典：{added} 个词条");
            Some(text)
        }
        None => None,
    };

    // 导出前刷新索引：保证产物反映知识库的当前内容，
    // 避免「知识库尚未建索引 → 静默产出空产物」这类坑。
    // （应用词典时 `apply_domain_dict` 已触发重建，无需重复。）
    if dict_text.is_none() {
        let report = vault.reindex(None).context("重建索引失败")?;
        println!(
            "[mkv] 索引已刷新：扫描 {} / 入库 {} / 链接 {} 条",
            report.scanned,
            report.indexed,
            report.links_found
        );
    }

    let docs = vault.documents().context("读取文档列表失败")?;

    // 抽取正文（与索引使用同一套解析逻辑）
    let mut texts: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for doc in &docs {
        if let Ok(text) = vault.read_extracted_text(&doc.path) {
            texts.insert(doc.path.clone(), text);
        }
    }

    // 双链
    let mut links: Vec<mkv_core::LinkRecord> = Vec::new();
    for doc in &docs {
        if let Ok(out) = vault.outgoing_links(&doc.path) {
            links.extend(out);
        }
    }

    let root_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());

    let index = mkv_core::prebuilt_export::build_index(
        &docs,
        &texts,
        &links,
        dict_text.as_deref(),
        &root_name,
    )
    .context("构建预构建索引失败")?;

    let json = index.to_json().context("序列化失败")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    std::fs::write(&out_path, json)
        .with_context(|| format!("写入失败: {}", out_path.display()))?;

    println!(
        "[mkv] 已生成 Web 端索引产物：{}（{} 个文档 / {} 个词条 / {} 条双链）",
        out_path.display(),
        index.doc_count(),
        index.postings.len(),
        index.links.len()
    );
    Ok(())
}

/// 监听知识库文件变化，增量更新索引与双链（Ctrl+C 退出）。
fn cmd_watch(root: PathBuf, rest: &[String]) -> Result<()> {
    let mut options = mkv_core::watcher::WatchOptions::default();
    let mut i = 1; // rest[0] 是 root
    while i < rest.len() {
        match rest[i].as_str() {
            "--auto-commit" => options.auto_commit = true,
            "--debounce" => {
                let ms = rest
                    .get(i + 1)
                    .context("--debounce 缺少毫秒数")?
                    .parse::<u64>()
                    .context("--debounce 必须是整数毫秒")?;
                options.debounce = Duration::from_millis(ms);
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    println!("[mkv] 正在监听：{}", root.display());
    println!(
        "[mkv] 去抖 {} ms · 自动提交 {} · Ctrl+C 退出",
        options.debounce.as_millis(),
        if options.auto_commit { "开" } else { "关" }
    );

    mkv_core::watcher::run_with_vault(&root, &options, |report| {
        println!(
            "[mkv-watch] {}",
            mkv_core::watcher::describe_report(report)
        );
    })
    .context("文件监听异常退出")
}

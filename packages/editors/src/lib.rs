//! 编辑器插件聚合层。
//!
//! 只有 **Desktop / CLI** 链接这个 crate 并在启动时调用 [`install()`]；
//! Web 只读端不依赖它，因此三个编辑器的解析 / 写回代码完全不进入 wasm 产物
//! （这是 `wasm < 800KB` 门禁与「零 C 依赖」约束能继续成立的关键）。
//!
//! 想替换某个编辑器实现时，只改这里注册的类型即可，`mkv-core` 与 `mkv-ui`
//! 都不需要改动 —— 这正是插件化的意义。
//!
//! # 后端选择（架构闭环）
//!
//! 每个插件可以用两种后端：
//!
//! - `builtin`（默认）：本项目自研的**包级外科手术**实现，未编辑部件字节级原样保留；
//! - `external`：**进程外**上游 MCP 实现（JSON-RPC 2.0 over stdio）。
//!
//! 选择写在 `editors.lock` 的 `backends` 段（运行期可改写到
//! `<库>/.kbindex/editors.json`）。为什么上游只能走进程外而不是链接 crate，
//! 证据见 [`mkv_core::backend::UPSTREAM_LINKING_FINDINGS`]。
//!
//! 切换是**逐插件**的：把 `gridcraft` 切到 `external` 不影响另外两个。

use std::sync::Arc;

use mkv_core::backend::{BackendHealth, ExternalConfig};
use mkv_core::editor_updates::{BackendPin, EditorsLock};
use mkv_core::format::Format;
use mkv_core::plugin::{BackendPlugin, EditorPlugin, PluginDescriptor};

use mkv_editor_deckcraft::DeckCraftPlugin;
use mkv_editor_gridcraft::GridCraftPlugin;
use mkv_editor_wordcraft::WordCraftPlugin;

pub use mkv_editor_deckcraft::{PLUGIN_ID as DECKCRAFT_ID, UPSTREAM_REPO as DECKCRAFT_REPO};
pub use mkv_editor_gridcraft::{PLUGIN_ID as GRIDCRAFT_ID, UPSTREAM_REPO as GRIDCRAFT_REPO};
pub use mkv_editor_wordcraft::{PLUGIN_ID as WORDCRAFT_ID, UPSTREAM_REPO as WORDCRAFT_REPO};

/// 进程外后端的默认调用超时（毫秒）。
pub const DEFAULT_EXTERNAL_TIMEOUT_MS: u64 = 10_000;

/// 当前内置的三个插件（顺序即注册表顺序）。
pub fn builtin_plugins() -> Vec<Arc<dyn EditorPlugin>> {
    vec![
        Arc::new(GridCraftPlugin),
        Arc::new(DeckCraftPlugin),
        Arc::new(WordCraftPlugin),
    ]
}

/// 格式键 → 内部格式枚举。
fn format_of(format_key: &str) -> Option<Format> {
    match format_key {
        "xlsx" => Some(Format::Excel),
        "pptx" => Some(Format::PowerPoint),
        "docx" => Some(Format::Word),
        _ => None,
    }
}

/// 插件 ID → 格式（供外部后端构造与 CLI 展示）。
pub fn format_of_plugin(plugin_id: &str) -> Option<Format> {
    match plugin_id {
        GRIDCRAFT_ID => Some(Format::Excel),
        DECKCRAFT_ID => Some(Format::PowerPoint),
        WORDCRAFT_ID => Some(Format::Word),
        _ => None,
    }
}

/// 按后端配置构建插件列表。
///
/// 每个插件独立决定用哪种后端：
/// - `builtin` → 直接用本项目自研实现；
/// - `external` → 起上游 MCP 进程，用 [`BackendPlugin`] 适配。
///
/// 配置缺失或格式不认识时**退回 builtin**：后端切换是可选增强，
/// 不该因为配置问题让整个编辑器不可用。
pub fn plugins_for(lock: &EditorsLock) -> Vec<Arc<dyn EditorPlugin>> {
    builtin_plugins()
        .into_iter()
        .map(|plugin| {
            let desc = plugin.descriptor();
            let pin = lock.backend_of(&desc.id);
            if pin.kind != "external" {
                return plugin;
            }
            let Some(format) = format_of(&desc.format_key) else {
                return plugin;
            };
            let config = ExternalConfig {
                command: pin.command.clone(),
                args: pin.args.clone(),
                timeout_ms: DEFAULT_EXTERNAL_TIMEOUT_MS,
            };
            let backend = Arc::new(mkv_core::backend::ExternalBackend::new(
                &desc.id,
                format,
                config,
            ));
            let mut ext_desc = desc.clone();
            ext_desc.backend = "external".to_string();
            Arc::new(BackendPlugin::new(ext_desc, backend)) as Arc<dyn EditorPlugin>
        })
        .collect()
}

/// 应用启动时注入插件。按 ID 覆盖式注册，重复调用是幂等的。
pub fn install() -> mkv_core::Result<()> {
    let lock = mkv_core::editor_updates::bundled_lock()?;
    install_with(&lock)
}

/// 按指定锁文件注入插件。
pub fn install_with(lock: &EditorsLock) -> mkv_core::Result<()> {
    mkv_core::plugin::install(plugins_for(lock))
}

/// 插件自描述清单（诊断 / UI / 升级检测用）。
pub fn descriptors() -> Vec<PluginDescriptor> {
    builtin_plugins()
        .iter()
        .map(|p| p.descriptor())
        .collect()
}

/// 各插件当前后端的健康状态（CLI 展示用）。
///
/// `builtin` 后端恒为可用；`external` 后端会**真实启动**上游进程做一次版本探测，
/// 探测失败只影响该条状态，不会中断整体检查。
pub fn backend_health(lock: &EditorsLock) -> Vec<BackendHealth> {
    let mut out = Vec::new();
    for plugin in plugins_for(lock) {
        let desc = plugin.descriptor();
        let pin = lock.backend_of(&desc.id);
        if pin.kind == "external" {
            if let Some(format) = format_of(&desc.format_key) {
                let config = ExternalConfig {
                    command: pin.command.clone(),
                    args: pin.args.clone(),
                    timeout_ms: DEFAULT_EXTERNAL_TIMEOUT_MS,
                };
                let backend = mkv_core::backend::ExternalBackend::new(&desc.id, format, config);
                out.push(backend.health());
                continue;
            }
        }
        out.push(BackendHealth {
            id: desc.id.clone(),
            kind: mkv_core::backend::BackendKind::Builtin,
            available: true,
            reason: String::new(),
            version: desc.plugin_version.clone(),
        });
    }
    out
}

/// 切换某插件的后端并写回运行时锁文件。
///
/// `kind` 为 `builtin` 时清空 `command` / `args`（避免留下失效配置）。
pub fn set_backend(
    root: &std::path::Path,
    plugin_id: &str,
    kind: &str,
    command: &str,
    args: &[String],
    reason: &str,
) -> mkv_core::Result<EditorsLock> {
    if kind != "builtin" && kind != "external" {
        return Err(mkv_core::Error::Config(format!(
            "后端类型 `{kind}` 非法（应为 builtin / external）"
        )));
    }
    if kind == "external" && command.trim().is_empty() {
        return Err(mkv_core::Error::Config(
            "切换到 external 后端必须指定上游命令（如 gridcraft-mcp）".into(),
        ));
    }
    let mut lock = mkv_core::editor_updates::load_state(root);
    let pin = BackendPin {
        id: plugin_id.to_string(),
        kind: kind.to_string(),
        reason: reason.to_string(),
        command: if kind == "external" {
            command.to_string()
        } else {
            String::new()
        },
        args: if kind == "external" {
            args.to_vec()
        } else {
            Vec::new()
        },
    };
    match lock.backends.iter_mut().find(|b| b.id == plugin_id) {
        Some(slot) => *slot = pin,
        None => lock.backends.push(pin),
    }
    mkv_core::editor_updates::save_state(root, &lock)?;
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_plugins_cover_excel_ppt_word_without_duplicates() {
        let ds = descriptors();
        assert_eq!(ds.len(), 3);
        let mut keys: Vec<String> = ds.iter().map(|d| d.format_key.clone()).collect();
        keys.sort();
        assert_eq!(keys, vec!["docx", "pptx", "xlsx"]);
        let mut ids: Vec<String> = ds.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3);
        assert!(ds
            .iter()
            .all(|d| d.contract_version == mkv_core::plugin::PLUGIN_CONTRACT_VERSION));
    }

    #[test]
    fn upstream_repos_are_the_three_craft_projects() {
        assert_eq!(GRIDCRAFT_REPO, "storytold/gridcraft");
        assert_eq!(DECKCRAFT_REPO, "storytold/deckcraft");
        assert_eq!(WORDCRAFT_REPO, "storytold/wordcraft");
    }

    #[test]
    fn bundled_lock_selects_builtin_backends() {
        let lock = mkv_core::editor_updates::bundled_lock().expect("内置锁文件必须有效");
        for id in [GRIDCRAFT_ID, DECKCRAFT_ID, WORDCRAFT_ID] {
            assert_eq!(lock.backend_of(id).kind, "builtin", "{id} 默认应为 builtin");
        }
        // 默认配置下构建出的插件与纯 builtin 完全一致
        let plugins = plugins_for(&lock);
        assert_eq!(plugins.len(), 3);
        for p in &plugins {
            assert_eq!(p.backend_kind(), mkv_core::backend::BackendKind::Builtin);
        }
    }

    #[test]
    fn external_pin_produces_external_backend_plugin() {
        let mut lock = mkv_core::editor_updates::bundled_lock().unwrap();
        lock.backends.retain(|b| b.id != GRIDCRAFT_ID);
        lock.backends.push(BackendPin {
            id: GRIDCRAFT_ID.to_string(),
            kind: "external".to_string(),
            reason: "测试".to_string(),
            command: "gridcraft-mcp".to_string(),
            args: vec!["--serve-mcp".to_string()],
        });
        let plugins = plugins_for(&lock);
        let grid = plugins
            .iter()
            .find(|p| p.descriptor().id == GRIDCRAFT_ID)
            .expect("应仍有 gridcraft 插件");
        assert_eq!(grid.backend_kind(), mkv_core::backend::BackendKind::External);
        assert_eq!(grid.descriptor().backend, "external");
        // 其它插件不受影响
        for p in &plugins {
            if p.descriptor().id != GRIDCRAFT_ID {
                assert_eq!(p.backend_kind(), mkv_core::backend::BackendKind::Builtin);
            }
        }
    }

    #[test]
    fn external_without_command_is_rejected_by_lock_validation() {
        let mut lock = mkv_core::editor_updates::bundled_lock().unwrap();
        lock.backends.retain(|b| b.id != GRIDCRAFT_ID);
        lock.backends.push(BackendPin {
            id: GRIDCRAFT_ID.to_string(),
            kind: "external".to_string(),
            reason: String::new(),
            command: String::new(),
            args: Vec::new(),
        });
        assert!(lock.validate().is_err(), "external 缺命令应被拒绝");
    }
}

//! 编辑器插件聚合层。
//!
//! 只有 **Desktop / CLI** 链接这个 crate 并在启动时调用 [`install()`]；
//! Web 只读端不依赖它，因此三个编辑器的解析 / 写回代码完全不进入 wasm 产物
//! （这是 `wasm < 800KB` 门禁与「零 C 依赖」约束能继续成立的关键）。
//!
//! 想替换某个编辑器实现时，只改这里注册的类型即可，`mkv-core` 与 `mkv-ui`
//! 都不需要改动 —— 这正是插件化的意义。

use std::sync::Arc;

use mkv_core::plugin::{EditorPlugin, PluginDescriptor};
use mkv_editor_deckcraft::DeckCraftPlugin;
use mkv_editor_gridcraft::GridCraftPlugin;
use mkv_editor_wordcraft::WordCraftPlugin;

pub use mkv_editor_deckcraft::{PLUGIN_ID as DECKCRAFT_ID, UPSTREAM_REPO as DECKCRAFT_REPO};
pub use mkv_editor_gridcraft::{PLUGIN_ID as GRIDCRAFT_ID, UPSTREAM_REPO as GRIDCRAFT_REPO};
pub use mkv_editor_wordcraft::{PLUGIN_ID as WORDCRAFT_ID, UPSTREAM_REPO as WORDCRAFT_REPO};

/// 当前内置的三个插件（顺序即注册表顺序）。
pub fn builtin_plugins() -> Vec<Arc<dyn EditorPlugin>> {
    vec![
        Arc::new(GridCraftPlugin),
        Arc::new(DeckCraftPlugin),
        Arc::new(WordCraftPlugin),
    ]
}

/// 应用启动时注入插件。按 ID 覆盖式注册，重复调用是幂等的。
pub fn install() -> mkv_core::Result<()> {
    mkv_core::plugin::install(builtin_plugins())
}

/// 插件自描述清单（诊断 / UI / 升级检测用）。
pub fn descriptors() -> Vec<PluginDescriptor> {
    builtin_plugins()
        .iter()
        .map(|p| p.descriptor())
        .collect()
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
}

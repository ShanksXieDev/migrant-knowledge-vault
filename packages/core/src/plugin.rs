//! 编辑器插件契约（v1）。
//!
//! # 为什么要独立出来
//!
//! v0.1 的三个内置编辑器是 `mkv-core` 内部的模块，UI 直接依赖它们的模型类型，
//! 因此「换一个编辑器实现」必须改 core + UI 两处。插件化之后：
//!
//! - core 只保留**契约**：[`EditorPlugin`] trait、稳定 DTO（[`PluginView`] /
//!   [`PluginAction`] / [`PluginDescriptor`]）与注册表；
//! - 每个编辑器是 workspace 里的独立 crate（见 `packages/plugins/`），
//!   通过 JSON 字节边界交换状态，core / UI 不认识它的私有模型；
//! - Desktop / CLI 启动时调用 `mkv_editors::install()` 注入插件；
//!   Web 只读端不注入，编辑器实现因此完全不进入 wasm 产物。
//!
//! # 契约稳定性
//!
//! [`PLUGIN_CONTRACT_VERSION`] 是唯一版本闸门：插件声明的版本不匹配时
//! [`PluginRegistry::register`] 直接拒绝，避免旧插件把新 UI 打崩。
//! 状态用 `Vec<u8>`（JSON）传递，不跨 crate 暴露 Rust 类型，也不依赖 Rust ABI。
//!
//! # 与上游三个 Craft 项目的关系
//!
//! `storytold/{gridcraft,wordcraft,deckcraft}` 是上游独立 Rust workspace。
//! 插件的 `upstream_repo` / `upstream_rev` 记录其对齐的上游提交，
//! 由 `editors.lock` + `editor_updates.rs` 检测与升级；默认后端 `builtin`
//! 仍是本项目自研的「包级外科手术」实现（README §9.2），保证排版保真。

use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::format::Format;

/// 插件契约版本。变更 DTO 或 trait 语义时递增，并同步 `editors.lock`。
pub const PLUGIN_CONTRACT_VERSION: u32 = 1;

// ---------------------------------------------------------------- 描述信息

/// 插件自描述信息（UI 与升级检测都只认这份 DTO）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDescriptor {
    /// 稳定 ID，如 `gridcraft`；升级前后必须保持不变。
    pub id: String,
    /// 展示名，如 `GridCraft`。
    pub display_name: String,
    /// 支持的格式键：`xlsx` / `pptx` / `docx`。
    pub format_key: String,
    /// 当前后端：`builtin`（外科手术实现）/ `upstream`（上游适配器）。
    pub backend: String,
    /// 对齐的上游仓库，如 `storytold/gridcraft`。
    pub upstream_repo: String,
    /// 对齐的上游提交 SHA（`editors.lock` 中的 pin）。
    pub upstream_rev: String,
    /// 插件 crate 自身版本。
    pub plugin_version: String,
    /// 契约版本，必须等于 [`PLUGIN_CONTRACT_VERSION`]。
    pub contract_version: u32,
    /// 能力标签，供 UI 决定是否显示某些按钮。
    pub capabilities: Vec<String>,
}

impl PluginDescriptor {
    /// 构造一个 `builtin` 后端描述。
    pub fn builtin(
        id: &str,
        display_name: &str,
        format_key: &str,
        upstream_repo: &str,
        upstream_rev: &str,
        plugin_version: &str,
        capabilities: &[&str],
    ) -> Self {
        Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            format_key: format_key.to_string(),
            backend: "builtin".to_string(),
            upstream_repo: upstream_repo.to_string(),
            upstream_rev: upstream_rev.to_string(),
            plugin_version: plugin_version.to_string(),
            contract_version: PLUGIN_CONTRACT_VERSION,
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// 格式 → 格式键。
pub fn format_key(format: Format) -> &'static str {
    match format {
        Format::Excel => "xlsx",
        Format::PowerPoint => "pptx",
        Format::Word => "docx",
        Format::Markdown => "md",
        Format::Visio => "vsdx",
        Format::Drawio => "drawio",
    }
}

// ---------------------------------------------------------------- 视图 DTO

/// 插件向 UI 提供的**声明式视图**：UI 只按这三种形态渲染，不认识插件私有模型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginView {
    /// 表格：工作表 + 稀疏单元格。
    Grid { sheets: Vec<SheetView> },
    /// 演示：页 + 形状文本。
    Deck { slides: Vec<SlideView> },
    /// 文档：段落序列 + 插件声明的可用样式（UI 不硬编码样式表）。
    Word {
        paragraphs: Vec<ParagraphView>,
        style_options: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetView {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<CellView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellView {
    pub row: usize,
    pub col: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlideView {
    pub index: usize,
    pub title: String,
    pub shapes: Vec<ShapeView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeView {
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParagraphView {
    pub index: usize,
    pub text: String,
    pub style: String,
}

// ---------------------------------------------------------------- 操作 DTO

/// UI 把用户操作转成稳定动作，交回插件执行；插件负责维护自己的状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginAction {
    SetCell {
        sheet: usize,
        row: usize,
        col: usize,
        text: String,
    },
    InsertRow {
        sheet: usize,
        at: usize,
    },
    RemoveRow {
        sheet: usize,
        at: usize,
    },
    InsertColumn {
        sheet: usize,
        at: usize,
    },
    RemoveColumn {
        sheet: usize,
        at: usize,
    },
    SetShapeText {
        slide: usize,
        shape: usize,
        text: String,
    },
    RemoveSlide {
        index: usize,
    },
    MoveSlide {
        from: usize,
        to: usize,
    },
    SetParagraph {
        index: usize,
        text: String,
    },
    SetParagraphStyle {
        index: usize,
        style: String,
    },
    InsertParagraph {
        at: usize,
        style: String,
    },
    RemoveParagraph {
        index: usize,
    },
}

// ---------------------------------------------------------------- 文档状态

/// 插件私有状态。对 core / UI 完全不透明（JSON 字节），
/// 因此插件可以独立升级模型结构而不影响宿主。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDocument {
    pub plugin_id: String,
    pub state: Vec<u8>,
}

// ---------------------------------------------------------------- trait

/// 编辑器插件必须实现的契约。
pub trait EditorPlugin: Send + Sync {
    /// 自描述信息（含契约版本与对齐的上游 rev）。
    fn descriptor(&self) -> PluginDescriptor;

    /// 是否支持该格式。
    fn supports(&self, format: Format) -> bool;

    /// 从原生文件字节打开一个编辑会话。
    fn open(&self, bytes: &[u8]) -> Result<PluginDocument>;

    /// 生成 UI 视图。
    fn view(&self, doc: &PluginDocument) -> Result<PluginView>;

    /// 应用一次用户操作，更新私有状态。
    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()>;

    /// 序列化回原生文件。**必须**以 `original` 为基底做外科改写，
    /// 未编辑的部件字节级原样保留（README §9.2）。
    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>>;
}

// ---------------------------------------------------------------- 注册表

/// 插件注册表：按格式 / ID 查找，供路由与 UI 使用。
#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn EditorPlugin>>,
}

impl PluginRegistry {
    /// 注册一个插件；契约版本不符或 ID 重复时拒绝。
    pub fn register(&mut self, plugin: Arc<dyn EditorPlugin>) -> Result<()> {
        let desc = plugin.descriptor();
        if desc.contract_version != PLUGIN_CONTRACT_VERSION {
            return Err(Error::Config(format!(
                "插件 {} 契约版本 {} 与宿主 {} 不匹配",
                desc.id, desc.contract_version, PLUGIN_CONTRACT_VERSION
            )));
        }
        if self.plugins.iter().any(|p| p.descriptor().id == desc.id) {
            return Err(Error::Config(format!("插件 {} 重复注册", desc.id)));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn EditorPlugin>> {
        self.plugins
            .iter()
            .find(|p| p.descriptor().id == id)
            .cloned()
    }

    pub fn for_format(&self, format: Format) -> Option<Arc<dyn EditorPlugin>> {
        self.plugins.iter().find(|p| p.supports(format)).cloned()
    }

    /// 按 ID 覆盖式注册（应用启动 / 插件升级后用）。
    ///
    /// 与 [`Self::register`] 的严格去重不同：这里允许同 ID 的新实现替换旧实现，
    /// 让「升级后重新注入」不需要清空整张注册表，也不会影响其它插件。
    pub fn replace(&mut self, plugin: Arc<dyn EditorPlugin>) -> Result<()> {
        let desc = plugin.descriptor();
        if desc.contract_version != PLUGIN_CONTRACT_VERSION {
            return Err(Error::Config(format!(
                "插件 {} 契约版本 {} 与宿主 {} 不匹配",
                desc.id, desc.contract_version, PLUGIN_CONTRACT_VERSION
            )));
        }
        self.plugins.retain(|p| p.descriptor().id != desc.id);
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        self.plugins.iter().map(|p| p.descriptor()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }
}

/// 全局插件槽。Web 端不调用 `install()`，因此 wasm 产物里没有编辑器实现。
static REGISTRY: OnceLock<RwLock<PluginRegistry>> = OnceLock::new();

pub fn registry() -> &'static RwLock<PluginRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(PluginRegistry::default()))
}

/// 安装插件（应用启动时调用；重复调用按 ID 覆盖，天然幂等）。
///
/// 采用覆盖式而非「只允许一次」：插件升级后重新注入、以及并行单测各自
/// 注入自己的插件都不会互相冲突。
pub fn install(plugins: Vec<Arc<dyn EditorPlugin>>) -> Result<()> {
    let mut guard = registry()
        .write()
        .map_err(|_| Error::Config("插件注册表锁已损坏".into()))?;
    for plugin in plugins {
        guard.replace(plugin)?;
    }
    Ok(())
}

fn lookup(id: &str) -> Result<Arc<dyn EditorPlugin>> {
    registry()
        .read()
        .map_err(|_| Error::Config("插件注册表锁已损坏".into()))?
        .get(id)
        .ok_or_else(|| {
            Error::Config(format!(
                "编辑器插件 {id} 未安装（Desktop/CLI 启动时需调用 mkv_editors::install()）"
            ))
        })
}

/// 取当前会话的 UI 视图。
pub fn view(session: &crate::editor::EditSession) -> Result<PluginView> {
    lookup(&session.plugin_id)?.view(&session.document)
}

/// 把一个 UI 动作应用到会话的插件状态。
pub fn apply_action(
    session: &mut crate::editor::EditSession,
    action: &PluginAction,
) -> Result<()> {
    let plugin = lookup(&session.plugin_id)?;
    if session.document.plugin_id != session.plugin_id {
        return Err(Error::Config("编辑会话的插件 ID 不一致".into()));
    }
    plugin.apply(&mut session.document, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy(String, &'static str, Format);

    impl EditorPlugin for Dummy {
        fn descriptor(&self) -> PluginDescriptor {
            PluginDescriptor {
                contract_version: PLUGIN_CONTRACT_VERSION,
                ..PluginDescriptor::builtin(
                    &self.0,
                    "Dummy",
                    format_key(self.2),
                    "storytold/dummy",
                    "rev",
                    "0.1.0",
                    &[],
                )
            }
        }
        fn supports(&self, f: Format) -> bool {
            f == self.2
        }
        fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
            Ok(PluginDocument {
                plugin_id: self.0.clone(),
                state: bytes.to_vec(),
            })
        }
        fn view(&self, _doc: &PluginDocument) -> Result<PluginView> {
            Ok(PluginView::Word { paragraphs: vec![], style_options: vec![] })
        }
        fn apply(&self, doc: &mut PluginDocument, _a: &PluginAction) -> Result<()> {
            doc.state.push(1);
            Ok(())
        }
        fn serialize(&self, doc: &PluginDocument, _o: &[u8]) -> Result<Vec<u8>> {
            Ok(doc.state.clone())
        }
    }

    #[test]
    fn registry_enforces_contract_version_and_unique_ids() {
        let mut reg = PluginRegistry::default();
        reg.register(Arc::new(Dummy("a".into(), "", Format::Excel)))
            .unwrap();
        assert_eq!(reg.len(), 1);
        assert!(reg.for_format(Format::Excel).is_some());
        assert!(reg.for_format(Format::Word).is_none());
        // 同 ID 重复注册应被拒绝
        assert!(reg
            .register(Arc::new(Dummy("a".into(), "", Format::Word)))
            .is_err());
    }

    #[test]
    fn descriptors_carry_upstream_alignment() {
        let p = Dummy("gridcraft".into(), "", Format::Excel);
        let d = p.descriptor();
        assert_eq!(d.contract_version, PLUGIN_CONTRACT_VERSION);
        assert_eq!(d.backend, "builtin");
        assert_eq!(d.format_key, "xlsx");
        assert_eq!(d.upstream_repo, "storytold/dummy");
    }

    #[test]
    fn view_and_action_go_through_plugin() {
        install(vec![Arc::new(Dummy("d".into(), "", Format::Word))]).unwrap();
        let mut session = crate::editor::EditSession {
            rel_path: "x.docx".into(),
            original: vec![9],
            plugin_id: "d".into(),
            document: PluginDocument {
                plugin_id: "d".into(),
                state: vec![9],
            },
        };
        assert!(matches!(
            view(&session).unwrap(),
            PluginView::Word { .. }
        ));
        apply_action(&mut session, &PluginAction::RemoveParagraph { index: 0 }).unwrap();
        assert_eq!(session.document.state, vec![9, 1]);
    }
}

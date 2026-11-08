//! 编辑器路由与编辑会话（插件化之后）。
//!
//! # 会话模型
//!
//! ```text
//!   EditorRouter::route()  →  EditorTarget
//!        │
//!        ├─ External { program, path }   → launch_external()
//!        │
//!        └─ Builtin { plugin_id, session }
//!                       │  PluginDocument（插件私有 JSON 状态）
//!                       ├─ plugin::view()          → 声明式 UI 视图
//!                       ├─ plugin::apply_action()  → 用户操作
//!                       └─ plugin::serialize()     → 写回原生文件
//!                                                    → Vault::update_document()
//! ```
//!
//! 内置编辑器**不再是 core 的模块**，而是 `packages/plugins/` 下的独立 crate，
//! 由 Desktop / CLI 启动时通过 [`crate::plugin::install`] 注入。
//! Web 只读端不注入，故 wasm 产物不含编辑器实现。
//!
//! 编辑会话持有原始字节；插件序列化时以「包级外科手术」方式只替换被编辑的
//! XML 部件，其余部件字节级原样保留（README §9.2）。

use crate::error::{Error, Result};
use crate::format::Format;
use crate::plugin::{format_key, PluginDocument};
use crate::storage::Storage;

/// 编辑模式偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// 外部唤起本机应用。
    External,
    /// 使用插件化内置编辑器。
    Builtin,
}

/// 编辑会话：持有原始字节与插件私有状态。
#[derive(Debug, Clone, PartialEq)]
pub struct EditSession {
    /// 文档相对路径。
    pub rel_path: String,
    /// 原始文件字节（保存时作为外科手术的基底）。
    pub original: Vec<u8>,
    /// 负责该会话的插件 ID。
    pub plugin_id: String,
    /// 插件私有状态（对宿主不透明）。
    pub document: PluginDocument,
}

impl EditSession {
    /// 序列化回原生文件字节（委托给插件）。
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let plugin = crate::plugin::registry()
            .read()
            .map_err(|_| Error::Config("插件注册表锁已损坏".into()))?
            .get(&self.plugin_id)
            .ok_or_else(|| Error::Config(format!("编辑器插件 {} 未安装", self.plugin_id)))?;
        plugin.serialize(&self.document, &self.original)
    }

    /// 写回存储并返回新的文件字节。
    pub fn save(&self, storage: &dyn Storage) -> Result<Vec<u8>> {
        let bytes = self.serialize()?;
        storage.write(&self.rel_path, &bytes)?;
        Ok(bytes)
    }
}

/// 编辑目标。
#[derive(Debug)]
pub enum EditorTarget {
    /// 调起外部程序，参数为可执行文件与文档路径。
    External {
        program: String,
        path: std::path::PathBuf,
    },
    /// 使用插件化内置编辑器。
    Builtin {
        plugin_id: String,
        editor: String,
        session: EditSession,
    },
}

/// 系统默认应用的启动命令（F-08）。
///
/// 抽成纯函数：不必真的启动进程就能被单测覆盖，也避免把平台判断散落到 UI 层。
/// - Windows：`cmd /C start "" <path>`（第一个空串是窗口标题，缺了会把路径当标题）
/// - macOS：`open <path>`
/// - Linux / BSD：`xdg-open <path>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOpenCommand {
    pub program: &'static str,
    pub args: Vec<String>,
}

pub fn system_open_command(path: &std::path::Path) -> ExternalOpenCommand {
    let p = path.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    {
        ExternalOpenCommand {
            program: "cmd",
            args: vec!["/C".into(), "start".into(), String::new(), p],
        }
    }
    #[cfg(target_os = "macos")]
    {
        ExternalOpenCommand {
            program: "open",
            args: vec![p],
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ExternalOpenCommand {
            program: "xdg-open",
            args: vec![p],
        }
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        // wasm32 等非原生目标仅保证可编译，不会真正执行。
        ExternalOpenCommand {
            program: "xdg-open",
            args: vec![p],
        }
    }
}

/// 编辑器路由器：内置能力来自全局插件注册表，外部唤起来自系统默认应用。
pub struct EditorRouter {
    /// 外部程序的映射：格式 → 可执行文件名。
    external_programs: Vec<(Format, String)>,
}

impl Default for EditorRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorRouter {
    /// 构建路由器。插件不在这里硬编码，而是由应用启动时注入注册表。
    pub fn new() -> Self {
        Self {
            external_programs: Vec::new(),
        }
    }

    /// 覆盖某格式的外部程序名。
    pub fn set_external_program(&mut self, format: Format, program: impl Into<String>) {
        let program = program.into();
        if let Some(slot) = self.external_programs.iter_mut().find(|(f, _)| *f == format) {
            slot.1 = program;
        } else {
            self.external_programs.push((format, program));
        }
    }

    /// 已安装的内置插件名称（未安装时为空，Web 只读端就是这种情况）。
    pub fn builtin_names(&self) -> Vec<String> {
        crate::plugin::registry()
            .read()
            .map(|r| r.descriptors().into_iter().map(|d| d.display_name).collect())
            .unwrap_or_default()
    }

    /// 路由：返回编辑目标。
    pub fn route(
        &self,
        storage: &dyn Storage,
        rel_path: &str,
        format: Format,
        mode: EditMode,
    ) -> Result<EditorTarget> {
        match mode {
            EditMode::External => {
                let abs = storage.absolute_path(rel_path).ok_or_else(|| {
                    Error::Config(format!("该后端不支持本地路径: {rel_path}"))
                })?;
                // 空字符串表示「用系统默认应用」；只有显式 set_external_program
                // 才使用指定可执行文件。六种格式因此都能被外部唤起。
                let program = self
                    .external_programs
                    .iter()
                    .find(|(f, _)| *f == format)
                    .map(|(_, p)| p.clone())
                    .unwrap_or_default();
                Ok(EditorTarget::External { program, path: abs })
            }
            EditMode::Builtin => {
                let registry = crate::plugin::registry()
                    .read()
                    .map_err(|_| Error::Config("插件注册表锁已损坏".into()))?;
                let plugin = registry.for_format(format).ok_or_else(|| {
                    Error::Config(format!(
                        "{} 无内置编辑器插件（Desktop/CLI 需先安装插件，或使用外部唤起）",
                        format.label()
                    ))
                })?;
                let desc = plugin.descriptor();
                let original = storage.read(rel_path)?;
                let document = plugin.open(&original)?;
                Ok(EditorTarget::Builtin {
                    plugin_id: desc.id.clone(),
                    editor: desc.display_name,
                    session: EditSession {
                        rel_path: rel_path.to_string(),
                        original,
                        plugin_id: desc.id,
                        document,
                    },
                })
            }
        }
    }

    /// 执行外部唤起（仅 Native 平台可用）。
    ///
    /// `program` 为空时按 F-08 走系统默认应用；非空则为显式指定的程序。
    /// 只 `spawn` 不 `wait`：Office / Visio 等应用由用户自行关闭，
    /// 之后保存文件会由 P0 文件监听自动增量重建索引与双链。
    pub fn launch_external(target: &EditorTarget) -> Result<()> {
        let EditorTarget::External { program, path } = target else {
            return Err(Error::Config("目标不是外部编辑程序".into()));
        };
        if !path.exists() {
            return Err(Error::NotFound(path.clone()));
        }
        let spawned = if program.is_empty() {
            let cmd = system_open_command(path);
            std::process::Command::new(cmd.program)
                .args(&cmd.args)
                .spawn()
        } else {
            std::process::Command::new(program).arg(path).spawn()
        };
        spawned.map_err(|e| Error::Config(format!("外部唤起失败: {e}")))?;
        Ok(())
    }
}

/// 供 UI / 诊断使用：当前格式是否有已安装的插件。
pub fn plugin_available(format: Format) -> bool {
    crate::plugin::registry()
        .read()
        .map(|r| r.for_format(format).is_some())
        .unwrap_or(false)
}

/// 供插件构造描述信息时使用：格式键。
pub fn key_of(format: Format) -> &'static str {
    format_key(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{
        EditorPlugin, PluginAction, PluginDescriptor, PluginDocument, PluginView,
        PLUGIN_CONTRACT_VERSION,
    };
    use std::sync::Arc;

    struct TestPlugin(Format);

    impl EditorPlugin for TestPlugin {
        fn descriptor(&self) -> PluginDescriptor {
            PluginDescriptor::builtin(
                "test-editor",
                "TestEditor",
                format_key(self.0),
                "storytold/test",
                "deadbeef",
                "0.1.0",
                &["read", "write"],
            )
        }
        fn supports(&self, f: Format) -> bool {
            f == self.0
        }
        fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
            Ok(PluginDocument {
                plugin_id: "test-editor".into(),
                state: bytes.to_vec(),
            })
        }
        fn view(&self, _doc: &PluginDocument) -> Result<PluginView> {
            Ok(PluginView::Word { paragraphs: vec![], style_options: vec![] })
        }
        fn apply(&self, doc: &mut PluginDocument, _a: &PluginAction) -> Result<()> {
            doc.state.push(7);
            Ok(())
        }
        fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(original, &doc.state[..doc.state.len().saturating_sub(1)]);
            Ok(doc.state.clone())
        }
    }

    fn sample_vault(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mkv_edit_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::demo::create_sample_vault(&dir).unwrap();
        dir
    }

    fn install_test_plugin() {
        crate::plugin::install(vec![Arc::new(TestPlugin(Format::Excel))]).unwrap();
    }

    #[test]
    fn routes_external_and_builtin_via_registry() {
        install_test_plugin();
        let dir = sample_vault("route");
        let storage = crate::storage::FsStorage::new(&dir);
        let router = EditorRouter::new();

        let t = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::External)
            .unwrap();
        assert!(matches!(t, EditorTarget::External { .. }));

        let t = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::Builtin)
            .unwrap();
        match t {
            EditorTarget::Builtin {
                plugin_id, editor, ..
            } => {
                assert_eq!(plugin_id, "test-editor");
                assert_eq!(editor, "TestEditor");
            }
            _ => panic!("expected builtin"),
        }

        // Visio 无内置编辑器插件
        let err = router
            .route(&storage, "03-台账/模具台账.xlsx", Format::Visio, EditMode::Builtin)
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_without_plugin_reports_actionable_error() {
        // Visio 按设计矩阵没有内置编辑器插件（README §9.3）。
        let dir = sample_vault("no-plugin");
        let storage = crate::storage::FsStorage::new(&dir);
        let err = EditorRouter::new()
            .route(
                &storage,
                "02-图示/模具布局.vsdx",
                Format::Visio,
                EditMode::Builtin,
            )
            .unwrap_err();
        assert!(format!("{err}").contains("插件"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_open_command_uses_os_default_app() {
        let cmd = system_open_command(std::path::Path::new("D:/我的库/模具台账.xlsx"));
        #[cfg(target_os = "windows")]
        {
            assert_eq!(cmd.program, "cmd");
            assert_eq!(cmd.args[1], "start");
            assert_eq!(cmd.args[2], "", "start 的第一个引号参数是窗口标题");
            assert!(cmd.args[3].contains("模具台账.xlsx"));
        }
        #[cfg(target_os = "macos")]
        assert_eq!(cmd.program, "open");
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(cmd.program, "xdg-open");
    }

    #[test]
    fn external_route_defaults_to_system_app_and_allows_override() {
        let dir = sample_vault("external");
        let storage = crate::storage::FsStorage::new(&dir);
        let router = EditorRouter::new();

        for (path, format) in [
            ("02-图示/评审流程.drawio", Format::Drawio),
            ("02-图示/模具布局.vsdx", Format::Visio),
        ] {
            let t = router.route(&storage, path, format, EditMode::External).unwrap();
            match t {
                EditorTarget::External { program, path: abs } => {
                    assert!(program.is_empty(), "默认应交给系统默认应用");
                    assert!(abs.exists(), "目标文件应真实存在: {}", abs.display());
                }
                _ => panic!("expected external"),
            }
        }

        let mut router = EditorRouter::new();
        router.set_external_program(Format::Drawio, "my-drawio");
        let t = router
            .route(&storage, "02-图示/评审流程.drawio", Format::Drawio, EditMode::External)
            .unwrap();
        assert!(matches!(t, EditorTarget::External { program, .. } if program == "my-drawio"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_external_rejects_missing_file_without_spawning() {
        let target = EditorTarget::External {
            program: String::new(),
            path: std::env::temp_dir().join(format!("mkv-missing-{}.docx", std::process::id())),
        };
        assert!(matches!(
            EditorRouter::launch_external(&target),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn session_save_delegates_to_plugin_serialize() {
        install_test_plugin();
        let dir = sample_vault("save");
        let storage = crate::storage::FsStorage::new(&dir);
        let t = EditorRouter::new()
            .route(&storage, "03-台账/模具台账.xlsx", Format::Excel, EditMode::Builtin)
            .unwrap();
        let EditorTarget::Builtin { mut session, .. } = t else {
            panic!("expected builtin");
        };
        crate::plugin::apply_action(
            &mut session,
            &PluginAction::SetParagraph {
                index: 0,
                text: "x".into(),
            },
        )
        .unwrap();
        let bytes = session.save(&storage).unwrap();
        assert_eq!(bytes.last(), Some(&7));
        assert_eq!(storage.read("03-台账/模具台账.xlsx").unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn contract_version_is_v1() {
        assert_eq!(PLUGIN_CONTRACT_VERSION, 1);
        assert_eq!(format_key(Format::PowerPoint), "pptx");
    }
}

//! 原生 Office 应用内定位（F-06 进阶 ③）。
//!
//! # 目标
//!
//! F-06 基础版只能「在库内跳到目标片段」——定位到的是**抽取后的文本片段**，
//! 不是原生 Office 应用里的那个对象。本模块补上后半截：点击引用卡片时，
//! 还能**唤起原生应用并把光标落到那个位置**（xlsx 选中某单元格、pptx 翻到某页）。
//!
//! # 分层约定
//!
//! 本模块只做**纯函数推导**：`(格式, 相对路径, 片段定位信息, 目标平台)` → 一份
//! 「怎么唤起」的方案（[`OfficeLaunchPlan`］）。
//!
//! **不 spawn 进程**：core 的 wasm 段不允许出现平台特定代码，进程调用留给
//! Desktop 端（`packages/desktop`）按方案执行。方案里的命令一律是
//! **argv 形态**（程序 + 参数数组），不是拼好的命令行字符串。
//!
//! # 能力矩阵（诚实降级）
//!
//! 「能定位」这件事在不同格式之间差别极大，本模块**不假装**：
//!
//! | 格式 | 目标粒度 | 应用内定位 | 手段 |
//! |------|----------|-----------|------|
//! | Excel | 单元格（库内实为**行**粒度） | ✅ 需工作表名 | `ms-excel:ofe|u|<file-url>#<工作表>!<单元格>` |
//! | PowerPoint | 页 | ✅ | `ms-powerpoint:ofe|u|<file-url>#slide=<n>` |
//! | Word | 段 | ❌ | 无公开的「跳到第 N 段」开关／协议锚点，退化为打开文件 |
//! | Visio | 页／图形 | ❌ | `ms-visio:` 只保证打开文件，无页级／图形级锚点 |
//! | Drawio | 节点 | ❌ | 无稳定的节点级外部定位协议，退化为打开文件 |
//! | Markdown | 段 | ❌ | 由库内原生渲染承载，外部编辑器按段序号定位无稳定协议 |
//!
//! 另外，**只有 Windows 具备「一条命令完成应用内定位」的能力**：
//! `ms-excel:` / `ms-word:` / `ms-powerpoint:` 是 Windows 注册的协议，
//! macOS 与 Linux 上既没有等价协议，也没有等价的单条命令行定位手段，
//! 因此在这两个平台上一律降级为「打开文件」。
//!
//! （macOS 的 Excel / Word 确实可以用 AppleScript 选中单元格或跳到段落，
//! 但它要求**文件已经打开**、依赖应用已启动，属于「应用内自动化」而非
//! 「一条命令唤起并定位」，且脚本内容随应用版本、语言环境而变，
//! 超出本模块的推导职责，故不纳入。）
//!
//! # 关于协议锚点的诚实声明
//!
//! 微软官方文档（*Office URI 结构描述*、*Command-line switches for Microsoft
//! Office products*）只承诺 `ms-<app>:ofe|u|<URI>` 能**打开**文档，且示例 URI
//! 都是 `http` / `https`；`#<工作表>!<单元格>` 与 `#slide=<n>` 这两个锚点属于
//! **社区通行做法**，官方文档未收录。因此本模块的 `Precise` 表示
//! 「协议 URI 已带上定位锚点」，不表示「所有 Office 版本都保证采纳该锚点」；
//! 未被采纳时的表现是**文件已打开、光标停在默认位置**。这一点在方案的
//! [`note`](OfficeLaunchPlan::note) 里会原样写明。
//!
//! # 已知边界（Excel 行／列口径）
//!
//! 库内对 Excel 的引用粒度是**行**（`locate::TextSegment` 的 `kind` 为 `row`），
//! 因此从引用能拿到的信息是「某工作表的第 N 个抽取行」，而不是「某个单元格」：
//!
//! - **列号不可考**：抽取器把每行的非空单元格用 ` | ` 拼接，空单元格被丢弃，
//!   列位置在文本层面已经丢失。因此行级引用推导出的单元格**默认取该行首列（A）**。
//! - **行号是工作簿级的**：`locate.rs` 的行计数器跨工作表累计、不按表重置，
//!   多工作表工作簿里「第 N 行」是**全簿第 N 个非空行**，未必等于表内行号。
//!   多表工作簿的锚点可能因此偏移到相邻行。
//!
//! 要精确到具体单元格，调用方应在定位信息里直接给出**表内行号与列号**
//! （例如选择器写成 `台账!C2`，或自行构造 [`Fragment::Cell`]）。
//! 本模块不替调用方猜测这些信息。
//!
//! # 中文路径
//!
//! 本项目知识库大量使用中文目录名（`03-台账/模具台账.xlsx`），因此 Windows 上
//! 的协议 URL 必须**正确转义**：盘符、反斜杠、空格、中文都要处理。
//! 转义规则见 [`path_to_file_url`]。
//!
//! 另外，`cmd /C start "" <uri>` 会经过 cmd 自己的解析：URI 里的 `%XX` 会被
//! cmd 当成 `%VAR%` 环境变量展开而损坏。因此本模块在 Windows 上默认用
//! **不经 shell 的直接 argv** 唤起（`rundll32.exe url.dll,FileProtocolHandler`），
//! 见 [`windows_protocol_command`]；`cmd /C start` 形式作为备选由
//! [`windows_start_command`] 提供。

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::format::Format;

/// Excel 最大列数（`XFD` 列）。
pub const EXCEL_MAX_COLUMN: usize = 16_384;

/// Excel 最大行数。
pub const EXCEL_MAX_ROW: usize = 1_048_576;

/// 目标平台。
///
/// 唤起方式随平台而异，因此它是**显式入参**而非编译期常量：
/// 这样单测可以在任意宿主上覆盖三套平台行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Windows：注册了 `ms-excel:` / `ms-word:` / `ms-powerpoint:` 等协议。
    Windows,
    /// macOS：`open` 唤起，没有等价的应用内定位协议。
    MacOs,
    /// Linux / BSD：`xdg-open` 唤起，没有等价的应用内定位协议。
    Linux,
}

/// 定位能力等级。
///
/// 二者的区别是**硬结论**，不是「尽量而为」：不支持就是不支持。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocateCapability {
    /// 可在原生应用内定位到片段级位置（协议 URI 已带上定位锚点）。
    Precise,
    /// 只能打开文件，无法在应用内定位到片段（降级）。
    OpenOnly,
}

/// 片段定位信息。
///
/// 这是「目标落在哪」的规范化描述，由 [`fragment_from_selector`]（从引用选择器
/// 原文推导）或 [`fragment_from_label`]（从 `locate::TextSegment::label`
/// 推导）产出，也可以由调用方直接构造。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Fragment {
    /// Excel 单元格。行／列均为 **1-based**；`sheet` 为工作表名。
    Cell {
        /// 工作表名；缺失时无法构造 `#工作表!单元格` 锚点。
        sheet: Option<String>,
        /// 行号（1-based）。
        row: usize,
        /// 列号（1-based）。
        ///
        /// `None` 表示**列号不可考**：库内对 Excel 的引用粒度是行，
        /// 抽取时已丢弃空单元格，列位置在文本层面丢失。
        /// 此时锚点落在该行的首列（`A`），见模块文档的「已知边界」。
        column: Option<usize>,
    },
    /// PowerPoint 幻灯片页码（1-based）。
    Slide {
        /// 页码（1-based）。
        page: usize,
    },
    /// Word 段落序号（1-based）。
    Paragraph {
        /// 段落序号（1-based）。
        index: usize,
    },
    /// Visio 页码（1-based）。
    Page {
        /// 页码（1-based）。
        index: usize,
    },
    /// 流程图节点（Drawio / Visio）。
    Node {
        /// 节点稳定 id（Drawio 的 `mxCell@id`、Visio 的 shape ID）或序号。
        id: String,
    },
    /// 无法定位到具体片段，只知道文件。
    Document,
}

/// 唤起命令（argv 形态）。
///
/// 与 [`crate::editor::ExternalOpenCommand`] 同构：程序名 + 参数数组，
/// 由宿主直接 `spawn`，**不经 shell**，因此参数里的 `%` / `&` 不会被二次解析。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommand {
    /// 可执行文件名。
    pub program: String,
    /// 参数数组。
    pub args: Vec<String>,
}

/// 唤起方案：怎么打开目标文件、能不能定位到片段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeLaunchPlan {
    /// 定位能力：`Precise` = 应用内可定位；`OpenOnly` = 只能打开文件。
    pub capability: LocateCapability,
    /// 目标格式。
    pub format: Format,
    /// 目标平台（决定 [`command`](OfficeLaunchPlan::command) 的形态）。
    pub platform: Platform,
    /// 目标文档相对路径（知识库内）。
    pub rel_path: String,
    /// 目标文档绝对路径。
    pub abs_path: String,
    /// 目标文档的 `file://` URL（已转义）。
    pub file_url: String,
    /// 片段定位信息。
    pub fragment: Fragment,
    /// 协议 URL（`ms-excel:` 等）；该格式／平台没有协议时为 `None`。
    pub protocol_uri: Option<String>,
    /// 推荐的唤起命令。
    pub command: LaunchCommand,
    /// 人类可读说明：怎么定位的，或**为什么降级**。
    pub note: String,
}

impl OfficeLaunchPlan {
    /// 是否可在原生应用内定位到片段。
    pub fn is_precise(&self) -> bool {
        self.capability == LocateCapability::Precise
    }
}

/// 当前宿主平台；wasm 等无进程能力的目标返回 `None`。
pub fn current_platform() -> Option<Platform> {
    if cfg!(target_os = "windows") {
        Some(Platform::Windows)
    } else if cfg!(target_os = "macos") {
        Some(Platform::MacOs)
    } else if cfg!(target_os = "linux") {
        Some(Platform::Linux)
    } else {
        None
    }
}

/// 规划「唤起原生 Office 应用并定位到片段」。
///
/// # 参数
///
/// - `abs_path`：目标文档的**绝对路径**（`Storage::absolute_path` 的结果）。
/// - `fragment`：片段定位信息，见 [`Fragment`]。
/// - `platform`：目标平台，决定唤起方式与可用协议。
///
/// 详见模块文档的[能力矩阵](crate::office_locator)。
pub fn plan_office_launch(
    format: Format,
    rel_path: &str,
    abs_path: &Path,
    fragment: &Fragment,
    platform: Platform,
) -> OfficeLaunchPlan {
    let abs = abs_path.to_string_lossy().to_string();
    let file_url = path_to_file_url(abs_path);
    let anchor = anchor_of(fragment);
    let (capability, protocol_uri, note) = resolve_launch(platform, format, &file_url, &anchor);
    let command = build_command(platform, format, &abs, protocol_uri.as_deref());
    OfficeLaunchPlan {
        capability,
        format,
        platform,
        rel_path: rel_path.to_string(),
        abs_path: abs,
        file_url,
        fragment: fragment.clone(),
        protocol_uri,
        command,
        note,
    }
}

/// 从引用选择器原文规划唤起（快捷入口）。
///
/// 选择器语义与 [`crate::refs::match_selector`] 对齐：纯数字按 1-based 序号理解，
/// `第N行` / `第N页` / `第N段` 取其中的数字，Excel 还额外支持 `A1` 记法。
pub fn plan_from_selector(
    format: Format,
    rel_path: &str,
    abs_path: &Path,
    selector: &str,
    platform: Platform,
) -> OfficeLaunchPlan {
    let fragment = fragment_from_selector(format, selector);
    plan_office_launch(format, rel_path, abs_path, &fragment, platform)
}

/// 从片段位置标签规划唤起（快捷入口）。
///
/// 标签取自 `locate::TextSegment::label`，例如「台账 · 第 2 行」「第 1 页」
/// 「第 3 段」。Excel 的标签里**带工作表名**，因此这条路径能拿到完整锚点。
pub fn plan_from_label(
    format: Format,
    rel_path: &str,
    abs_path: &Path,
    label: &str,
    platform: Platform,
) -> OfficeLaunchPlan {
    let fragment = fragment_from_label(format, label);
    plan_office_launch(format, rel_path, abs_path, &fragment, platform)
}

/// 从引用选择器原文推导片段定位信息。
///
/// # 选择器语义
///
/// - 空选择器 → [`Fragment::Document`]（只知道文件）。
/// - Excel：纯数字 → 行号（列缺省为 1）；`A1` 记法（如 `C2`）→ 行列俱全。
/// - PowerPoint：数字 → 页码；Word：数字 → 段落序号；
///   Visio：数字 → 页码；Drawio：整个选择器 → 节点 id。
/// - Markdown：数字 → 段落序号。
///
/// 注意：Excel 的行号取自**抽取文本的行序**（`locate::TextSegment` 的 row 序号），
/// 而该序号目前是**全工作簿累计**（跨工作表不重置），因此**推导不出工作表名**——
/// 需要工作表名时请改用 [`fragment_from_label`]。
pub fn fragment_from_selector(format: Format, selector: &str) -> Fragment {
    let compact: String = selector.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Fragment::Document;
    }
    match format {
        Format::Excel => {
            // `工作表!单元格` 形式：工作表名与单元格都明确。
            // 注意这里用**未压缩**的选择器去切分，否则含空格的工作表名会被压掉。
            if let Some((sheet, cell)) = selector.trim().split_once('!') {
                let name = sheet.trim().trim_matches('\'').to_string();
                let cell: String = cell.chars().filter(|c| !c.is_whitespace()).collect();
                return match parse_a1(&cell) {
                    Some((row, column)) => Fragment::Cell {
                        sheet: if name.is_empty() { None } else { Some(name) },
                        row,
                        column: Some(column),
                    },
                    None => Fragment::Document,
                };
            }
            // 裸 A1 记法：行列俱全，但工作表未知。
            if let Some((row, column)) = parse_a1(&compact) {
                return Fragment::Cell {
                    sheet: None,
                    row,
                    column: Some(column),
                };
            }
            // 纯数字 / 「第N行」：只有行号，列不可考。
            match leading_number(&compact) {
                Some(row) => Fragment::Cell {
                    sheet: None,
                    row,
                    column: None,
                },
                None => Fragment::Document,
            }
        }
        Format::PowerPoint => match leading_number(&compact) {
            Some(page) => Fragment::Slide { page },
            None => Fragment::Document,
        },
        Format::Word => match leading_number(&compact) {
            Some(index) => Fragment::Paragraph { index },
            None => Fragment::Document,
        },
        Format::Visio => match leading_number(&compact) {
            Some(index) => Fragment::Page { index },
            None => Fragment::Document,
        },
        Format::Drawio => Fragment::Node { id: compact },
        Format::Markdown => match leading_number(&compact) {
            Some(index) => Fragment::Paragraph { index },
            None => Fragment::Document,
        },
    }
}

/// 从片段位置标签推导片段定位信息。
///
/// 标签形如「台账 · 第 2 行」（Excel，带工作表名）、「第 1 页」（PPT／Visio）、
/// 「第 3 段」（Word／Markdown）、「节点 3」（Drawio）。
///
/// 这是 Excel 唯一能拿到**工作表名**的路径，而工作表名是构造
/// `#<工作表>!<单元格>` 锚点的必要成分。
pub fn fragment_from_label(format: Format, label: &str) -> Fragment {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Fragment::Document;
    }
    match format {
        Format::Excel => {
            let (sheet, rest) = match trimmed.split_once(" · ") {
                Some((left, right)) => {
                    let name = left.trim();
                    (if name.is_empty() { None } else { Some(name.to_string()) }, right)
                }
                None => (None, trimmed),
            };
            match leading_number(rest) {
                Some(row) => Fragment::Cell {
                    sheet,
                    row,
                    column: None,
                },
                None => Fragment::Document,
            }
        }
        Format::PowerPoint => match leading_number(trimmed) {
            Some(page) => Fragment::Slide { page },
            None => Fragment::Document,
        },
        Format::Word | Format::Markdown => match leading_number(trimmed) {
            Some(index) => Fragment::Paragraph { index },
            None => Fragment::Document,
        },
        Format::Visio => match leading_number(trimmed) {
            Some(index) => Fragment::Page { index },
            None => Fragment::Document,
        },
        Format::Drawio => match leading_number(trimmed) {
            Some(n) => Fragment::Node { id: n.to_string() },
            None => Fragment::Node {
                id: trimmed.to_string(),
            },
        },
    }
}

/// A1 记法单元格地址（行号 + 列号 → `C2`）。
///
/// 行／列均为 **1-based**；超出 Excel 有效范围时返回 `None`。
/// 列号省略时（库内行级引用）取首列 `A`。
pub fn cell_address(row: usize, column: usize) -> Option<String> {
    let col = column_label(column)?;
    if row == 0 || row > EXCEL_MAX_ROW {
        return None;
    }
    Some(format!("{col}{row}"))
}

/// 列号 → 列字母（1 → `A`，26 → `Z`，27 → `AA`）。
///
/// 超出 Excel 有效列数（`XFD` = 16384）时返回 `None`。
pub fn column_label(column: usize) -> Option<String> {
    if column == 0 || column > EXCEL_MAX_COLUMN {
        return None;
    }
    let mut n = column;
    let mut buf: Vec<char> = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        buf.push((b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    buf.reverse();
    Some(buf.into_iter().collect())
}

/// A1 记法文本 → `(行号, 列号)`（均为 1-based）。
///
/// 例如 `C2` → `(2, 3)`。非法输入返回 `None`。
pub fn parse_a1(text: &str) -> Option<(usize, usize)> {
    let s = text.trim();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let column = column_index(letters)?;
    let row = digits.parse::<usize>().ok()?;
    if row == 0 || row > EXCEL_MAX_ROW {
        return None;
    }
    Some((row, column))
}

/// 本地路径 → `file://` URL（已转义）。
///
/// # 转义规则
///
/// - Windows 反斜杠统一换成正斜杠；
/// - 盘符 `C:` 原样保留（`file:///C:/...`，冒号在路径段是合法字符）；
/// - UNC 路径 `\\server\share\a.xlsx` → `file://server/share/a.xlsx`；
/// - 空格编码为 `%20`；非 ASCII（中文）按 UTF-8 逐字节编码为 `%E4%B8%AD` 形式；
/// - `#` / `?` / `%` 等在路径段有特殊含义的字符一律编码，
///   否则会被误当成 fragment 或 query。
///
/// 目录分隔符 `/` 与盘符冒号保持原样，其余按 RFC 3986 的 `pchar` 规则处理。
pub fn path_to_file_url(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = raw.strip_prefix("//") {
        // UNC：\\server\share\file → file://server/share/file
        let mut parts = rest.splitn(2, '/');
        let host = parts.next().unwrap_or("");
        let tail = parts.next().unwrap_or("");
        if host.is_empty() {
            return format!("file:///{}", encode_path(tail));
        }
        return format!("file://{host}/{}", encode_path(tail));
    }
    let encoded = encode_path(&raw);
    if raw.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// Excel 单元格定位 URI。
///
/// 形如 `ms-excel:ofe|u|file:///C:/.../台账.xlsx#台账!C2`。
///
/// `ofe` = Open For Edit，`u` = 随后的 URI 描述符。工作表名按 Excel 约定
/// 在含空格／特殊字符时用单引号包起来（`'Sales Data'!A1`）。
pub fn excel_cell_uri(file_url: &str, sheet: &str, cell: &str) -> String {
    format!("ms-excel:ofe|u|{file_url}#{}", excel_sheet_anchor(sheet, cell))
}

/// PowerPoint 幻灯片定位 URI。
///
/// 形如 `ms-powerpoint:ofe|u|file:///C:/.../方案.pptx#slide=3`。
pub fn powerpoint_slide_uri(file_url: &str, page: usize) -> String {
    format!("ms-powerpoint:ofe|u|{file_url}#slide={page}")
}

/// 「仅打开文件」的 Office 协议 URI（无定位锚点）。
///
/// 支持 Excel / PowerPoint / Word / Visio；Drawio 与 Markdown 没有对应的
/// Windows 协议，返回 `None`。
pub fn office_open_uri(format: Format, file_url: &str) -> Option<String> {
    let scheme = match format {
        Format::Excel => "ms-excel",
        Format::PowerPoint => "ms-powerpoint",
        Format::Word => "ms-word",
        Format::Visio => "ms-visio",
        Format::Drawio | Format::Markdown => return None,
    };
    Some(format!("{scheme}:ofe|u|{file_url}"))
}

/// Windows 上不经 shell 直接唤起协议 URI 的命令。
///
/// 用 `rundll32.exe url.dll,FileProtocolHandler <uri>`：参数原样交给
/// `ShellExecute`，**不会**像 `cmd /C start` 那样把 URI 里的 `%XX` 当成
/// `%VAR%` 环境变量展开。这对中文路径是硬要求。
pub fn windows_protocol_command(uri: &str) -> LaunchCommand {
    LaunchCommand {
        program: "rundll32.exe".into(),
        args: vec!["url.dll,FileProtocolHandler".into(), uri.to_string()],
    }
}

/// Windows 上用 `cmd /C start "" <uri>` 唤起协议 URI 的命令。
///
/// 与 [`crate::editor::system_open_command`] 的惯例一致（第一个空串是窗口标题）。
///
/// **注意**：cmd 会展开 `%VAR%`，因此 URI 里含 `%XX` 转义时会被破坏；
/// 中文路径请优先用 [`windows_protocol_command`]。
pub fn windows_start_command(uri: &str) -> LaunchCommand {
    LaunchCommand {
        program: "cmd".into(),
        args: vec!["/C".into(), "start".into(), String::new(), uri.to_string()],
    }
}

// ---------------------------------------------------------------- 内部实现

/// 片段能否被原生应用锚定。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    /// Excel 单元格（有工作表名，锚点完整）。
    Cell {
        sheet: String,
        addr: String,
        /// 列号是否由调用方明确给出。
        ///
        /// `false` 表示这是**行级引用**（库内 Excel 粒度），列号不可考，
        /// 锚点落在该行首列；说明文案要如实区分这两种情况。
        column_known: bool,
    },
    /// Excel 单元格（有行列但缺工作表名，锚点不完整）。
    CellWithoutSheet { addr: String },
    /// PowerPoint 幻灯片。
    Slide { page: usize },
    /// 该格式／平台没有可用的定位锚点。
    None,
}

/// 把 [`Fragment`] 归约成「能否锚定」。
fn anchor_of(fragment: &Fragment) -> Anchor {
    match fragment {
        Fragment::Cell {
            sheet: Some(sheet),
            row,
            column,
        } => match cell_address(*row, column.unwrap_or(1)) {
            Some(addr) => Anchor::Cell {
                sheet: sheet.clone(),
                addr,
                column_known: column.is_some(),
            },
            None => Anchor::None,
        },
        Fragment::Cell {
            sheet: None,
            row,
            column,
        } => match cell_address(*row, column.unwrap_or(1)) {
            Some(addr) => Anchor::CellWithoutSheet { addr },
            None => Anchor::None,
        },
        Fragment::Slide { page } if *page >= 1 => Anchor::Slide { page: *page },
        _ => Anchor::None,
    }
}

/// 决定定位能力、协议 URI 与说明文案。
fn resolve_launch(
    platform: Platform,
    format: Format,
    file_url: &str,
    anchor: &Anchor,
) -> (LocateCapability, Option<String>, String) {
    if platform != Platform::Windows {
        return (
            LocateCapability::OpenOnly,
            None,
            format!(
                "{} 没有注册 `ms-*:` 协议，也没有等价的应用内定位命令行，\
                 无法在应用内定位到目标位置，退化为用系统默认应用打开文件。",
                platform_name(platform)
            ),
        );
    }
    match (format, anchor) {
        (Format::Excel, Anchor::Cell {
            sheet,
            addr,
            column_known,
        }) => {
            let caveat = if *column_known {
                "锚点精确到指定单元格。"
            } else {
                "注意：这是**行级引用**——库内对 Excel 的粒度是行，\
                 抽取时已丢弃空单元格，列位置不可考，因此锚点落在该行首列 `A`。"
            };
            (
                LocateCapability::Precise,
                Some(excel_cell_uri(file_url, sheet, addr)),
                format!(
                    "通过 `ms-excel:` 协议 URI 的 `#{sheet}!{addr}` 锚点请求在 Excel 中选中该位置。\
                     {caveat}\
                     另需注意：行号取自抽取文本的行序，是**工作簿级累计**的，\
                     多工作表工作簿可能偏移到相邻行。\
                     锚点写法是社区通行做法，微软官方文档只承诺 `ofe|u|<URI>` 能打开文件，\
                     是否被本机 Office 版本采纳需实机验证；未采纳时表现为文件已打开、光标停在默认位置。"
                ),
            )
        }
        (Format::Excel, Anchor::CellWithoutSheet { addr }) => (
            LocateCapability::OpenOnly,
            office_open_uri(format, file_url),
            format!(
                "选择器给出了单元格 {addr} 但没有工作表名，无法构造 `#{{工作表}}!{addr}` 锚点，\
                 退化为在 Excel 中打开文件。"
            ),
        ),
        (Format::PowerPoint, Anchor::Slide { page }) => (
            LocateCapability::Precise,
            Some(powerpoint_slide_uri(file_url, *page)),
            format!(
                "通过 `ms-powerpoint:` 协议 URI 的 `#slide={page}` 锚点请求翻到第 {page} 页。\
                 该锚点属于社区做法，微软官方文档只承诺 `ofe|u|<URI>` 能打开演示文稿，\
                 需实机验证；未采纳时表现为文件已打开、停在第一页。"
            ),
        ),
        (Format::Word, _) => (
            LocateCapability::OpenOnly,
            office_open_uri(format, file_url),
            "Word 没有公开的「跳到第 N 段」命令行开关或协议锚点\
             （官方开关只有 /t /m /h 等，均不涉及段落定位），\
             无法在应用内定位到目标段落，退化为打开文档后由用户自行定位。"
                .to_string(),
        ),
        (Format::Visio, _) => (
            LocateCapability::OpenOnly,
            office_open_uri(format, file_url),
            "Visio 未公开页级／图形级定位协议（`ms-visio:` 只保证打开文件），\
             无法在应用内定位到目标页，退化为打开文件。"
                .to_string(),
        ),
        (Format::Drawio, _) => (
            LocateCapability::OpenOnly,
            None,
            "Drawio 没有稳定的节点级外部定位协议（`drawio://` 也未提供节点锚点），\
             无法在应用内定位到目标节点，退化为用系统默认应用打开文件。"
                .to_string(),
        ),
        (Format::Markdown, _) => (
            LocateCapability::OpenOnly,
            None,
            "Markdown 由库内原生渲染承载，段级定位应走库内跳转；\
             外部编辑器按段序号定位没有稳定协议，退化为打开文件。"
                .to_string(),
        ),
        (Format::Excel, _) => (
            LocateCapability::OpenOnly,
            office_open_uri(format, file_url),
            "行号或列号超出 Excel 有效范围，无法构造单元格锚点，退化为在 Excel 中打开文件。"
                .to_string(),
        ),
        (Format::PowerPoint, _) => (
            LocateCapability::OpenOnly,
            office_open_uri(format, file_url),
            "选择器没有给出有效页码，无法构造 `#slide=<n>` 锚点，退化为在 PowerPoint 中打开文件。"
                .to_string(),
        ),
    }
}

/// 按平台与格式组装唤起命令。
fn build_command(
    platform: Platform,
    format: Format,
    abs_path: &str,
    protocol_uri: Option<&str>,
) -> LaunchCommand {
    match platform {
        Platform::Windows => match protocol_uri {
            Some(uri) => windows_protocol_command(uri),
            None => LaunchCommand {
                program: "cmd".into(),
                args: vec!["/C".into(), "start".into(), String::new(), abs_path.to_string()],
            },
        },
        Platform::MacOs => match macos_app_name(format) {
            Some(app) => LaunchCommand {
                program: "open".into(),
                args: vec!["-a".into(), app.into(), abs_path.to_string()],
            },
            None => LaunchCommand {
                program: "open".into(),
                args: vec![abs_path.to_string()],
            },
        },
        Platform::Linux => LaunchCommand {
            program: "xdg-open".into(),
            args: vec![abs_path.to_string()],
        },
    }
}

/// macOS 上对应的原生应用名；没有对应应用时返回 `None`。
fn macos_app_name(format: Format) -> Option<&'static str> {
    match format {
        Format::Excel => Some("Microsoft Excel"),
        Format::PowerPoint => Some("Microsoft PowerPoint"),
        Format::Word => Some("Microsoft Word"),
        Format::Drawio | Format::Visio | Format::Markdown => None,
    }
}

/// 平台中文名（说明文案用）。
fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "Windows",
        Platform::MacOs => "macOS",
        Platform::Linux => "Linux",
    }
}

/// Excel 工作表锚点：`工作表!单元格`（必要时给工作表名加单引号）。
///
/// 表名在 fragment 里**保持字面量**（不 percent-encode）——这与 Excel 自己写
/// 内部超链接时的做法一致（锚点形如 `#'Sales Data'!A1`，表名是原样的）。
/// 唯一例外是 `#`：它会提前结束 fragment，必须编码成 `%23`。
fn excel_sheet_anchor(sheet: &str, cell: &str) -> String {
    format!("{}!{}", quote_sheet_name(sheet).replace('#', "%23"), cell)
}

/// 按 Excel 约定给工作表名加单引号。
///
/// Excel 的规则（与 Excel 自身写超链接时的行为一致）：
///
/// 1. 含**空格或标点**（非字母数字、非 `_`、非 `.`）→ 必须加引号；
/// 2. **以数字或 `.` 开头** → 必须加引号（否则会被当成数字）；
/// 3. 形如**合法 A1 引用**（`A1`、`XFD1048576`）→ 必须加引号，
///    否则 `#A1!C2` 会被解析成「单元格 A1 的 C2」而不是「表 A1 的 C2」。
///
/// 字母数字判定用 Unicode 语义（`char::is_alphanumeric`），
/// 因此**中文表名不加引号**（`台账!C2`）——这正是本项目中文表名需要的。
fn quote_sheet_name(name: &str) -> String {
    if !name.starts_with('\'') && sheet_name_requires_quoting(name) {
        // Excel 用两个单引号转义表名内部的单引号。
        return format!("'{}'", name.replace('\'', "''"));
    }
    name.to_string()
}

/// 工作表名是否需要加引号（见 [`quote_sheet_name`] 的规则）。
fn sheet_name_requires_quoting(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    // 规则 1：只允许字母数字、`_`、`.`。
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return true;
    }
    // 规则 2：以数字或 `.` 开头。
    if name.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return true;
    }
    // 规则 3：形如合法 A1 引用。
    if looks_like_a1_reference(name) {
        return true;
    }
    // 规则 4：以 R / RC / C 这类 R1C1 引用开头。
    let upper = name.to_ascii_uppercase();
    matches!(upper.as_str(), "R" | "C" | "RC")
}

/// 表名是否形如合法的 A1 单元格引用。
fn looks_like_a1_reference(name: &str) -> bool {
    let Some(pos) = name.find(|c: char| c.is_ascii_digit()) else {
        return false;
    };
    let letters = &name[..pos];
    let digits = &name[pos..];
    if letters.is_empty() || letters.len() > 3 {
        return false;
    }
    if !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(row) = digits.parse::<usize>() else {
        return false;
    };
    let Some(column) = column_index(letters) else {
        return false;
    };
    row >= 1 && row <= EXCEL_MAX_ROW && column <= EXCEL_MAX_COLUMN
}

/// 取字符串中第一段连续 ASCII 数字。
fn leading_number(s: &str) -> Option<usize> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// 列字母 → 列号（1-based）。
fn column_index(letters: &str) -> Option<usize> {
    // 超过 3 个字母一定越界（`XFD` 是最大列），提前返回也避免 `usize` 溢出。
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n = 0usize;
    for c in letters.chars() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    if n == 0 || n > EXCEL_MAX_COLUMN {
        return None;
    }
    Some(n)
}

/// 路径段编码：逐段 percent-encode，分隔符 `/` 保留。
fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// 单个路径段的 percent-encode。
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if is_path_safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// 路径段中无需转义的字节（RFC 3986 `pchar` 去掉 `%` 与 `/`）。
fn is_path_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b':'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b'@'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(p: &str) -> &Path {
        Path::new(p)
    }

    // ---------------- A1 记法 ----------------

    #[test]
    fn cell_address_converts_row_and_column() {
        // 任务给定的样例：行 2 列 3 → C2
        assert_eq!(cell_address(2, 3).as_deref(), Some("C2"));
        assert_eq!(cell_address(1, 1).as_deref(), Some("A1"));
        assert_eq!(cell_address(2, 1).as_deref(), Some("A2"));
        assert_eq!(cell_address(10, 26).as_deref(), Some("Z10"));
    }

    #[test]
    fn column_label_crosses_z_boundary() {
        assert_eq!(column_label(26).as_deref(), Some("Z"));
        assert_eq!(column_label(27).as_deref(), Some("AA"));
        assert_eq!(column_label(52).as_deref(), Some("AZ"));
        assert_eq!(column_label(53).as_deref(), Some("BA"));
        assert_eq!(column_label(EXCEL_MAX_COLUMN).as_deref(), Some("XFD"));
    }

    #[test]
    fn out_of_range_row_and_column_are_rejected() {
        assert!(cell_address(0, 1).is_none());
        assert!(cell_address(1, 0).is_none());
        assert!(cell_address(EXCEL_MAX_ROW + 1, 1).is_none());
        assert!(cell_address(1, EXCEL_MAX_COLUMN + 1).is_none());
        assert!(column_label(0).is_none());
        assert!(column_label(EXCEL_MAX_COLUMN + 1).is_none());
    }

    #[test]
    fn parse_a1_reads_back_what_cell_address_writes() {
        assert_eq!(parse_a1("C2"), Some((2, 3)));
        assert_eq!(parse_a1("XFD1048576"), Some((1_048_576, 16_384)));
        assert_eq!(parse_a1("AA1"), Some((1, 27)));
        assert_eq!(parse_a1("2"), None);
        assert_eq!(parse_a1("C0"), None);
        assert_eq!(parse_a1("节点3"), None);
    }

    // ---------------- file:// URL 转义 ----------------

    #[test]
    fn windows_chinese_path_is_percent_encoded() {
        let url = path_to_file_url(abs(r"C:\Users\xiexi\我的库\03-台账\模具台账.xlsx"));
        assert_eq!(
            url,
            "file:///C:/Users/xiexi/%E6%88%91%E7%9A%84%E5%BA%93/03-%E5%8F%B0%E8%B4%A6/%E6%A8%A1%E5%85%B7%E5%8F%B0%E8%B4%A6.xlsx"
        );
        assert!(!url.contains('\\'), "反斜杠必须换成正斜杠");
        assert!(!url.contains(' '), "空格必须转义");
    }

    #[test]
    fn windows_path_with_space_keeps_drive_colon_literal() {
        let url = path_to_file_url(abs(r"C:\Program Files\My Vault\台账.xlsx"));
        assert_eq!(
            url,
            "file:///C:/Program%20Files/My%20Vault/%E5%8F%B0%E8%B4%A6.xlsx"
        );
        assert!(url.starts_with("file:///C:/"), "盘符冒号不转义");
    }

    #[test]
    fn posix_and_unc_paths_map_to_file_urls() {
        assert_eq!(
            path_to_file_url(abs("/home/me/我的库/a.md")),
            "file:///home/me/%E6%88%91%E7%9A%84%E5%BA%93/a.md"
        );
        assert_eq!(
            path_to_file_url(abs(r"\\server\share\台账.xlsx")),
            "file://server/share/%E5%8F%B0%E8%B4%A6.xlsx"
        );
    }

    #[test]
    fn hash_and_question_mark_in_path_are_escaped() {
        let url = path_to_file_url(abs("/tmp/a#b?c.xlsx"));
        assert!(url.contains("%23"), "# 必须转义，否则会被当成 fragment");
        assert!(url.contains("%3F"), "? 必须转义，否则会被当成 query");
    }

    // ---------------- Excel ----------------

    #[test]
    fn excel_cell_uri_carries_sheet_and_a1_anchor() {
        let uri = excel_cell_uri("file:///C:/a.xlsx", "台账", "C2");
        assert_eq!(uri, "ms-excel:ofe|u|file:///C:/a.xlsx#台账!C2");
        // 带空格的工作表名按 Excel 约定加单引号
        assert_eq!(
            excel_cell_uri("file:///C:/a.xlsx", "Sales Data", "A1"),
            "ms-excel:ofe|u|file:///C:/a.xlsx#'Sales Data'!A1"
        );
    }

    #[test]
    fn sheet_name_quoting_follows_excel_rules() {
        // 中文表名按 Unicode 字母数字判定，不加引号。
        assert_eq!(excel_cell_uri("u", "台账", "A1"), "ms-excel:ofe|u|u#台账!A1");
        // 空格 / 标点 → 加引号
        assert_eq!(
            excel_cell_uri("u", "Sales Data", "A1"),
            "ms-excel:ofe|u|u#'Sales Data'!A1"
        );
        assert_eq!(
            excel_cell_uri("u", "台-账", "A1"),
            "ms-excel:ofe|u|u#'台-账'!A1"
        );
        // 以数字开头 → 加引号（否则会被当成数字）
        assert_eq!(
            excel_cell_uri("u", "2024台账", "A1"),
            "ms-excel:ofe|u|u#'2024台账'!A1"
        );
        // 形如 A1 引用的表名 → 加引号（否则 `A1!C2` 会被误解析）
        assert_eq!(excel_cell_uri("u", "A1", "C2"), "ms-excel:ofe|u|u#'A1'!C2");
        // 表名内部的单引号按 Excel 规则转义成两个
        assert_eq!(
            excel_cell_uri("u", "Bob's data", "A1"),
            "ms-excel:ofe|u|u#'Bob''s data'!A1"
        );
        // 表名里的 `#` 会提前结束 fragment，必须编码
        assert_eq!(
            excel_cell_uri("u", "台#账", "A1"),
            "ms-excel:ofe|u|u#'台%23账'!A1"
        );
    }

    #[test]
    fn excel_plan_locates_cell_on_windows() {
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\我的库\03-台账\模具台账.xlsx"),
            &Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: Some(3),
            },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::Precise);
        assert!(plan.is_precise());
        let uri = plan.protocol_uri.expect("Excel 应有协议 URI");
        assert!(uri.starts_with("ms-excel:ofe|u|file:///C:/"), "{uri}");
        assert!(uri.ends_with("#台账!C2"), "{uri}");
        assert!(uri.contains("%E5%8F%B0%E8%B4%A6"), "中文路径要转义: {uri}");
        // 不经 shell 的直接 argv，避免 cmd 把 %XX 当环境变量展开
        assert_eq!(plan.command.program, "rundll32.exe");
        assert_eq!(plan.command.args[0], "url.dll,FileProtocolHandler");
        assert_eq!(plan.command.args[1], uri);
    }

    #[test]
    fn excel_without_sheet_name_degrades_to_open_only() {
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\我的库\03-台账\模具台账.xlsx"),
            &Fragment::Cell {
                sheet: None,
                row: 2,
                column: Some(1),
            },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::OpenOnly);
        assert!(!plan.is_precise());
        let uri = plan.protocol_uri.expect("Excel 仍有打开协议");
        assert!(!uri.contains('#'), "没有工作表名就不该有锚点: {uri}");
        assert!(plan.note.contains("没有工作表名"), "{}", plan.note);
    }

    #[test]
    fn excel_out_of_range_row_degrades_to_open_only() {
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\a.xlsx"),
            &Fragment::Cell {
                sheet: Some("台账".into()),
                row: EXCEL_MAX_ROW + 1,
                column: Some(1),
            },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::OpenOnly);
        assert!(plan.note.contains("超出"), "{}", plan.note);
    }

    #[test]
    fn excel_row_level_reference_anchors_to_first_column() {
        // 库内 Excel 粒度是行，列不可考 → 锚点落在该行首列 A。
        let plan = plan_from_label(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\a.xlsx"),
            "台账 · 第 2 行",
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::Precise);
        assert!(plan.protocol_uri.unwrap().ends_with("#台账!A2"));
        assert!(plan.note.contains("行级引用"), "{}", plan.note);
        assert!(plan.note.contains("首列"), "{}", plan.note);
    }

    #[test]
    fn excel_explicit_cell_anchor_note_says_precise() {
        // 列号明确时，说明文案不应再提「行级引用」。
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\a.xlsx"),
            &Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: Some(3),
            },
            Platform::Windows,
        );
        assert!(plan.note.contains("精确到指定单元格"), "{}", plan.note);
        assert!(!plan.note.contains("行级引用"), "{}", plan.note);
    }

    // ---------------- PowerPoint ----------------

    #[test]
    fn powerpoint_plan_locates_slide_on_windows() {
        let plan = plan_office_launch(
            Format::PowerPoint,
            "05-方案/项目方案.pptx",
            abs(r"C:\我的库\05-方案\项目方案.pptx"),
            &Fragment::Slide { page: 3 },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::Precise);
        let uri = plan.protocol_uri.expect("PPT 应有协议 URI");
        assert!(uri.starts_with("ms-powerpoint:ofe|u|file:///C:/"), "{uri}");
        assert!(uri.ends_with("#slide=3"), "{uri}");
        assert!(uri.contains("%E9%A1%B9%E7%9B%AE%E6%96%B9%E6%A1%88"), "{uri}");
    }

    // ---------------- 降级判定 ----------------

    #[test]
    fn word_degrades_because_no_paragraph_protocol() {
        let plan = plan_office_launch(
            Format::Word,
            "04-文档/模流分析报告.docx",
            abs(r"C:\我的库\04-文档\模流分析报告.docx"),
            &Fragment::Paragraph { index: 3 },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::OpenOnly);
        let uri = plan.protocol_uri.expect("Word 仍有打开协议");
        assert_eq!(uri, format!("ms-word:ofe|u|{}", plan.file_url));
        assert!(!uri.contains('#'), "Word 不该有段落锚点: {uri}");
        assert!(plan.note.contains("Word"), "{}", plan.note);
        assert!(plan.note.contains("退化"), "{}", plan.note);
    }

    #[test]
    fn visio_and_drawio_degrade_to_open_only() {
        let visio = plan_office_launch(
            Format::Visio,
            "02-图示/模具布局.vsdx",
            abs(r"C:\我的库\02-图示\模具布局.vsdx"),
            &Fragment::Page { index: 2 },
            Platform::Windows,
        );
        assert_eq!(visio.capability, LocateCapability::OpenOnly);
        assert!(visio
            .protocol_uri
            .as_deref()
            .is_some_and(|u| u.starts_with("ms-visio:")));

        let drawio = plan_office_launch(
            Format::Drawio,
            "02-图示/评审流程.drawio",
            abs(r"C:\我的库\02-图示\评审流程.drawio"),
            &Fragment::Node { id: "s1".into() },
            Platform::Windows,
        );
        assert_eq!(drawio.capability, LocateCapability::OpenOnly);
        assert!(drawio.protocol_uri.is_none(), "Drawio 没有官方协议");
        assert_eq!(drawio.command.program, "cmd", "退化为系统默认应用");
    }

    #[test]
    fn markdown_degrades_to_open_only() {
        let plan = plan_office_launch(
            Format::Markdown,
            "01-笔记/工艺参数基线.md",
            abs(r"C:\我的库\01-笔记\工艺参数基线.md"),
            &Fragment::Paragraph { index: 2 },
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::OpenOnly);
        assert!(plan.protocol_uri.is_none());
        assert!(plan.note.contains("库内"), "{}", plan.note);
    }

    #[test]
    fn non_windows_platforms_always_degrade() {
        for platform in [Platform::MacOs, Platform::Linux] {
            let plan = plan_office_launch(
                Format::Excel,
                "03-台账/模具台账.xlsx",
                abs("/Users/me/台账.xlsx"),
                &Fragment::Cell {
                    sheet: Some("台账".into()),
                    row: 2,
                    column: Some(3),
                },
                platform,
            );
            assert_eq!(plan.capability, LocateCapability::OpenOnly);
            assert!(plan.protocol_uri.is_none());
            assert!(plan.note.contains("没有"), "{}", plan.note);
        }
    }

    // ---------------- 跨平台命令 ----------------

    #[test]
    fn macos_uses_open_with_app_name() {
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs("/Users/me/台账.xlsx"),
            &Fragment::Document,
            Platform::MacOs,
        );
        assert_eq!(plan.command.program, "open");
        assert_eq!(plan.command.args, vec!["-a", "Microsoft Excel", "/Users/me/台账.xlsx"]);

        let drawio = plan_office_launch(
            Format::Drawio,
            "02-图示/评审流程.drawio",
            abs("/Users/me/评审流程.drawio"),
            &Fragment::Document,
            Platform::MacOs,
        );
        assert_eq!(drawio.command.program, "open");
        assert_eq!(drawio.command.args, vec!["/Users/me/评审流程.drawio"]);
    }

    #[test]
    fn linux_uses_xdg_open() {
        let plan = plan_office_launch(
            Format::PowerPoint,
            "05-方案/项目方案.pptx",
            abs("/home/me/项目方案.pptx"),
            &Fragment::Slide { page: 1 },
            Platform::Linux,
        );
        assert_eq!(plan.command.program, "xdg-open");
        assert_eq!(plan.command.args, vec!["/home/me/项目方案.pptx"]);
    }

    #[test]
    fn windows_start_command_matches_editor_convention() {
        let cmd = windows_start_command("ms-excel:ofe|u|file:///C:/a.xlsx");
        assert_eq!(cmd.program, "cmd");
        assert_eq!(cmd.args[1], "start");
        assert_eq!(cmd.args[2], "", "start 的第一个引号参数是窗口标题");
    }

    // ---------------- 选择器 / 标签解析 ----------------

    #[test]
    fn selector_parsing_matches_vault_semantics() {
        // 纯数字 = 1-based 序号；Excel 粒度是行 → 列不可考
        assert_eq!(
            fragment_from_selector(Format::Excel, "2"),
            Fragment::Cell {
                sheet: None,
                row: 2,
                column: None
            }
        );
        // 「第2行」→ 行号
        assert_eq!(
            fragment_from_selector(Format::Excel, "第2行"),
            Fragment::Cell {
                sheet: None,
                row: 2,
                column: None
            }
        );
        // A1 记法 → 行列俱全
        assert_eq!(
            fragment_from_selector(Format::Excel, "C2"),
            Fragment::Cell {
                sheet: None,
                row: 2,
                column: Some(3)
            }
        );
        // `工作表!单元格` → 工作表与单元格都明确
        assert_eq!(
            fragment_from_selector(Format::Excel, "台账!C2"),
            Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: Some(3)
            }
        );
        // 含空格的工作表名不能被压掉
        assert_eq!(
            fragment_from_selector(Format::Excel, "Sales Data!C2"),
            Fragment::Cell {
                sheet: Some("Sales Data".into()),
                row: 2,
                column: Some(3)
            }
        );
        assert_eq!(
            fragment_from_selector(Format::PowerPoint, "第1页"),
            Fragment::Slide { page: 1 }
        );
        assert_eq!(
            fragment_from_selector(Format::Word, "第3段"),
            Fragment::Paragraph { index: 3 }
        );
        assert_eq!(
            fragment_from_selector(Format::Visio, "第2页"),
            Fragment::Page { index: 2 }
        );
        assert_eq!(
            fragment_from_selector(Format::Drawio, "节点3"),
            Fragment::Node { id: "节点3".into() }
        );
    }

    #[test]
    fn empty_selector_yields_document_target() {
        for format in [
            Format::Markdown,
            Format::Excel,
            Format::PowerPoint,
            Format::Word,
            Format::Visio,
            Format::Drawio,
        ] {
            assert_eq!(
                fragment_from_selector(format, "   "),
                Fragment::Document,
                "{format:?} 的空选择器应退化为整篇文档"
            );
            assert_eq!(fragment_from_label(format, ""), Fragment::Document);
        }
    }

    #[test]
    fn label_parsing_recovers_excel_sheet_name() {
        assert_eq!(
            fragment_from_label(Format::Excel, "台账 · 第 2 行"),
            Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: None
            }
        );
        assert_eq!(
            fragment_from_label(Format::Excel, "第 7 行"),
            Fragment::Cell {
                sheet: None,
                row: 7,
                column: None
            }
        );
        assert_eq!(
            fragment_from_label(Format::PowerPoint, "第 1 页"),
            Fragment::Slide { page: 1 }
        );
        assert_eq!(
            fragment_from_label(Format::Word, "第 3 段"),
            Fragment::Paragraph { index: 3 }
        );
    }

    #[test]
    fn label_based_excel_plan_gets_full_anchor() {
        let plan = plan_from_label(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\我的库\03-台账\模具台账.xlsx"),
            "台账 · 第 2 行",
            Platform::Windows,
        );
        assert_eq!(plan.capability, LocateCapability::Precise);
        assert!(plan.protocol_uri.unwrap().ends_with("#台账!A2"));
    }

    #[test]
    fn current_platform_is_known_on_desktop() {
        // wasm 等无进程能力的目标返回 None；桌面目标必须是 Some
        assert_eq!(current_platform().is_some(), cfg!(not(target_arch = "wasm32")));
    }

    #[test]
    fn plan_serializes_and_round_trips() {
        let plan = plan_office_launch(
            Format::Excel,
            "03-台账/模具台账.xlsx",
            abs(r"C:\我的库\03-台账\模具台账.xlsx"),
            &Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: Some(3),
            },
            Platform::Windows,
        );
        let json = serde_json::to_string(&plan).expect("序列化应成功");
        // 能力等级用 snake_case，便于跨进程传递时阅读
        assert!(json.contains(r#""capability":"precise""#), "{json}");
        assert!(json.contains(r#""platform":"windows""#), "{json}");
        // 片段定位信息用内部标签枚举
        assert!(json.contains(r#""kind":"cell""#), "{json}");
        let back: OfficeLaunchPlan = serde_json::from_str(&json).expect("反序列化应成功");
        assert_eq!(back, plan);
    }

    #[test]
    fn fragment_round_trips_through_json() {
        for fragment in [
            Fragment::Cell {
                sheet: Some("台账".into()),
                row: 2,
                column: Some(3),
            },
            Fragment::Cell {
                sheet: None,
                row: 7,
                column: None,
            },
            Fragment::Slide { page: 3 },
            Fragment::Paragraph { index: 4 },
            Fragment::Page { index: 5 },
            Fragment::Node { id: "s1".into() },
            Fragment::Document,
        ] {
            let json = serde_json::to_string(&fragment).expect("序列化应成功");
            let back: Fragment = serde_json::from_str(&json).expect("反序列化应成功");
            assert_eq!(back, fragment);
        }
    }
}

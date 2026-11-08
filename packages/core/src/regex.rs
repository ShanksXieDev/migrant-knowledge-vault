//! 轻量正则引擎（F-12 正则检索）。
//!
//! # 为什么自己写而不是用 `regex` crate
//!
//! 本项目有一条硬约束：**wasm 产物 < 800KB，且 wasm32 依赖树零 C 依赖**
//! （见 README §12.7 / CI 质量门禁）。`regex` crate 会带来
//! `regex-syntax` + `regex-automata` + `aho-corasick` 的依赖闭包，
//! 对 wasm 体积的贡献是数十 KB 级别，而正则检索在本项目里只是
//! **一个可选的检索模式**——不值得为此牺牲体积门禁。
//!
//! 因此这里自研一个**零依赖**的回溯式引擎：
//!
//! - 只依赖 `std`，无 `unsafe`；
//! - 编译期把模式编译成**扁平指令表**，运行时用回溯 VM 匹配；
//! - 回溯用**显式堆栈**实现（不是递归），因此长文本不会爆调用栈；
//! - 带**回溯步数上限**，杜绝灾难性回溯把界面卡死。
//!
//! # 支持的语法
//!
//! | 语法 | 说明 |
//! |---|---|
//! | `.` | 任意字符（不含换行） |
//! | `*` `+` `?` | 零次 / 一次以上 / 可选（均支持贪婪与非贪婪 `*?`） |
//! | `{n}` `{n,}` `{n,m}` | 重复次数（支持非贪婪 `{n,m}?`） |
//! | `\|` | 择一 |
//! | `(...)` `(?:...)` | 分组 |
//! | `[...]` `[^...]` | 字符类（支持范围 `a-z`） |
//! | `^` `$` | 文本开头 / 文本结尾（**严格**语义：`$` 不在末尾换行前匹配） |
//! | `\d \w \s` `\D \W \S` | 数字 / 单词 / 空白及其取反 |
//! | `\n \r \t \0` | 控制字符 |
//! | `\.` `\*` `\+` `\?` `\(` `\)` `\[` `\]` `\{` `\}` `\|` `\^` `\$` `\\` `\/` `\-` | 转义元字符 |
//!
//! # 明确不支持（编译期报错，不静默降级）
//!
//! 反向引用（`\1`）、前后瞻（`(?=` `(?!` `(?<=` `(?<!`）、内联标志（`(?i)`）、
//! 命名捕获组（`(?<name>...`）、单词边界（`\b` `\B`）、Unicode 属性（`\p{...}`）、
//! 十六进制转义（`\x41`）与 `x` / `m` 模式。
//! 静默忽略这些语法会让用户以为匹配规则生效了，因此一律返回编译错误。
//!
//! # 匹配语义
//!
//! 与主流引擎一致：**左优先 + 贪婪优先**。同一位置上的多个匹配可能
//! 采用不同的切分方式（如 `.*` 会尽可能吃掉更多字符），
//! 这是正则的固有特性，与 `regex` crate 的 `find` 行为一致。
//!
//! 重复体**可以匹配空串**时（如 `(a*)*`），引擎会检测到「本轮没有
//! 消费任何字符」并终止该次重复——与 Python `re`、Rust `regex` 的行为一致。
//!
//! # 字节偏移
//!
//! 对外暴露的 `start` / `end` 一律是**字节偏移**，可直接用于
//! `&text[start..end]` 切片。内部匹配在**字符**上进行，
//! 因此不存在「切在多字节字符中间」的问题。
//!
//! # 三处容易踩坑的约定
//!
//! 1. **空模式**：`Regex::new("")` **合法**，等价于匹配空字符串
//!    （零宽匹配，位于指定位置）。`find_all` 会跳过零宽匹配，因此
//!    `find_all("", text, 0)` 永远返回空列表。
//! 2. **`find_at` 的 `start_byte` 不在字符边界上时**，按**最近的后继字符
//!    边界**（向后取整）处理，绝不会返回 `start < start_byte` 的匹配。
//! 3. **`find_all` 的结果非空且互不重叠**：零宽匹配会被跳过
//!    （但扫描游标照常前进一个字符，因此不会死循环），
//!    且下一个匹配从上一个匹配的 `end` 之后开始。

// ---------------------------------------------------------------- 数据结构

/// 一次匹配的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    /// 匹配起点的**字节偏移**。
    pub start: usize,
    /// 匹配终点的**字节偏移**（不含）。
    pub end: usize,
    /// 匹配到的文本。
    pub text: String,
}

/// 字符类中的一个条目。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassItem {
    /// 单个字符。
    Single(char),
    /// 字符范围（含两端）。
    Range(char, char),
    /// `\d` / `\D`。
    Digit(bool),
    /// `\w` / `\W`。
    Word(bool),
    /// `\s` / `\S`。
    Space(bool),
}

impl ClassItem {
    fn matches(&self, c: char) -> bool {
        match self {
            ClassItem::Single(x) => *x == c,
            ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
            ClassItem::Digit(neg) => c.is_ascii_digit() != *neg,
            ClassItem::Word(neg) => (c.is_alphanumeric() || c == '_') != *neg,
            ClassItem::Space(neg) => c.is_whitespace() != *neg,
        }
    }
}

/// 一个字符类。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CharClass {
    negated: bool,
    items: Vec<ClassItem>,
}

impl CharClass {
    fn matches(&self, c: char) -> bool {
        let hit = self.items.iter().any(|i| i.matches(c));
        hit != self.negated
    }
}

/// 抽象语法树节点。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// 空序列。
    Empty,
    /// 单个字符。
    Char(char),
    /// `.`
    Any,
    /// 字符类（下标指向 `Regex::classes`）。
    Class(usize),
    /// `^`
    Start,
    /// `$`
    End,
    /// 串联。
    Concat(Vec<Node>),
    /// 择一。
    Alt(Vec<Node>),
    /// 重复。
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
    },
}

/// 编译后的指令。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Inst {
    /// 匹配单个字符。
    Char(char),
    /// 匹配任意非换行字符。
    Any,
    /// 匹配字符类。
    Class(usize),
    /// 匹配文本开头。
    Start,
    /// 匹配文本结尾。
    End,
    /// 无条件跳转。
    Jmp(usize),
    /// 普通分支：`Split(优先, 次选)`。
    ///
    /// 优先分支失败时回溯到次选分支（并恢复字符位置）。
    Split(usize, usize),
    /// 循环分支：`LoopSplit(循环编号, 循环体, 循环出口, 是否贪婪)`。
    ///
    /// 贪婪：优先进入循环体，失败才走出口；
    /// 惰性：优先走出口（少匹配几轮），失败才补一轮循环体。
    ///
    /// 与普通 [`Inst::Split`] 的区别：压栈的帧额外记录
    /// 「该循环上一轮的起始位置」与「循环出口地址」，
    /// 回溯时据此恢复循环状态、必要时**跳过已证明为空的循环体**。
    LoopSplit(u32, usize, usize, bool),
    /// 循环入口：`LoopReset(循环编号)`。
    ///
    /// 位于 [`Inst::LoopSplit`] **之前**，只有「从循环外首次进入」才会
    /// 执行到它——循环的回边（[`Inst::Chk`] → [`Inst::LoopSplit`]）直接
    /// 跳到决策点，**不会**经过这里。
    ///
    /// 因此它是「重置该循环的已完成轮数」的唯一位置。这件事必须做对：
    /// 嵌套循环里，外层循环进入下一轮会**重新进入**内层循环，若内层的
    /// 轮数还留着上一轮的值，内层就会误判「最小轮数已满足」而提前退出
    /// （甚至匹配 0 个字符），外层随即判定「本轮无进展」而结束——
    /// 结果是 `(?:X){2,}` 这类模式匹配到比参考引擎短得多的内容。
    LoopReset(u32),
    /// 记录本轮起始位置：`LoopMark(循环编号)`。
    ///
    /// 每个循环有**独立的**起始位置槽位，因此嵌套循环互不干扰。
    LoopMark(u32),
    /// 循环末尾的空进度检查：`Chk(循环编号, 循环头, 循环出口)`。
    ///
    /// 若本轮**没有消费任何字符**（说明重复体可以匹配空串，继续循环只会
    /// 原地打转），则跳到循环出口；否则回到循环头继续下一轮。
    ///
    /// 关键实现细节：
    ///
    /// 1. 位置基准来自该循环**自己的**起始位置槽位（[`Inst::LoopMark`] 写入），
    ///    不是全局共享的栈——共享的话，内层循环弹出标记会把外层的基准弄丢，
    ///    「没有进展」会被误判成「有进展」，循环原地打转。
    /// 2. 检测到「本轮没进展」时，把该位置记进「已证明为空的循环体位置」表。
    ///    惰性循环的回溯会试图**重新进入循环体**，而空循环体再进一次还是空，
    ///    于是状态完全重复 → 死循环。回溯时据此跳过即可（语义等价：
    ///    多匹配几轮空迭代不会改变任何可能的匹配结果）。
    Chk(u32, usize, usize),
    /// 匹配成功。
    Match,
}

/// 「不属于任何循环」的哨兵值（循环编号的上限）。
const NO_LOOP: u32 = u32::MAX;

/// 回溯栈中的一帧。
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// 回溯后继续执行的指令位置。
    pc: usize,
    /// 回溯后恢复的字符位置。
    sp: usize,
    /// 该帧涉及的循环编号；[`NO_LOOP`] 表示不涉及任何循环。
    loop_id: u32,
    /// 该帧入栈时，循环状态回滚日志的长度。
    ///
    /// 回溯弹帧时把日志回滚到这个长度——**所有**循环的轮数与轮起始位置
    /// 都会一并还原。这正是嵌套重复能对齐参考引擎的关键：外层循环的
    /// 轮数也会被内层循环的回溯正确撤销（只恢复「本循环」的状态不够，
    /// 外层的轮数会被内层重放循环体时多算的那一次污染）。
    undo_len: usize,
    /// 该循环是否贪婪（贪婪循环走出口时需要校验最小轮数）。
    loop_greedy: bool,
    /// 该循环的出口地址（跳过「已证明为空」的循环体时直接跳到这里）。
    loop_exit: usize,
}

/// 循环状态的回滚日志条目。
///
/// 任何对循环状态的修改（轮数 +1、轮起始位置入栈、循环入口归零）都先
/// 在这里记下**旧值**；回溯弹帧时按后进先出逆序还原。
///
/// 这与 CPython `sre` 把重复计数压进回溯栈（state stack）的做法一致：
/// 回溯到某个决策点时，整个循环状态快照都要回到那一刻，而不是只回到
/// 「当前循环」的状态。
#[derive(Debug, Clone, Copy)]
struct LoopUndo {
    /// 被修改的循环编号。
    loop_id: u32,
    /// 修改前的已完成轮数。
    count: u32,
    /// 修改前的轮起始位置栈深度。
    marks_len: usize,
}

/// 一个已编译的正则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regex {
    prog: Vec<Inst>,
    classes: Vec<CharClass>,
    /// 循环数量（编译期分配）。
    ///
    /// 运行期为每个循环准备一个独立的标记栈，因此这里要提前知道总数。
    loops: u32,
    /// 每个循环的最小重复次数（与循环编号一一对应）。
    ///
    /// `*` / `?` 为 0，`+` 为 1，`{n,}` 为 n。
    loop_mins: Vec<u32>,
    /// 模式原文（用于错误信息与调试）。
    source: String,
}

/// 单次 `find_at` 的步数预算基数。
///
/// 病态模式（如 `a+a+a+...b`）的匹配树规模是指数级的，没有上限就会
/// 把界面卡死。超限时**当作不匹配**返回——对用户而言「没找到」远好于
/// 「界面无响应」。
const MAX_STEPS_BASE: u64 = 2_000_000;

/// 每字符允许的额外步数。
///
/// 线性扫描类模式（`.*` `\w+` 之类）大约需要 4 步/字符，
/// 因此预算要随文本长度线性增长，否则大文档会被误判成「不匹配」。
const MAX_STEPS_PER_CHAR: u64 = 8;

/// 单次调用的步数预算。
fn step_budget(char_count: usize) -> u64 {
    MAX_STEPS_BASE.saturating_add(MAX_STEPS_PER_CHAR.saturating_mul(char_count as u64))
}

/// 重复次数上限（编译期）。
///
/// `a{1000000}` 会展开成一百万条指令，属于「模式本身过大」，
/// 编译期直接报错比让引擎默默吃掉几十 MB 内存要好。
const MAX_REPEAT_COUNT: u32 = 10_000;

/// `find_all` 的内部命中数上限（防止超大文档上耗时过长 / 内存暴涨）。
///
/// 注意：`limit == 0` 表示「不限数量」，但仍受此硬上限保护。
const MAX_ALL_MATCHES: usize = 100_000;

// ---------------------------------------------------------------- 公开 API

impl Regex {
    /// 编译一个正则模式。
    ///
    /// 失败返回中文错误说明（模式非法或使用了不支持的语法），
    /// 格式为 `正则语法错误：位置 {n} 处{说明}`，其中 `n` 是模式中
    /// 出错字符的**下标**（从 0 开始；等于模式长度表示在末尾出错）。
    pub fn new(pattern: &str) -> Result<Self, String> {
        let mut parser = Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            classes: Vec::new(),
        };
        let ast = parser.parse_alt()?;
        if parser.pos < parser.chars.len() {
            let pos = parser.pos;
            let ch = parser.chars[pos];
            return Err(parser.err(pos, &format!("出现多余的 `{ch}`")));
        }
        let classes = std::mem::take(&mut parser.classes);
        // 单独一个作用域：编译期借用 `classes`，结束后才能把它移进 `Regex`。
        let (prog, loops, loop_mins) = {
            let mut compiler = Compiler {
                prog: Vec::new(),
                loops: 0,
                loop_mins: Vec::new(),
                classes: &classes,
            };
            compiler.emit(&ast);
            compiler.prog.push(Inst::Match);
            let prog = compiler.prog;
            let loops = compiler.loops;
            let loop_mins = compiler.loop_mins;
            (prog, loops, loop_mins)
        };
        Ok(Regex {
            prog,
            classes,
            loops,
            loop_mins,
            source: pattern.to_string(),
        })
    }

    /// 模式原文。
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// 文本中是否存在匹配。
    pub fn is_match(&self, text: &str) -> bool {
        self.find_at(text, 0).is_some()
    }

    /// 从 `start_byte` 开始查找第一个匹配。
    ///
    /// `start_byte` 是**字节偏移**。若它落在多字节字符中间，会**向后
    /// 取整**到最近的后继字符边界（而不是 panic，也不会返回
    /// `start < start_byte` 的匹配）。
    pub fn find_at(&self, text: &str, start_byte: usize) -> Option<RegexMatch> {
        let chars: Vec<char> = text.chars().collect();
        let offsets = byte_offsets(&chars);
        let start_idx = char_index_at(&offsets, start_byte);
        let budget = step_budget(chars.len());
        let mut steps = 0u64;
        let (s, e) = self.find_from(&chars, start_idx, budget, &mut steps)?;
        let start = offsets[s];
        let end = offsets[e];
        Some(RegexMatch {
            start,
            end,
            text: text[start..end].to_string(),
        })
    }

    /// 查找全部匹配。
    ///
    /// 结果**非空且互不重叠**：零宽匹配会被跳过（但扫描游标照常前进
    /// 一个字符，所以不会死循环），下一个匹配从上一个匹配的 `end` 之后开始。
    ///
    /// `limit == 0` 表示**不限数量**（内部仍有 [`MAX_ALL_MATCHES`] 的硬上限）。
    pub fn find_all(&self, text: &str, limit: usize) -> Vec<RegexMatch> {
        let chars: Vec<char> = text.chars().collect();
        let offsets = byte_offsets(&chars);
        let cap = if limit == 0 {
            MAX_ALL_MATCHES
        } else {
            limit.min(MAX_ALL_MATCHES)
        };
        let budget = step_budget(chars.len());
        let mut steps = 0u64;
        let mut out = Vec::new();
        let mut cursor = 0usize;
        while out.len() < cap {
            let Some((s, e)) = self.find_from(&chars, cursor, budget, &mut steps) else {
                break;
            };
            if e > s {
                out.push(RegexMatch {
                    start: offsets[s],
                    end: offsets[e],
                    text: text[offsets[s]..offsets[e]].to_string(),
                });
            }
            // 空匹配不计入结果，但游标必须前进，否则会死循环
            cursor = if e > s { e } else { e + 1 };
            if cursor > chars.len() || steps >= budget {
                break;
            }
        }
        out
    }

    /// 从指定字符下标开始扫描，返回第一个匹配的 `(起点, 终点)`（字符下标）。
    ///
    /// `steps` 是**跨调用共享**的计数器，因此整个 `find_at` / `find_all`
    /// 的总工作量是有界的。
    fn find_from(
        &self,
        chars: &[char],
        start_idx: usize,
        budget: u64,
        steps: &mut u64,
    ) -> Option<(usize, usize)> {
        // 每个循环一个**独立**的标记栈（循环编号 → 已完成轮次的起始位置）。
        //
        // 独立的意义：嵌套循环的回溯互不干扰；共享一个栈的话，
        // 内层循环弹出标记会把外层的基准也弄丢。
        let mut loop_marks: Vec<Vec<usize>> = vec![Vec::new(); self.loops as usize];
        // 每个循环**已完成**的轮数。
        //
        // 与「标记栈长度」分开维护：失败轮次也会留下标记，若拿标记数当轮数，
        // 惰性 `+` 的「至少一轮」会被误判为已满足 → 匹配结果错乱。
        let mut loop_count: Vec<u32> = vec![0; self.loops as usize];
        // 每个循环「已证明匹配空串」的位置集合。
        //
        // 惰性循环的回溯会试图**再进一次循环体**；若循环体在该位置匹配空串，
        // 再进一次结果完全相同 → 状态重复 → 死循环（表现为「明明能匹配却
        // 返回 None」）。记录下来后，回溯时直接跳过该循环体即可。
        //
        // 这是**文本位置**的性质，因此跨起点共享，不必每次清空。
        let mut loop_empty_at: Vec<Vec<usize>> = vec![Vec::new(); self.loops as usize];
        for i in start_idx..=chars.len() {
            if *steps >= budget {
                return None;
            }
            for m in loop_marks.iter_mut() {
                m.clear();
            }
            // 轮数是**本次尝试**的状态，换起点必须归零。
            for c in loop_count.iter_mut() {
                *c = 0;
            }
            if let Some(end) = self.run_at(
                chars,
                i,
                budget,
                steps,
                &mut loop_marks,
                &mut loop_count,
                &mut loop_empty_at,
            ) {
                return Some((i, end));
            }
        }
        None
    }

    /// 从指定字符下标开始匹配，返回匹配终点的字符下标。
    ///
    /// 回溯用**显式栈**实现：栈放在堆上，因此匹配长度再长也不会
    /// 撑爆调用栈（早期版本用递归实现，实测 8KB 以上的文本就会
    /// `STATUS_STACK_OVERFLOW`，直接把整个进程带崩）。
    fn run_at(
        &self,
        chars: &[char],
        start_idx: usize,
        budget: u64,
        steps: &mut u64,
        loop_marks: &mut [Vec<usize>],
        loop_count: &mut [u32],
        loop_empty_at: &mut [Vec<usize>],
    ) -> Option<usize> {
        let mut pc = 0usize;
        let mut sp = start_idx;
        let mut stack: Vec<Frame> = Vec::new();
        // 循环状态回滚日志：每次修改循环状态前先记下旧值，
        // 回溯弹帧时按帧记录的日志长度逆序还原。
        let mut undo: Vec<LoopUndo> = Vec::new();

        loop {
            *steps += 1;
            if *steps > budget {
                return None;
            }

            // 先判断当前指令是否匹配成功；失败则回溯。
            let failed = match &self.prog[pc] {
                Inst::Match => return Some(sp),
                Inst::Char(c) => !(sp < chars.len() && chars[sp] == *c),
                Inst::Any => !(sp < chars.len() && chars[sp] != '\n'),
                Inst::Class(i) => !(sp < chars.len() && self.classes[*i].matches(chars[sp])),
                Inst::Start => sp != 0,
                // 严格语义：`$` 只匹配文本**真正结尾**（与 Rust `regex` crate 一致），
                // 不在末尾换行前匹配。
                Inst::End => sp != chars.len(),
                Inst::Jmp(_)
                | Inst::Split(_, _)
                | Inst::LoopSplit(_, _, _, _)
                | Inst::LoopReset(_)
                | Inst::LoopMark(_)
                | Inst::Chk(_, _, _) => false,
            };

            if failed {
                match stack.pop() {
                    Some(f) => {
                        // 回溯：把循环状态回滚到该帧入栈时的快照。
                        //
                        // 必须回滚**所有**循环，而不只是该帧所属的那个：
                        // 嵌套重复里，内层循环重放上一轮循环体时会把**外层**
                        // 的轮数多算一次；若只恢复内层，外层 `{2,}` 的最小
                        // 次数就会被虚假满足 → 匹配结果比参考引擎短。
                        //
                        // 这与 CPython `sre` 把重复计数压进回溯栈的做法等价。
                        while undo.len() > f.undo_len {
                            let u = undo.pop().expect("回滚日志非空");
                            let id = u.loop_id as usize;
                            loop_count[id] = u.count;
                            loop_marks[id].truncate(u.marks_len);
                        }
                        if f.loop_id != NO_LOOP {
                            let id = f.loop_id as usize;
                            // 贪婪循环走出口 = 放弃剩余轮次；若此时已完成
                            // 轮数不足 `min`，这条出口**不合法**，必须继续
                            // 往更外层回溯（而不是硬生生接受一个轮数不足的匹配）。
                            if f.loop_greedy && loop_count[id] < self.loop_mins[id] {
                                // 保持 `pc` 指向同一条失败指令，继续弹栈。
                                continue;
                            }
                            // 该位置的循环体已被证明匹配空串 → 跳过它，
                            // 直接走循环出口（再进一次结果相同，只会死循环）。
                            if loop_empty_at[id].contains(&f.sp) {
                                pc = f.loop_exit;
                                sp = f.sp;
                                continue;
                            }
                        }
                        pc = f.pc;
                        sp = f.sp;
                    }
                    None => return None,
                }
                continue;
            }

            match &self.prog[pc] {
                Inst::Char(_) | Inst::Any | Inst::Class(_) => {
                    pc += 1;
                    sp += 1;
                }
                Inst::Start | Inst::End => pc += 1,
                Inst::Jmp(x) => pc = *x,
                Inst::Split(x, y) => {
                    // 次选分支压栈：优先分支失败时从这里回溯
                    stack.push(Frame {
                        pc: *y,
                        sp,
                        loop_id: NO_LOOP,
                        undo_len: undo.len(),
                        loop_greedy: false,
                        loop_exit: 0,
                    });
                    pc = *x;
                }
                Inst::LoopSplit(id, body_at, exit_at, greedy) => {
                    // 贪婪：优先进循环体，失败才走出口（放弃剩余轮次）。
                    // 惰性：优先进出口（少匹配几轮），失败才补一轮循环体；
                    //       但**至少**要匹配 `min` 轮，所以轮数不足时仍走循环体。
                    //
                    // `done` 取**已完成轮数**（独立计数器），不能取标记栈长度：
                    // 标记是「进入轮次」的痕迹，失败轮次也会留下痕迹。
                    let done = loop_count[*id as usize];
                    let min = self.loop_mins[*id as usize];
                    let (pri, alt) = if *greedy {
                        (*body_at, *exit_at)
                    } else if done < min {
                        (*body_at, *body_at)
                    } else {
                        (*exit_at, *body_at)
                    };
                    stack.push(Frame {
                        pc: alt,
                        sp,
                        loop_id: *id,
                        undo_len: undo.len(),
                        loop_greedy: *greedy,
                        loop_exit: *exit_at,
                    });
                    pc = pri;
                }
                Inst::LoopReset(id) => {
                    // 循环入口：把该循环的已完成轮数归零。
                    //
                    // 只有「从循环外首次进入」会执行到这里（回边直接跳
                    // `LoopSplit`），因此嵌套循环里外层进入下一轮时，
                    // 内层的轮数会被正确重置。
                    undo.push(LoopUndo {
                        loop_id: *id,
                        count: loop_count[*id as usize],
                        marks_len: loop_marks[*id as usize].len(),
                    });
                    loop_count[*id as usize] = 0;
                    pc += 1;
                }
                Inst::LoopMark(id) => {
                    undo.push(LoopUndo {
                        loop_id: *id,
                        count: loop_count[*id as usize],
                        marks_len: loop_marks[*id as usize].len(),
                    });
                    loop_marks[*id as usize].push(sp);
                    pc += 1;
                }
                Inst::Chk(id, loop_at, exit_at) => {
                    // 本轮是否消费了字符？基准是该循环**自己**的起始位置。
                    let id = *id as usize;
                    let base = loop_marks[id].last().copied();
                    let no_progress = base == Some(sp);
                    if no_progress {
                        // 记下「循环体在此位置匹配空串」，供回溯时跳过。
                        if !loop_empty_at[id].contains(&sp) {
                            loop_empty_at[id].push(sp);
                        }
                    }
                    // 本轮**真正完成**（循环体匹配成功）→ 轮数 +1。
                    //
                    // 这是轮数的**唯一**增长点：失败轮次永远走不到这里，
                    // 因此计数不会被陈旧标记污染。
                    undo.push(LoopUndo {
                        loop_id: id as u32,
                        count: loop_count[id],
                        marks_len: loop_marks[id].len(),
                    });
                    loop_count[id] += 1;
                    pc = if no_progress { *exit_at } else { *loop_at };
                }
                Inst::Match => return Some(sp),
            }
        }
    }
}

// ---------------------------------------------------------------- 辅助

/// 每个字符对应的字节偏移（长度 = 字符数 + 1）。
fn byte_offsets(chars: &[char]) -> Vec<usize> {
    let mut out = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0usize;
    for c in chars {
        out.push(acc);
        acc += c.len_utf8();
    }
    out.push(acc);
    out
}

/// 找出字节偏移 `byte` 对应的字符下标。
///
/// 若 `byte` 恰好落在字符边界上，返回该字符的下标；
/// 若落在某个多字节字符中间，按**最近的后继字符边界**处理
/// （向后取整），因此永远不会匹配到 `byte` 之前的内容。
fn char_index_at(offsets: &[usize], byte: usize) -> usize {
    let last = offsets.len() - 1;
    match offsets.binary_search(&byte) {
        Ok(i) => i.min(last),
        // `Err(i)` 是第一个大于 `byte` 的下标 → 后继字符边界
        Err(i) => i.min(last),
    }
}

// ---------------------------------------------------------------- 解析器

/// 递归下降解析器。
struct Parser {
    chars: Vec<char>,
    pos: usize,
    /// 解析过程中收集到的字符类（AST 里的 `Node::Class` 存其下标）。
    classes: Vec<CharClass>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// 生成一条带位置的中文错误信息。
    fn err(&self, pos: usize, msg: &str) -> String {
        format!("正则语法错误：位置 {pos} 处{msg}")
    }

    /// 登记一个字符类，返回其下标。
    fn add_class(&mut self, class: CharClass) -> usize {
        self.classes.push(class);
        self.classes.len() - 1
    }

    /// 解析择一：`a|b|c`
    fn parse_alt(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().expect("恰有一个分支"))
        } else {
            Ok(Node::Alt(branches))
        }
    }

    /// 解析串联：`ab*c`
    fn parse_concat(&mut self) -> Result<Node, String> {
        let mut items: Vec<Node> = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            items.push(self.parse_quantifier(atom)?);
        }
        match items.len() {
            0 => Ok(Node::Empty),
            1 => Ok(items.pop().expect("恰有一个元素")),
            _ => Ok(Node::Concat(items)),
        }
    }

    /// 解析重复次数后缀。
    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('{') => match self.try_parse_bounds()? {
                Some(b) => b,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        // 非贪婪标记
        let greedy = if self.peek() == Some('?') {
            self.pos += 1;
            false
        } else {
            true
        };
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    /// 尝试解析 `{n}` / `{n,}` / `{n,m}`。
    ///
    /// 不是合法的重复语法时返回 `Ok(None)`，此时 `{` 被当作**字面量**处理
    /// （与主流引擎一致，例如 `a{` 匹配字面的 "a{"）。
    fn try_parse_bounds(&mut self) -> Result<Option<(u32, Option<u32>)>, String> {
        let start = self.pos;
        self.pos += 1; // 吃掉 '{'
        let mut min_text = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                min_text.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if min_text.is_empty() {
            self.pos = start;
            return Ok(None);
        }
        let min: u32 = min_text
            .parse()
            .map_err(|_| self.err(start, &format!("重复次数 `{min_text}` 过大")))?;
        if min > MAX_REPEAT_COUNT {
            return Err(self.err(
                start,
                &format!("重复次数 `{min}` 过大（上限 {MAX_REPEAT_COUNT}）"),
            ));
        }
        match self.peek() {
            Some('}') => {
                self.pos += 1;
                Ok(Some((min, Some(min))))
            }
            Some(',') => {
                self.pos += 1;
                let mut max_text = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        max_text.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if self.peek() != Some('}') {
                    self.pos = start;
                    return Ok(None);
                }
                self.pos += 1;
                let max = if max_text.is_empty() {
                    None
                } else {
                    Some(
                        max_text
                            .parse()
                            .map_err(|_| self.err(start, &format!("重复次数 `{max_text}` 过大")))?,
                    )
                };
                if let Some(m) = max {
                    if m < min {
                        return Err(self.err(
                            start,
                            &format!("重复次数区间非法：`{{{min},{m}}}` 的上界小于下界"),
                        ));
                    }
                    if m > MAX_REPEAT_COUNT {
                        return Err(self.err(
                            start,
                            &format!("重复次数 `{m}` 过大（上限 {MAX_REPEAT_COUNT}）"),
                        ));
                    }
                }
                Ok(Some((min, max)))
            }
            _ => {
                self.pos = start;
                Ok(None)
            }
        }
    }

    /// 解析一个原子。
    fn parse_atom(&mut self) -> Result<Node, String> {
        let Some(c) = self.next() else {
            return Ok(Node::Empty);
        };
        let at = self.pos - 1;
        match c {
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '(' => self.parse_group(),
            '[' => self.parse_class(),
            '\\' => self.parse_escape(),
            '*' | '+' | '?' => Err(self.err(
                at,
                &format!("出现多余的量词 `{c}`（前面没有可重复的内容）"),
            )),
            ')' => Err(self.err(at, "出现多余的 `)`")),
            _ => Ok(Node::Char(c)),
        }
    }

    /// 解析分组：`(...)` 或 `(?:...)`。
    fn parse_group(&mut self) -> Result<Node, String> {
        if self.peek() == Some('?') {
            self.pos += 1;
            let at = self.pos;
            match self.peek() {
                Some(':') => {
                    self.pos += 1;
                }
                Some('=') | Some('!') => {
                    return Err(self.err(at, "不支持前瞻断言（`(?=` / `(?!)`）"));
                }
                Some('<') => {
                    // `(?<=` / `(?<!` 是后顾；`(?<name>` 是命名捕获（也不支持）
                    let after = self.chars.get(self.pos + 1).copied();
                    if after == Some('=') || after == Some('!') {
                        return Err(self.err(at, "不支持后顾断言（`(?<=` / `(?<!`）"));
                    }
                    return Err(self.err(at, "不支持命名捕获组（`(?<name>...`）"));
                }
                Some(c) => {
                    return Err(self.err(at, &format!("不支持分组标志语法 `(?{c}`（内联标志等）")));
                }
                None => return Err(self.err(at, "模式在 `(?` 处意外结束")),
            }
        }
        let inner = self.parse_alt()?;
        if self.next() != Some(')') {
            return Err(self.err(self.pos, "缺少配对的 `)`"));
        }
        Ok(inner)
    }

    /// 解析字符类 `[...]`。
    fn parse_class(&mut self) -> Result<Node, String> {
        let class_at = self.pos - 1;
        let negated = if self.peek() == Some('^') {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut items: Vec<ClassItem> = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err(class_at, "字符类 `[` 缺少配对的 `]`"));
            };
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;
            self.pos += 1;

            let lo = if c == '\\' {
                match self.parse_escape_item()? {
                    ClassItem::Single(x) => x,
                    other => {
                        // `\d` 之类的类别不能作为范围端点
                        items.push(other);
                        continue;
                    }
                }
            } else {
                c
            };

            // 范围：`a-z`
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).copied() != Some(']') {
                let dash_at = self.pos;
                self.pos += 1;
                let Some(rc) = self.next() else {
                    return Err(self.err(dash_at, "字符类在 `-` 处意外结束"));
                };
                let hi = if rc == '\\' {
                    match self.parse_escape_item()? {
                        ClassItem::Single(x) => x,
                        _ => {
                            return Err(self.err(dash_at, "字符类范围的上界不能是 `\\d` 这类别名"));
                        }
                    }
                } else {
                    rc
                };
                if hi < lo {
                    return Err(self.err(dash_at, &format!("字符类范围非法：`{lo}-{hi}`")));
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Single(lo));
            }
        }
        let idx = self.add_class(CharClass { negated, items });
        Ok(Node::Class(idx))
    }

    /// 解析转义序列（`\` 已被消费）。
    ///
    /// 注意：**不要**在这里再取一次字符——`parse_escape_item` 负责消费
    /// 转义符后的那个字符。早期版本在这里多取了一次，
    /// 导致 `\d` 被解析成「`d` 加上后面那个字符」，几乎所有转义都失效。
    fn parse_escape(&mut self) -> Result<Node, String> {
        match self.parse_escape_item()? {
            ClassItem::Single(x) => Ok(Node::Char(x)),
            // 类别别名在类外使用时，转成**等价字符类**：
            // `\d` → `[0-9]`；`\D` → `[^0-9]`。
            ClassItem::Digit(neg) => {
                let idx = self.add_class(CharClass {
                    negated: neg,
                    items: vec![ClassItem::Digit(false)],
                });
                Ok(Node::Class(idx))
            }
            ClassItem::Word(neg) => {
                let idx = self.add_class(CharClass {
                    negated: neg,
                    items: vec![ClassItem::Word(false)],
                });
                Ok(Node::Class(idx))
            }
            ClassItem::Space(neg) => {
                let idx = self.add_class(CharClass {
                    negated: neg,
                    items: vec![ClassItem::Space(false)],
                });
                Ok(Node::Class(idx))
            }
            _ => Err(self.err(self.pos, "出现非法的转义序列")),
        }
    }

    /// 解析转义序列的**内容**（`\` 已被消费）。
    fn parse_escape_item(&mut self) -> Result<ClassItem, String> {
        let escape_at = self.pos - 1;
        let Some(c) = self.next() else {
            return Err(self.err(escape_at, "模式在 `\\` 处意外结束"));
        };
        let item = match c {
            'd' => ClassItem::Digit(false),
            'D' => ClassItem::Digit(true),
            'w' => ClassItem::Word(false),
            'W' => ClassItem::Word(true),
            's' => ClassItem::Space(false),
            'S' => ClassItem::Space(true),
            'n' => ClassItem::Single('\n'),
            'r' => ClassItem::Single('\r'),
            't' => ClassItem::Single('\t'),
            '0' => ClassItem::Single('\0'),
            '1'..='9' => {
                return Err(self.err(
                    escape_at,
                    &format!("不支持反向引用 `\\{c}`（本引擎不实现反向引用）"),
                ));
            }
            // 其余字母型转义（`\b` `\p` `\x` `\u` `\A` …）一律报错，
            // 避免「用户以为词边界生效了、其实匹配的是字面字母」。
            'a'..='z' | 'A'..='Z' => {
                return Err(self.err(
                    escape_at,
                    &format!("不支持的转义序列 `\\{c}`（请显式转义元字符或使用字符类）"),
                ));
            }
            other => ClassItem::Single(other),
        };
        Ok(item)
    }
}

// ---------------------------------------------------------------- 编译器

/// 编译期上下文：指令表 + 循环编号分配器。
struct Compiler<'a> {
    /// 扁平指令表。
    prog: Vec<Inst>,
    /// 已分配的循环编号数量。
    loops: u32,
    /// 每个循环的最小重复次数（与循环编号一一对应）。
    loop_mins: Vec<u32>,
    /// 字符类表（校验类下标是否越界）。
    classes: &'a [CharClass],
}

impl<'a> Compiler<'a> {
    /// 递归发射指令。
    fn emit(&mut self, node: &Node) {
        match node {
            Node::Empty => {}
            Node::Char(c) => self.prog.push(Inst::Char(*c)),
            Node::Any => self.prog.push(Inst::Any),
            Node::Class(idx) => {
                if *idx < self.classes.len() {
                    self.prog.push(Inst::Class(*idx));
                } else {
                    self.prog.push(Inst::Any);
                }
            }
            Node::Start => self.prog.push(Inst::Start),
            Node::End => self.prog.push(Inst::End),
            Node::Concat(items) => {
                for it in items {
                    self.emit(it);
                }
            }
            Node::Alt(branches) => {
                // 结构：Split(B1, B2) B1: ... Jmp(END) B2: Split(B2a, B3) ... END:
                let mut jump_slots: Vec<usize> = Vec::new();
                for (i, branch) in branches.iter().enumerate() {
                    if i + 1 < branches.len() {
                        let split_at = self.prog.len();
                        self.prog.push(Inst::Split(0, 0));
                        let body_at = self.prog.len();
                        self.emit(branch);
                        let jmp_at = self.prog.len();
                        self.prog.push(Inst::Jmp(0));
                        jump_slots.push(jmp_at);
                        // 回填 Split 的优先分支
                        let next_at = self.prog.len();
                        self.prog[split_at] = Inst::Split(body_at, next_at);
                    } else {
                        self.emit(branch);
                    }
                }
                let end_at = self.prog.len();
                for j in jump_slots {
                    self.prog[j] = Inst::Jmp(end_at);
                }
            }
            Node::Repeat {
                node,
                min,
                max,
                greedy,
            } => self.emit_repeat(node, *min, *max, *greedy),
        }
    }

    /// 发射重复指令。
    fn emit_repeat(&mut self, node: &Node, min: u32, max: Option<u32>, greedy: bool) {
        match max {
            // `{n,}` / `*` / `+`：接一个循环。
            //
            // 循环结构：
            //   RESET: LoopReset(id)    ← 循环入口：轮数归零（仅首次进入时执行）
            //   LOOP:  LoopSplit(id, body, exit, greedy)
            //   body:  LoopMark(id)      ← 记录本轮起始字符位置
            //          <重复体>
            //          Chk(id, LOOP, exit) ← 本轮没消费字符 → 跳 exit
            //   exit:
            //
            // **不发射「强制副本」**（`{n,}` 的前 n 次也走循环体）。
            //
            // 原因：强制副本会让「复制体的回溯分支」排在「循环头的回溯
            // 分支」之下，于是回溯顺序与参考引擎相反——嵌套循环里表现为
            // 「外层还没试完内部的可能，就先去试外层的下一轮」，最终找不到
            // 本该找到的匹配（例：`(?:.*)+?[ab]` 对 "bbA_B"，参考引擎给
            // (0,2)，强制副本结构会给不出结果）。
            //
            // 最小次数改由**计数**保证：循环体每完成一轮，`Chk` 就把该循环
            // 的已完成轮数 +1；`min` 记录在 `Regex::loop_mins` 里。
            //
            // `LoopReset` 必须在 `LoopSplit` **之前**：只有「从循环外首次
            // 进入」才会经过它，回边（`Chk` → `LoopSplit`）直接跳决策点。
            // 这样嵌套循环里外层进入下一轮时，内层的轮数会被正确归零。
            None => {
                let id = self.loops;
                self.loops += 1;
                self.loop_mins.push(min);
                let reset_at = self.prog.len();
                self.prog.push(Inst::LoopReset(id));
                let loop_at = self.prog.len();
                self.prog.push(Inst::LoopSplit(id, 0, 0, greedy));
                let body_at = self.prog.len();
                self.prog.push(Inst::LoopMark(id));
                self.emit(node);
                let chk_at = self.prog.len();
                self.prog.push(Inst::Chk(id, 0, 0));
                let exit_at = self.prog.len();
                self.prog[loop_at] = Inst::LoopSplit(id, body_at, exit_at, greedy);
                self.prog[chk_at] = Inst::Chk(id, loop_at, exit_at);
                let _ = reset_at;
            }
            // `{n,m}`：强制 n 次后接 (m-n) 个可选
            //
            // 这里**保留**强制副本：上界已知，不需要用计数来保证最小轮数，
            // 直接展开即可（`{n,m}` 的轮数是有限的，不会出现无界循环）。
            Some(m) => {
                let optional = m.saturating_sub(min);
                // 强制重复的部分
                for _ in 0..min {
                    self.emit(node);
                }
                if optional == 0 {
                    return;
                }
                let mut splits: Vec<usize> = Vec::new();
                let mut jmps: Vec<usize> = Vec::new();
                for _ in 0..optional {
                    let split_at = self.prog.len();
                    self.prog.push(Inst::Split(0, 0));
                    splits.push(split_at);
                    self.emit(node);
                    self.prog.push(Inst::Jmp(0));
                    jmps.push(self.prog.len() - 1);
                }
                let end_at = self.prog.len();
                // 第 i 段：Split 的次选分支 → 第 i+1 段的 Split（末段 → end）；
                // body 之后的 Jmp 同样指向第 i+1 段的 Split。
                for i in 0..splits.len() {
                    let body_at = splits[i] + 1;
                    let next_at = if i + 1 < splits.len() {
                        splits[i + 1]
                    } else {
                        end_at
                    };
                    self.prog[splits[i]] = if greedy {
                        Inst::Split(body_at, next_at)
                    } else {
                        Inst::Split(next_at, body_at)
                    };
                    self.prog[jmps[i]] = Inst::Jmp(next_at);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, text: &str) -> Option<(usize, usize)> {
        let re = Regex::new(pattern).expect("模式应合法");
        re.find_at(text, 0).map(|x| (x.start, x.end))
    }

    fn all(pattern: &str, text: &str) -> Vec<(usize, usize)> {
        let re = Regex::new(pattern).expect("模式应合法");
        re.find_all(text, 0)
            .into_iter()
            .map(|x| (x.start, x.end))
            .collect()
    }

    // ---------------------------------------------------------- 字面量

    /// 临时调试：打印编译出的指令表并逐条追踪执行。
    #[test]
    #[ignore]
    fn debug_trace() {
        let pat = std::env::var("MKV_DEBUG_PAT").unwrap_or_else(|_| r"(\S?)+b{1,3}".to_string());
        let text = std::env::var("MKV_DEBUG_TEXT").unwrap_or_else(|_| "_b ".to_string());
        let re = Regex::new(&pat).expect("模式应合法");
        let chars: Vec<char> = text.chars().collect();
        println!("=== pattern: {pat}   text: {text:?} ({:?}) ===", chars);
        for (i, inst) in re.prog.iter().enumerate() {
            println!("{i:3}: {inst:?}");
        }
        println!("--- trace ---");
        let mut loop_marks: Vec<Vec<usize>> = vec![Vec::new(); re.loops as usize];
        let mut loop_count: Vec<u32> = vec![0; re.loops as usize];
        let mut loop_empty_at: Vec<Vec<usize>> = vec![Vec::new(); re.loops as usize];
        let mut stack: Vec<Frame> = Vec::new();
        let mut undo: Vec<LoopUndo> = Vec::new();
        let mut pc = 0usize;
        let mut sp = 0usize;
        for _step in 0..200 {
            println!(
                "pc={:3} sp={} marks={:?} count={:?} stack={:?}  inst={:?}",
                pc,
                sp,
                loop_marks,
                loop_count,
                stack,
                re.prog[pc]
            );
            let failed = match &re.prog[pc] {
                Inst::Match => {
                    println!("=> MATCH at {sp}");
                    return;
                }
                Inst::Char(c) => !(sp < chars.len() && chars[sp] == *c),
                Inst::Any => !(sp < chars.len() && chars[sp] != '\n'),
                Inst::Class(i) => !(sp < chars.len() && re.classes[*i].matches(chars[sp])),
                Inst::Start => sp != 0,
                Inst::End => sp != chars.len(),
                Inst::Jmp(_)
                | Inst::Split(_, _)
                | Inst::LoopSplit(_, _, _, _)
                | Inst::LoopReset(_)
                | Inst::LoopMark(_)
                | Inst::Chk(_, _, _) => false,
            };
            if failed {
                match stack.pop() {
                    Some(f) => {
                        while undo.len() > f.undo_len {
                            let u = undo.pop().expect("回滚日志非空");
                            let id = u.loop_id as usize;
                            loop_count[id] = u.count;
                            loop_marks[id].truncate(u.marks_len);
                        }
                        if f.loop_id != NO_LOOP {
                            let id = f.loop_id as usize;
                            if f.loop_greedy && loop_count[id] < re.loop_mins[id] {
                                println!("   MIN-NOT-MET -> keep backtracking");
                                continue;
                            }
                            if loop_empty_at[id].contains(&f.sp) {
                                pc = f.loop_exit;
                                sp = f.sp;
                                println!("   SKIP-EMPTY-BODY -> pc={} sp={}", pc, sp);
                                continue;
                            }
                        }
                        pc = f.pc;
                        sp = f.sp;
                        println!("   BACKTRACK -> pc={} sp={}", pc, sp);
                    }
                    None => {
                        println!("   FAIL (stack empty)");
                        return;
                    }
                }
                continue;
            }
            match &re.prog[pc] {
                Inst::Char(_) | Inst::Any | Inst::Class(_) => {
                    pc += 1;
                    sp += 1;
                }
                Inst::Start | Inst::End => pc += 1,
                Inst::Jmp(x) => pc = *x,
                Inst::Split(x, y) => {
                    stack.push(Frame {
                        pc: *y,
                        sp,
                        loop_id: NO_LOOP,
                        undo_len: undo.len(),
                        loop_greedy: false,
                        loop_exit: 0,
                    });
                    pc = *x;
                }
                Inst::LoopSplit(id, body_at, exit_at, greedy) => {
                    let done = loop_count[*id as usize];
                    let min = re.loop_mins[*id as usize];
                    let (pri, alt) = if *greedy {
                        (*body_at, *exit_at)
                    } else if done < min {
                        (*body_at, *body_at)
                    } else {
                        (*exit_at, *body_at)
                    };
                    stack.push(Frame {
                        pc: alt,
                        sp,
                        loop_id: *id,
                        undo_len: undo.len(),
                        loop_greedy: *greedy,
                        loop_exit: *exit_at,
                    });
                    pc = pri;
                }
                Inst::LoopReset(id) => {
                    // 循环入口：把该循环的已完成轮数归零。
                    //
                    // 只有「从循环外首次进入」会执行到这里（回边直接跳
                    // `LoopSplit`），因此嵌套循环里外层进入下一轮时，
                    // 内层的轮数会被正确重置。
                    undo.push(LoopUndo {
                        loop_id: *id,
                        count: loop_count[*id as usize],
                        marks_len: loop_marks[*id as usize].len(),
                    });
                    loop_count[*id as usize] = 0;
                    pc += 1;
                }
                Inst::LoopMark(id) => {
                    undo.push(LoopUndo {
                        loop_id: *id,
                        count: loop_count[*id as usize],
                        marks_len: loop_marks[*id as usize].len(),
                    });
                    loop_marks[*id as usize].push(sp);
                    pc += 1;
                }
                Inst::Chk(id, loop_at, exit_at) => {
                    let id = *id as usize;
                    let base = loop_marks[id].last().copied();
                    let no_progress = base == Some(sp);
                    if no_progress && !loop_empty_at[id].contains(&sp) {
                        loop_empty_at[id].push(sp);
                    }
                    undo.push(LoopUndo {
                        loop_id: id as u32,
                        count: loop_count[id],
                        marks_len: loop_marks[id].len(),
                    });
                    loop_count[id] += 1;
                    println!(
                        "   CHK no_progress={} count={} -> pc={}",
                        no_progress,
                        loop_count[id],
                        if no_progress { *exit_at } else { *loop_at }
                    );
                    pc = if no_progress { *exit_at } else { *loop_at };
                }
                Inst::Match => return,
            }
        }
    }

    #[test]
    fn literal_match() {
        assert_eq!(m("abc", "xxabcxx"), Some((2, 5)));
        assert_eq!(m("abc", "xxxx"), None);
    }

    #[test]
    fn literal_chinese_match() {
        // "xx" 占 2 字节，"模流" 占 6 字节
        assert_eq!(m("模流", "xx模流xx"), Some((2, 8)));
    }

    #[test]
    fn literal_symbols_need_escaping() {
        // `+` 是量词，必须转义才是字面量
        assert_eq!(m(r"a\+b", "a+b"), Some((0, 3)));
        assert_eq!(m("a+b", "ab"), Some((0, 2)));
    }

    #[test]
    fn literal_whole_text_is_found() {
        assert_eq!(m("abc", "abc"), Some((0, 3)));
    }

    // ---------------------------------------------------------- 通配符

    #[test]
    fn dot_matches_any_but_newline() {
        assert_eq!(m("a.c", "abc"), Some((0, 3)));
        assert_eq!(m("a.c", "a\nc"), None);
    }

    #[test]
    fn dot_matches_one_chinese_char() {
        // 「模.流」需要 3 个字符："模字流"
        assert_eq!(m("模.流", "模字流"), Some((0, 9)));
    }

    #[test]
    fn dot_does_not_match_empty() {
        assert_eq!(m("a.c", "ac"), None);
    }

    // ---------------------------------------------------------- 量词

    #[test]
    fn star_is_greedy() {
        assert_eq!(m("a*", "aaab"), Some((0, 3)));
        assert_eq!(m("a*", "b"), Some((0, 0)));
    }

    #[test]
    fn plus_requires_one() {
        assert_eq!(m("a+", "aaab"), Some((0, 3)));
        assert_eq!(m("a+", "b"), None);
    }

    #[test]
    fn question_is_optional() {
        assert_eq!(m("ab?c", "ac"), Some((0, 2)));
        assert_eq!(m("ab?c", "abc"), Some((0, 3)));
    }

    #[test]
    fn bounded_repeat() {
        assert_eq!(m("a{3}", "aaaa"), Some((0, 3)));
        assert_eq!(m("a{2,3}", "aaaa"), Some((0, 3)));
        assert_eq!(m("a{2}", "a"), None);
    }

    #[test]
    fn open_bounded_repeat() {
        assert_eq!(m("a{2,}", "aaaa"), Some((0, 4)));
        assert_eq!(m("a{2,}", "a"), None);
        assert_eq!(m("a{0,}", "aaa"), Some((0, 3)));
    }

    #[test]
    fn exact_repeat_of_zero_is_empty_match() {
        assert_eq!(m("a{0}", "aaa"), Some((0, 0)));
    }

    // ---------------------------------------------------------- 贪婪 vs 非贪婪

    #[test]
    fn greedy_vs_lazy_star() {
        assert_eq!(m("a*", "aaa"), Some((0, 3)));
        assert_eq!(m("a*?", "aaa"), Some((0, 0)));
    }

    #[test]
    fn greedy_vs_lazy_plus() {
        assert_eq!(m("a+", "aaa"), Some((0, 3)));
        assert_eq!(m("a+?", "aaa"), Some((0, 1)));
    }

    #[test]
    fn greedy_vs_lazy_question() {
        assert_eq!(m("a??", "aaa"), Some((0, 0)));
        assert_eq!(m("a?", "aaa"), Some((0, 1)));
    }

    #[test]
    fn greedy_vs_lazy_bounded() {
        assert_eq!(m("a{2,3}", "aaaa"), Some((0, 3)));
        assert_eq!(m("a{2,3}?", "aaaa"), Some((0, 2)));
    }

    #[test]
    fn lazy_bounded_open_repeat() {
        assert_eq!(m("a{2,}?", "aaaa"), Some((0, 2)));
    }

    // ---------------------------------------------------------- 锚点

    #[test]
    fn anchors_limit_position() {
        assert_eq!(m("^abc", "xabc"), None);
        assert_eq!(m("^abc", "abcx"), Some((0, 3)));
        assert_eq!(m("abc$", "xabc"), Some((1, 4)));
        assert_eq!(m("abc$", "abcx"), None);
    }

    #[test]
    fn both_anchors() {
        assert_eq!(m("^abc$", "abc"), Some((0, 3)));
        assert_eq!(m("^abc$", "xabc"), None);
    }

    #[test]
    fn dollar_is_strict_end_of_text() {
        // 严格语义：`$` 只匹配文本真正结尾，不在末尾换行前匹配
        assert_eq!(m("b$", "a\nb"), Some((2, 3)));
        assert_eq!(m("b$", "a\nb\n"), None);
    }

    // ---------------------------------------------------------- 分组与交替

    #[test]
    fn alternation_picks_first_branch() {
        assert_eq!(m("cat|dog", "dog"), Some((0, 3)));
        assert_eq!(m("cat|dog", "cat"), Some((0, 3)));
    }

    #[test]
    fn group_scopes_alternation() {
        assert_eq!(m("(ab)+", "ababab"), Some((0, 6)));
        assert_eq!(m("(ab)+c", "ababc"), Some((0, 5)));
    }

    #[test]
    fn alternation_with_suffix() {
        assert_eq!(m("(foo|bar)baz", "barbaz"), Some((0, 6)));
        assert_eq!(m("(foo|bar)baz", "foobaz"), Some((0, 6)));
        assert_eq!(m("(foo|bar)baz", "quxbaz"), None);
    }

    #[test]
    fn nested_group() {
        assert_eq!(m("((ab)c)+", "abcabc"), Some((0, 6)));
    }

    #[test]
    fn non_capturing_group_is_supported() {
        assert_eq!(m("(?:ab)+", "abab"), Some((0, 4)));
    }

    #[test]
    fn repeated_group_with_bounds() {
        assert_eq!(m("(ab){2,3}", "ababab"), Some((0, 6)));
        assert_eq!(m("(ab){2,3}", "abababab"), Some((0, 6)));
    }

    // ---------------------------------------------------------- 字符类

    #[test]
    fn character_class_matches_and_ranges() {
        assert_eq!(m("[abc]+", "xxabcyy"), Some((2, 5)));
        assert_eq!(m("[a-c]+", "zzbczz"), Some((2, 4)));
        assert_eq!(m("[^a-c]+", "abczz"), Some((3, 5)));
    }

    #[test]
    fn class_range_boundaries_are_inclusive() {
        // 范围含两端
        assert_eq!(m("[a-c]+", "abc"), Some((0, 3)));
        // 紧邻范围之外的字符不匹配
        assert_eq!(m("[a-c]", "d"), None);
        assert_eq!(m("[a-c]", "`"), None);
    }

    #[test]
    fn negated_class_excludes_members() {
        assert_eq!(m("[^a-c]+", "abc"), None);
        assert_eq!(m("[^a-c]", "d"), Some((0, 1)));
    }

    #[test]
    fn class_escapes_work_inside_and_outside_brackets() {
        assert_eq!(m(r"\d+", "abc123"), Some((3, 6)));
        assert_eq!(m(r"[0-9]+", "abc123"), Some((3, 6)));
        assert_eq!(m(r"\w+", "  hello"), Some((2, 7)));
        assert_eq!(m(r"\s+", "ab   cd"), Some((2, 5)));
    }

    #[test]
    fn class_escapes_inside_brackets() {
        assert_eq!(m(r"[\d]+", "ab123"), Some((2, 5)));
        assert_eq!(m(r"[\w]+", "  ab"), Some((2, 4)));
        assert_eq!(m(r"[\s]+", "ab  cd"), Some((2, 4)));
    }

    #[test]
    fn negated_class_escape_is_supported() {
        assert_eq!(m(r"\D+", "12ab"), Some((2, 4)));
    }

    #[test]
    fn negated_class_with_escape_inside_brackets() {
        assert_eq!(m(r"[^\d]+", "12ab"), Some((2, 4)));
        assert_eq!(m(r"[^\d]+", "1234"), None);
    }

    #[test]
    fn predefined_digit_word_space() {
        assert_eq!(m(r"\d", "a1"), Some((1, 2)));
        assert_eq!(m(r"\w", " a"), Some((1, 2)));
        assert_eq!(m(r"\s", "ab "), Some((2, 3)));
    }

    #[test]
    fn predefined_negated_classes() {
        assert_eq!(m(r"\D", "1a"), Some((1, 2)));
        assert_eq!(m(r"\W", "a b"), Some((1, 2)));
        assert_eq!(m(r"\S", "  a"), Some((2, 3)));
    }

    #[test]
    fn class_combined_with_range() {
        assert_eq!(m(r"[a-z0-9_]+", "ab_12"), Some((0, 5)));
    }

    // ---------------------------------------------------------- 转义

    #[test]
    fn escaped_metacharacters_are_literal() {
        assert_eq!(m(r"a\.b", "a.b"), Some((0, 3)));
        assert_eq!(m(r"a\.b", "axb"), None);
        assert_eq!(m(r"a\$b", "a$b"), Some((0, 3)));
    }

    #[test]
    fn all_supported_escapes_are_literal() {
        let cases = [
            (r"a\.b", "a.b"),
            (r"a\*b", "a*b"),
            (r"a\+b", "a+b"),
            (r"a\?b", "a?b"),
            (r"a\(b", "a(b"),
            (r"a\)b", "a)b"),
            (r"a\[b", "a[b"),
            (r"a\]b", "a]b"),
            (r"a\{b", "a{b"),
            (r"a\}b", "a}b"),
            (r"a\|b", "a|b"),
            (r"a\^b", "a^b"),
            (r"a\$b", "a$b"),
            (r"a\\b", "a\\b"),
            (r"a\/b", "a/b"),
            (r"a\-b", "a-b"),
        ];
        for (pat, text) in cases {
            let re = Regex::new(pat).unwrap_or_else(|e| panic!("{pat} 编译失败: {e}"));
            let hit = re.find_at(text, 0);
            assert_eq!(hit.map(|h| h.text), Some(text.to_string()), "模式 {pat}");
        }
    }

    #[test]
    fn control_character_escapes() {
        assert_eq!(m(r"a\nb", "a\nb"), Some((0, 3)));
        assert_eq!(m(r"a\tb", "a\tb"), Some((0, 3)));
    }

    // ---------------------------------------------------------- UTF-8 字节偏移

    #[test]
    fn multibyte_offsets_are_byte_based() {
        let text = "模流分析";
        let re = Regex::new("模流").unwrap();
        let hit = re.find_at(text, 0).expect("应命中");
        assert_eq!(hit.start, 0);
        assert_eq!(hit.end, 6, "两个汉字各 3 字节");
        assert_eq!(hit.text, "模流");
        assert_eq!(&text[hit.start..hit.end], "模流");
    }

    #[test]
    fn multibyte_match_offsets_are_on_char_boundaries() {
        let text = "前面模流与仿真分析后面";
        let re = Regex::new("模流.*分析").unwrap();
        let hit = re.find_at(text, 0).expect("应命中");
        assert!(text.is_char_boundary(hit.start), "start 必须是字符边界");
        assert!(text.is_char_boundary(hit.end), "end 必须是字符边界");
        assert_eq!(&text[hit.start..hit.end], "模流与仿真分析");
    }

    #[test]
    fn multibyte_search_after_chinese_prefix() {
        let text = "模流浇口模流";
        let re = Regex::new("模流").unwrap();
        let hit = re.find_at(text, 0).expect("应命中");
        assert_eq!(hit.start, 0);
        let hit2 = re.find_at(text, hit.end).expect("应命中第二处");
        assert_eq!(hit2.start, 12, "「浇口」6 字节 + 「模流」6 字节");
        assert_eq!(&text[hit2.start..hit2.end], "模流");
    }

    #[test]
    fn multibyte_find_all_offsets_are_slicable() {
        let text = "模流浇口模流";
        let re = Regex::new("模流").unwrap();
        let hits = re.find_all(text, 0);
        assert_eq!(hits.len(), 2);
        for h in &hits {
            assert!(text.is_char_boundary(h.start));
            assert!(text.is_char_boundary(h.end));
            assert_eq!(&text[h.start..h.end], "模流");
        }
    }

    #[test]
    fn start_byte_inside_multibyte_char_degrades_safely() {
        let text = "模流分析";
        let re = Regex::new("流").unwrap();
        // 字节偏移 1 落在「模」字中间 → 向后取整到「流」的起点
        let hit = re.find_at(text, 1).expect("应能安全退化并命中");
        assert_eq!(&text[hit.start..hit.end], "流");
        assert_eq!(hit.start, 3);
    }

    #[test]
    fn start_byte_inside_multibyte_char_never_matches_earlier_text() {
        let text = "模流";
        let re = Regex::new("模").unwrap();
        // 「模」占字节 0..3，从字节 1 开始不应再匹配到它
        assert!(re.find_at(text, 1).is_none(), "不得返回 start < start_byte 的匹配");
    }

    // ---------------------------------------------------------- find_at / find_all

    #[test]
    fn find_at_continues_from_given_offset() {
        let re = Regex::new("ab").unwrap();
        assert_eq!(re.find_at("ab-ab", 0).map(|h| (h.start, h.end)), Some((0, 2)));
        assert_eq!(re.find_at("ab-ab", 2).map(|h| (h.start, h.end)), Some((3, 5)));
        assert_eq!(re.find_at("ab-ab", 4), None);
    }

    #[test]
    fn find_at_beyond_text_returns_none() {
        let re = Regex::new("a").unwrap();
        assert!(re.find_at("abc", 99).is_none());
    }

    #[test]
    fn find_all_collects_all_matches() {
        assert_eq!(all("a+", "aa b aaa"), vec![(0, 2), (5, 8)]);
    }

    #[test]
    fn find_all_matches_do_not_overlap() {
        assert_eq!(all("aa", "aaaa"), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn find_all_skips_empty_matches() {
        let re = Regex::new("a*").unwrap();
        let hits = re.find_all("bbb", 0);
        assert!(hits.is_empty(), "零宽匹配不应计入结果，实际 {:?}", hits);
        for h in &hits {
            assert!(h.start < h.end, "find_all 的匹配必须非空");
        }
    }

    #[test]
    fn empty_match_does_not_loop_forever() {
        let re = Regex::new("a*").unwrap();
        let hits = re.find_all("bbb", 0);
        assert!(hits.len() <= 5, "空匹配不应无限增长，实际 {}", hits.len());
    }

    #[test]
    fn find_all_respects_limit() {
        let re = Regex::new("a").unwrap();
        assert_eq!(re.find_all("aaaa", 2).len(), 2);
    }

    #[test]
    fn find_all_limit_zero_means_unlimited() {
        let re = Regex::new("a").unwrap();
        assert_eq!(re.find_all("aaaa", 0).len(), 4);
    }

    #[test]
    fn is_match_is_consistent_with_find_at() {
        let re = Regex::new("x+y").unwrap();
        assert!(re.is_match("aaxxy"));
        assert!(!re.is_match("aaxz"));
    }

    // ---------------------------------------------------------- 空模式

    #[test]
    fn empty_pattern_is_valid_and_matches_empty_string() {
        // 约定：空模式编译成功，匹配零宽空串
        let re = Regex::new("").expect("空模式应编译成功");
        let hit = re.find_at("abc", 0).expect("应命中");
        assert_eq!(hit.start, 0);
        assert_eq!(hit.end, 0);
        assert_eq!(hit.text, "");
    }

    #[test]
    fn empty_pattern_find_all_yields_nothing() {
        let re = Regex::new("").unwrap();
        assert!(re.find_all("abc", 0).is_empty(), "零宽匹配应被跳过");
    }

    // ---------------------------------------------------------- 语法错误

    #[test]
    fn backreference_is_rejected() {
        let err = Regex::new(r"(a)\1").expect_err("反向引用应报错");
        assert!(err.contains("反向引用"), "实际错误信息：{err}");
    }

    #[test]
    fn lookahead_is_rejected() {
        assert!(Regex::new("(?=a)").is_err());
        assert!(Regex::new("(?!a)").is_err());
        assert!(Regex::new("(?<=a)").is_err());
        assert!(Regex::new("(?<!a)").is_err());
    }

    #[test]
    fn inline_flags_are_rejected() {
        assert!(Regex::new("(?i)abc").is_err());
        assert!(Regex::new("(?m)abc").is_err());
    }

    #[test]
    fn named_group_is_rejected() {
        assert!(Regex::new("(?<name>a)").is_err());
    }

    #[test]
    fn unbalanced_group_is_rejected() {
        assert!(Regex::new("(abc").is_err());
        assert!(Regex::new("abc)").is_err());
    }

    #[test]
    fn dangling_quantifier_is_rejected() {
        assert!(Regex::new("*abc").is_err());
        assert!(Regex::new("+abc").is_err());
        assert!(Regex::new("?abc").is_err());
    }

    #[test]
    fn unterminated_class_is_rejected() {
        assert!(Regex::new("[abc").is_err());
    }

    #[test]
    fn invalid_bound_range_is_rejected() {
        assert!(Regex::new("a{3,1}").is_err());
    }

    #[test]
    fn dangling_escape_is_rejected() {
        assert!(Regex::new("a\\").is_err());
    }

    #[test]
    fn unknown_alnum_escapes_are_rejected() {
        // 静默降级会让用户以为词边界 / Unicode 属性生效了
        assert!(Regex::new(r"\b").is_err());
        assert!(Regex::new(r"\B").is_err());
        assert!(Regex::new(r"\p{L}").is_err());
        assert!(Regex::new(r"\x41").is_err());
    }

    #[test]
    fn error_message_contains_position() {
        for pat in [
            "(abc",
            "[abc",
            r"(a)\1",
            "(?=a)",
            "(?<=a)",
            "(?<n>a)",
            "(?i)x",
            "*abc",
            "abc)",
            "a{3,1}",
            "a\\",
            r"\b",
        ] {
            let err = Regex::new(pat).expect_err("应报错");
            assert!(
                err.contains("位置") && err.contains("正则语法错误"),
                "错误信息应含位置，实际：{err}"
            );
        }
    }

    #[test]
    fn error_message_is_chinese() {
        let err = Regex::new("(abc").unwrap_err();
        assert!(err.chars().any(|c| c as u32 > 0x2E80), "错误信息应为中文：{err}");
    }

    // ---------------------------------------------------------- 健壮性

    #[test]
    fn pathological_pattern_terminates() {
        // 病态模式：指数级回溯。必须被步数上限拦住，而不是卡死。
        let re = Regex::new("a+a+a+a+a+a+a+a+a+a+b").unwrap();
        let text = "a".repeat(64);
        let start = std::time::Instant::now();
        let hit = re.find_at(&text, 0);
        assert!(start.elapsed().as_secs() < 5, "病态模式必须快速返回");
        assert!(hit.is_none(), "不应匹配");
    }

    #[test]
    fn pathological_nested_repeat_terminates() {
        let re = Regex::new("(a+)+b").unwrap();
        let text = "a".repeat(40);
        let start = std::time::Instant::now();
        let hit = re.find_at(&text, 0);
        assert!(start.elapsed().as_secs() < 5, "病态模式必须快速返回");
        assert!(hit.is_none(), "不应匹配");
    }

    #[test]
    fn empty_loop_body_does_not_hang() {
        // `(a*)*` 的重复体可以匹配空串 —— 必须能终止，而不是死循环
        let re = Regex::new("(a*)*").unwrap();
        let start = std::time::Instant::now();
        let hit = re.find_at("aaa", 0);
        assert!(start.elapsed().as_secs() < 5, "空循环体必须快速返回");
        assert_eq!(hit.map(|h| (h.start, h.end)), Some((0, 3)));
    }

    #[test]
    fn nullable_repeat_body_backtracks_like_reference_engine() {
        // 重复体可匹配空串时，回溯必须能退回到更少的轮次
        assert_eq!(m("(\\S?)+b{1,3}", "_b "), Some((0, 2)));
        assert_eq!(m("(\\S?)+b{1,3}", "_bb "), Some((0, 3)));
    }

    #[test]
    fn nested_nullable_repeat_does_not_swallow_required_suffix() {
        // 外层重复体可空 → 不能把后面的必需部分吞掉
        assert_eq!(m("(.?_*?)*?[^\\w]$", "b\n2a "), Some((2, 5)));
    }

    #[test]
    fn nested_unbounded_repeat_keeps_its_min_round_count() {
        // 嵌套 `{n,}` 的**最小轮数约束**不能被内层循环的回溯绕过。
        //
        // 早期实现只在回溯时恢复「当前循环」的轮数：内层循环重放上一轮
        // 循环体时，外层的轮数会被多算一次，`{2,}` 的 min=2 因此被虚假
        // 满足 → 匹配结果比参考引擎（Python `re`）短，甚至凭空多出一段
        // 本不该存在的匹配。
        //
        // 修复：循环状态改用「回滚日志」整体还原（见 [`LoopUndo`]）——
        // 回溯弹帧时把**所有**循环的轮数与轮起始位置一并回滚。
        assert_eq!(m("([abc1]+?){2,}", "1Ca22A "), None);
        assert_eq!(m("(a+){2,}", "a"), None);
        assert_eq!(m("(a+?){2,}", "a"), None);
        assert_eq!(m("(a+?){2,}", "aa"), Some((0, 2)));
        assert_eq!(m("(a*){2,}", "a"), Some((0, 1)));
    }

    #[test]
    fn long_text_does_not_overflow_the_stack() {
        // 早期实现用递归做回溯，8KB 以上的文本就会 STATUS_STACK_OVERFLOW
        // （整个进程被 abort）。现在回溯栈在堆上，长文本必须安全。
        let text = "a".repeat(200_000);
        let re = Regex::new("a*").unwrap();
        let hit = re.find_at(&text, 0).expect("长文本应正常匹配");
        assert_eq!((hit.start, hit.end), (0, 200_000));
    }

    #[test]
    fn long_text_with_dot_does_not_overflow_the_stack() {
        let text = "x".repeat(100_000);
        let re = Regex::new("x.*x").unwrap();
        let hit = re.find_at(&text, 0).expect("长文本应正常匹配");
        assert_eq!((hit.start, hit.end), (0, 100_000));
    }

    #[test]
    fn long_chinese_text_offsets_are_valid() {
        let text = "模".repeat(50_000);
        let re = Regex::new("模").unwrap();
        let hit = re.find_at(&text, 0).expect("应命中");
        assert_eq!(hit.start, 0);
        assert_eq!(hit.end, 3);
        assert!(text.is_char_boundary(hit.end));
    }

    // ---------------------------------------------------------- 杂项

    #[test]
    fn brace_that_is_not_a_bound_is_literal() {
        assert_eq!(m("a{", "a{"), Some((0, 2)));
    }

    #[test]
    fn pattern_text_is_kept() {
        let re = Regex::new("abc").unwrap();
        assert_eq!(re.as_str(), "abc");
    }

    #[test]
    fn chinese_pattern_works_end_to_end() {
        let re = Regex::new("模流.*分析").unwrap();
        let text = "前面模流与仿真分析后面";
        let hit = re.find_at(text, 0).expect("应命中");
        assert_eq!(&text[hit.start..hit.end], "模流与仿真分析");
    }
}

//! 中文分词器：jieba-rs + 自定义领域词典。
//!
//! 实现 tantivy 的 [`Tokenizer`] trait，注册名 `mkv_jieba`（见技术方案设计 §5）。
//!
//! 领域词典约定（决策：中文分词需要自定义领域词典）：
//! - 词典文件支持两种写法，可混用：
//!   - **纯词条**（推荐）：每行一个词，如 `模流分析`；
//!   - **jieba 原生格式**：每行 `词 词频 [词性]`，如 `模流分析 1000 n`。
//! - 以 `#` 开头的行视为注释，空行忽略。
//! - 词典文件置于知识库 `.kbindex/config.toml` 中 `tokenizer.dict` 指定的路径，
//!   或由用户在设置中导入；导入后调用 [`JiebaTokenizer::load_domain_dict`]。

use jieba_rs::{Jieba, TokenizeMode};
use tantivy_tokenizer_api::{Token, TokenStream, Tokenizer};

/// jieba 分词器。
///
/// `Clone` 语义：共享同一份词典（`Arc`），避免为每个索引写入线程复制大词典。
#[derive(Clone)]
pub struct JiebaTokenizer {
    jieba: std::sync::Arc<Jieba>,
}

impl Default for JiebaTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl JiebaTokenizer {
    /// 使用 jieba 内置默认词典构建。
    pub fn new() -> Self {
        Self {
            jieba: std::sync::Arc::new(Jieba::new()),
        }
    }

    /// 载入自定义领域词典。
    ///
    /// 支持两种写法（可混用）：
    /// - 纯词条：每行一个词，词频由 jieba 自动推算；
    /// - jieba 原生格式：`词 词频 [词性]`。
    ///
    /// `#` 开头的行为注释，空行忽略。
    ///
    /// 失败时保留原词典并返回错误，避免因词典问题导致整个索引不可用。
    pub fn load_domain_dict(&mut self, dict_text: &str) -> crate::Result<usize> {
        let entries = parse_domain_dict(dict_text);
        if entries.is_empty() {
            return Ok(0);
        }

        let mut jieba = (*self.jieba).clone();

        // jieba 原生格式走 load_dict（可带词性）；
        let native: String = entries
            .iter()
            .filter_map(|e| e.native.as_ref())
            .map(|s| format!("{s}\n"))
            .collect();
        if !native.is_empty() {
            let mut reader = std::io::BufReader::new(native.as_bytes());
            jieba
                .load_dict(&mut reader)
                .map_err(|e| crate::Error::Config(format!("领域词典加载失败: {e}")))?;
        }

        // 纯词条走 add_word（词频由 jieba 的 suggest_freq 推算）
        for e in entries.iter().filter(|e| e.native.is_none()) {
            jieba.add_word(&e.word, None, None);
        }

        self.jieba = std::sync::Arc::new(jieba);
        Ok(entries.len())
    }

    /// 追加单个自定义词。
    pub fn add_word(&mut self, word: &str, freq: Option<usize>) {
        let mut jieba = (*self.jieba).clone();
        jieba.add_word(word, freq, None);
        self.jieba = std::sync::Arc::new(jieba);
    }

    /// 直接对文本分词，返回词条列表。
    ///
    /// 与写入 tantivy 索引时使用**完全相同**的分词规则
    /// （Search 模式 + 领域词典），因此可直接用于构建预构建索引，
    /// 保证「建索引」与「查查询」两侧切分一致。
    pub fn cut(&self, text: &str) -> Vec<String> {
        self.jieba
            .tokenize(text, TokenizeMode::Search, true)
            .into_iter()
            .map(|t| t.word.to_string())
            .collect()
    }
}

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        // Search 模式：产出更细粒度的词，适合检索召回。
        let tokens = self.jieba.tokenize(text, TokenizeMode::Search, true);
        JiebaTokenStream {
            tokens,
            index: 0,
            current: Token::default(),
        }
    }
}

/// jieba 分词流。
pub struct JiebaTokenStream<'a> {
    tokens: Vec<jieba_rs::Token<'a>>,
    index: usize,
    current: Token,
}

impl TokenStream for JiebaTokenStream<'_> {
    fn advance(&mut self) -> bool {
        if self.index >= self.tokens.len() {
            return false;
        }
        let tok = &self.tokens[self.index];
        self.current.reset();
        self.current.offset_from = tok.byte_start;
        self.current.offset_to = tok.byte_end;
        self.current.position = self.index;
        self.current.text = tok.word.to_string();
        self.current.position_length = 1;
        self.index += 1;
        true
    }

    fn token(&self) -> &Token {
        &self.current
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.current
    }
}

/// 领域词典条目。
struct DictEntry {
    /// 词条文本。
    word: String,
    /// jieba 原生格式行（`词 词频 [词性]`）；纯词条为 `None`。
    native: Option<String>,
}

/// 解析领域词典文本。
///
/// - 空行与 `#` 开头的注释行忽略；
/// - `词 整数 [词性]` 视为 jieba 原生格式；
/// - 其余整行视为一个词条（允许含空格的英文术语）。
fn parse_domain_dict(dict_text: &str) -> Vec<DictEntry> {
    let mut out = Vec::new();
    for raw in dict_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or_default();
        if first.is_empty() {
            continue;
        }
        if let Some(second) = parts.next() {
            if second.parse::<usize>().is_ok() {
                let mut native = format!("{first} {second}");
                if let Some(tag) = parts.next() {
                    native.push(' ');
                    native.push_str(tag);
                }
                out.push(DictEntry {
                    word: first.to_string(),
                    native: Some(native),
                });
                continue;
            }
        }
        out.push(DictEntry {
            word: line.to_string(),
            native: None,
        });
    }
    out
}

/// 分词器注册名，schema 中通过 `set_tokenizer("mkv_jieba")` 引用。
pub const TOKENIZER_NAME: &str = "mkv_jieba";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_chinese_text() {
        let mut tok = JiebaTokenizer::new();
        let mut stream = tok.token_stream("知识库全文检索");
        let mut words = Vec::new();
        while let Some(t) = stream.next() {
            words.push(t.text.clone());
        }
        assert!(words.contains(&"知识库".to_string()) || words.contains(&"全文".to_string()));
        assert!(!words.is_empty());
    }

    #[test]
    fn domain_dict_adds_custom_word() {
        let mut tok = JiebaTokenizer::new();
        // 领域词"Moldflow"应被完整切出
        tok.add_word("Moldflow", Some(1000));
        let mut stream = tok.token_stream("Moldflow 分析报告");
        let words: Vec<String> = std::iter::from_fn(|| stream.next().map(|t| t.text.clone())).collect();
        assert!(words.iter().any(|w| w == "Moldflow"));
    }

    #[test]
    fn loads_domain_dict_text() {
        let mut tok = JiebaTokenizer::new();
        let dict = "注塑模具 1000 n\n";
        let added = tok.load_domain_dict(dict).expect("dict should load");
        assert!(added >= 1);
    }

    /// 回归：领域词典支持「纯词条」写法与注释行。
    ///
    /// jieba 的 load_dict 只接受 `词 词频 [词性]`，遇到注释或纯词条会直接报错。
    #[test]
    fn loads_plain_word_dict_with_comments() {
        let mut tok = JiebaTokenizer::new();
        let dict = "# 注释行\n\n模流分析\n熔接线\nCooling Water\n冷却水路 2000 n\n";
        let added = tok.load_domain_dict(dict).expect("plain dict should load");
        assert_eq!(added, 4);

        let mut stream = tok.token_stream("模流分析报告");
        let words: Vec<String> =
            std::iter::from_fn(|| stream.next().map(|t| t.text.clone())).collect();
        assert!(
            words.iter().any(|w| w == "模流分析"),
            "words = {words:?}"
        );
    }
}


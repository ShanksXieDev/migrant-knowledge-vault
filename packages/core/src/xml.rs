//! 轻量 XML 扫描器：产出带**字节偏移**的节点树。
//!
//! 为什么不用 quick-xml：编辑原生 Office 文件需要「精确改写某一段 XML、
//! 其余字节原样保留」（守"文件优先"，同时不丢样式表 / 母版 / 主题）。
//! quick-xml 0.42 的事件流不暴露字节偏移，无法做外科手术式改写，
//! 因此这里自实现一个只覆盖 OOXML 所需子集的扫描器：
//!
//! - 元素（开始/结束/自闭合）
//! - 属性（单/双引号值）
//! - 文本、CDATA、注释、XML 声明、处理指令
//!
//! 不支持：命名空间前缀解析（OOXML 中前缀稳定，按本地名匹配即可）、
//! DOCTYPE 内部子集、DTD 实体。OOXML 不使用 DTD，故不构成问题。

/// XML 节点。
#[derive(Debug, Clone)]
pub struct XmlNode {
    /// 本地名（不含命名空间前缀）。
    pub name: String,
    /// 属性列表（保持原始顺序，键为**限定名**，如 `r:id`）。
    pub attrs: Vec<(String, String)>,
    /// 子节点。
    pub children: Vec<XmlNode>,
    /// 直接文本内容（已解码实体）。
    pub text: String,
    /// 元素起始偏移（指向 `<`）。
    pub start: usize,
    /// 元素结束偏移（指向 `>` 之后的第一个位置）。
    pub end: usize,
    /// 起始标签结束偏移（`>` 之后），文本内容起点。
    pub inner_start: usize,
    /// 内容结束偏移（闭合标签 `<` 之前）；自闭合时等于 `inner_start`。
    pub inner_end: usize,
    /// 是否为自闭合元素（`<tag/>`）。
    pub self_closing: bool,
}

impl XmlNode {
    /// 取属性值（按**限定名**精确匹配，如 `r:id`）。
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 取属性值（按本地名匹配，忽略命名空间前缀）。
    ///
    /// 注意：`id` 与 `r:id` 的本地名相同，此处会优先匹配无前缀的那个；
    /// 需要精确区分时请改用 [`XmlNode::attr`]。
    pub fn attr_local(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| local_name(k) == name)
            .map(|(_, v)| v.as_str())
    }

    /// 取属性值（空串兜底）。
    pub fn attr_or(&self, name: &str, default: &str) -> String {
        self.attr(name).unwrap_or(default).to_string()
    }

    /// 取属性值（按本地名，空串兜底）。
    pub fn attr_local_or(&self, name: &str, default: &str) -> String {
        self.attr_local(name).unwrap_or(default).to_string()
    }

    /// 按**限定名列表**依次尝试取属性值，取第一个命中的。
    ///
    /// 用于区分同名的不同命名空间属性，例如 `<p:sldId id="256" r:id="rId2"/>`：
    /// - `attr_first(&["r:id"])` → `rId2`
    /// - `attr_first(&["id"])`   → `256`
    pub fn attr_first(&self, names: &[&str]) -> Option<&str> {
        for n in names {
            if let Some(v) = self.attr(n) {
                return Some(v);
            }
        }
        None
    }

    /// 按限定名列表取属性值（空串兜底）。
    pub fn attr_first_or(&self, names: &[&str], default: &str) -> String {
        self.attr_first(names).unwrap_or(default).to_string()
    }

    /// 查找第一个同名直接子节点。
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.name == name)
    }

    /// 查找所有同名直接子节点。
    pub fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// 递归收集所有同名后代。
    pub fn descendants<'a>(&'a self, name: &str) -> Vec<&'a XmlNode> {
        let mut out = Vec::new();
        self.collect_into(name, &mut out);
        out
    }

    fn collect_into<'a>(&'a self, name: &str, out: &mut Vec<&'a XmlNode>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.collect_into(name, out);
        }
    }

    /// 深度优先收集所有后代节点（保持文档顺序）。
    pub fn all_descendants(&self) -> Vec<&XmlNode> {
        let mut out = Vec::new();
        for c in &self.children {
            out.push(c);
            out.extend(c.all_descendants());
        }
        out
    }

    /// 节点的完整文本（递归拼接所有后代文本）。
    pub fn full_text(&self) -> String {
        let mut s = self.text.clone();
        for c in &self.children {
            s.push_str(&c.full_text());
        }
        s
    }
}

/// 解析 XML 文档，返回根节点。
pub fn parse(xml: &str) -> Option<XmlNode> {
    let mut p = Parser {
        s: xml.as_bytes(),
        pos: 0,
    };
    p.parse_document()
}

/// XML 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XML 解析失败（偏移 {}）：{}", self.offset, self.message)
    }
}

impl std::error::Error for XmlError {}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse_document(&mut self) -> Option<XmlNode> {
        loop {
            self.skip_ws();
            if self.pos >= self.s.len() {
                return None;
            }
            if self.s[self.pos] != b'<' {
                // 根节点之前的杂散文本，忽略
                self.pos += 1;
                continue;
            }
            // 跳过声明 / 注释 / 处理指令
            if self.starts_with("<?") {
                self.skip_until("?>", 2);
                continue;
            }
            if self.starts_with("<!--") {
                self.skip_until("-->", 3);
                continue;
            }
            if self.starts_with("<!") {
                // DOCTYPE 等，跳到匹配的 '>'
                self.skip_doctype();
                continue;
            }
            return self.parse_element();
        }
    }

    fn parse_element(&mut self) -> Option<XmlNode> {
        let start = self.pos;
        debug_assert_eq!(self.s[self.pos], b'<');
        self.pos += 1;

        let name = self.read_name();
        if name.is_empty() {
            return None;
        }

        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_ws();
            if self.pos >= self.s.len() {
                return None;
            }
            match self.s[self.pos] {
                b'>' => {
                    self.pos += 1;
                    break;
                }
                b'/' => {
                    if self.pos + 1 < self.s.len() && self.s[self.pos + 1] == b'>' {
                        self_closing = true;
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                _ => {
                    let key = self.read_qname();
                    if key.is_empty() {
                        self.pos += 1;
                        continue;
                    }
                    self.skip_ws();
                    let value = if self.pos < self.s.len() && self.s[self.pos] == b'=' {
                        self.pos += 1;
                        self.skip_ws();
                        self.read_attr_value()
                    } else {
                        String::new()
                    };
                    attrs.push((key, value));
                }
            }
        }

        let inner_start = self.pos;
        let mut children = Vec::new();
        let mut text = String::new();

        if self_closing {
            return Some(XmlNode {
                name,
                attrs,
                children,
                text,
                start,
                end: self.pos,
                inner_start,
                inner_end: inner_start,
                self_closing: true,
            });
        }

        let mut inner_end = inner_start;
        loop {
            if self.pos >= self.s.len() {
                inner_end = self.pos;
                break;
            }
            if self.s[self.pos] == b'<' {
                if self.starts_with("</") {
                    // 结束标签：内容到此为止
                    inner_end = self.pos;
                    // 结束标签
                    self.pos += 2;
                    let _close = self.read_name();
                    self.skip_ws();
                    if self.pos < self.s.len() && self.s[self.pos] == b'>' {
                        self.pos += 1;
                    }
                    break;
                }
                if self.starts_with("<!--") {
                    self.skip_until("-->", 3);
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    self.pos += 9;
                    let start_cd = self.pos;
                    while self.pos < self.s.len() && !self.starts_with("]]>") {
                        self.pos += 1;
                    }
                    let raw = std::str::from_utf8(&self.s[start_cd..self.pos])
                        .unwrap_or_default()
                        .to_string();
                    text.push_str(&raw);
                    if self.starts_with("]]>") {
                        self.pos += 3;
                    }
                    continue;
                }
                if self.starts_with("<?") {
                    self.skip_until("?>", 2);
                    continue;
                }
                match self.parse_element() {
                    Some(child) => children.push(child),
                    None => break,
                }
                continue;
            }
            // 文本
            let t_start = self.pos;
            while self.pos < self.s.len() && self.s[self.pos] != b'<' {
                self.pos += 1;
            }
            let raw = std::str::from_utf8(&self.s[t_start..self.pos]).unwrap_or_default();
            text.push_str(&unescape(raw));
        }

        Some(XmlNode {
            name,
            attrs,
            children,
            text,
            start,
            end: self.pos,
            inner_start,
            inner_end,
            self_closing: false,
        })
    }

    fn read_name(&mut self) -> String {
        let raw = self.read_qname();
        local_name(&raw).to_string()
    }

    /// 读取名称，**保留命名空间前缀**（如 `r:id`）。
    fn read_qname(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b':' || c == b'.' {
                self.pos += 1;
            } else if c >= 0x80 {
                // UTF-8 续字节/多字节字符，视为名称的一部分
                self.pos += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[start..self.pos])
            .unwrap_or_default()
            .to_string()
    }

    fn read_attr_value(&mut self) -> String {
        if self.pos >= self.s.len() {
            return String::new();
        }
        let quote = self.s[self.pos];
        if quote != b'"' && quote != b'\'' {
            // 无引号值：读到空白或 '>'
            let start = self.pos;
            while self.pos < self.s.len()
                && !self.s[self.pos].is_ascii_whitespace()
                && self.s[self.pos] != b'>'
            {
                self.pos += 1;
            }
            return unescape(std::str::from_utf8(&self.s[start..self.pos]).unwrap_or_default());
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.s.len() && self.s[self.pos] != quote {
            self.pos += 1;
        }
        let raw = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or_default();
        if self.pos < self.s.len() {
            self.pos += 1;
        }
        unescape(raw)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.s[self.pos..].starts_with(pat.as_bytes())
    }

    fn skip_until(&mut self, pat: &str, _min_len: usize) {
        while self.pos < self.s.len() {
            if self.starts_with(pat) {
                self.pos += pat.len();
                return;
            }
            self.pos += 1;
        }
    }

    fn skip_doctype(&mut self) {
        self.pos += 2;
        let mut depth = 0i32;
        while self.pos < self.s.len() {
            match self.s[self.pos] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                b'>' if depth <= 0 => {
                    self.pos += 1;
                    return;
                }
                _ => {}
            }
            self.pos += 1;
        }
    }
}

/// XML 转义（写回文件时使用）。
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// XML 实体解码。
pub fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            let ch_len = utf8_len(bytes[i]);
            out.push_str(&text[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        // 找 ';'
        let semi = text[i..].find(';').map(|p| i + p);
        let Some(semi) = semi else {
            out.push('&');
            i += 1;
            continue;
        };
        let entity = &text[i + 1..semi];
        let replacement = match entity {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" => Some("'".to_string()),
            _ => {
                if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32).map(|c| c.to_string())
                } else {
                    None
                }
            }
        };
        match replacement {
            Some(r) => {
                out.push_str(&r);
                i = semi + 1;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// 取限定名的本地部分（去掉命名空间前缀）。
fn local_name(qname: &str) -> &str {
    match qname.rsplit_once(':') {
        Some((_, local)) => local,
        None => qname,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_elements_with_attributes() {
        let xml = r#"<?xml version="1.0"?><root a="1"><child b="x">hello</child><child>world</child></root>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.name, "root");
        assert_eq!(root.attr("a"), Some("1"));
        let children: Vec<&XmlNode> = root.children_named("child").collect();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].text, "hello");
        assert_eq!(children[0].attr("b"), Some("x"));
        assert_eq!(children[1].text, "world");
    }

    #[test]
    fn handles_self_closing_and_cdata() {
        let xml = "<root><a/><b><![CDATA[<raw>]]></b></root>";
        let root = parse(xml).unwrap();
        assert!(root.child("a").is_some());
        assert_eq!(root.child("b").unwrap().text, "<raw>");
    }

    #[test]
    fn tracks_byte_offsets() {
        let xml = "<root><a>xx</a></root>";
        let root = parse(xml).unwrap();
        let a = root.child("a").unwrap();
        assert_eq!(&xml[a.start..a.end], "<a>xx</a>");
    }

    #[test]
    fn escapes_and_unescapes() {
        assert_eq!(escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(unescape("a&lt;b&gt;&amp;"), "a<b>&");
        assert_eq!(unescape("&#65;&#x42;"), "AB");
    }

    #[test]
    fn parses_namespaced_names_by_local_name() {
        let xml = r#"<w:document xmlns:w="urn:w"><w:body><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.name, "document");
        let t = root.descendants("t");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].text, "hi");
    }
}


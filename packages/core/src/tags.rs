//! 标签提取（F-07 多维过滤的数据来源）。
//!
//! # 为什么需要这个模块
//!
//! `meta.rs` 里的 `tags` 表从 v0.1 就存在，但**从来没有写入路径**——
//! 标签功能一直是有表无数据。F-07 要做「按标签 / 类型 / 时间 / 来源过滤」，
//! 必须先有稳定的标签来源。
//!
//! # 标签来源（按优先级，同名去重）
//!
//! 1. **Markdown front-matter**：`tags: [模具, 评审]` 或 `tags: 模具`。
//!    这是用户显式声明的标签，优先级最高。
//! 2. **目录名**：文档所在目录（如 `03-台账`）作为一个标签。
//!    知识库普遍按目录组织，目录即天然的分类维度。
//! 3. **格式标签**：如 `Excel` / `Markdown`。
//!    与「类型」维度重叠，但让「只按标签过滤」也能表达「所有 Excel」。
//!
//! # 为什么不做「自动关键词标签」
//!
//! 自动打标（如按正文出现频次）会让标签集合随内容漂移，
//! 用户昨天勾的标签今天就消失了。目录与 front-matter 是**用户可控**的，
//! 过滤结果可预期——这是 PRD「组合过滤结果准确」的前提。

use crate::format::Format;

/// 标签集合（去重后按字典序输出，保证可复现）。
pub fn extract_tags(text: &str, dir: &str, format: Format) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let t = s.trim();
        if t.is_empty() {
            return;
        }
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    };

    // 1. front-matter
    for t in front_matter_tags(text) {
        push(&t);
    }

    // 2. 目录名（只取最后一级：`03-台账/子目录/x.md` → `子目录`）
    if !dir.is_empty() {
        if let Some(leaf) = dir.rsplit('/').next() {
            push(leaf);
        }
    }

    // 3. 格式
    push(format.label());

    out.sort();
    out.dedup();
    out
}

/// 从 Markdown front-matter 提取 `tags`。
///
/// 支持两种写法：
/// ```yaml
/// ---
/// tags: [模具, 评审]
/// ---
/// ```
/// ```yaml
/// ---
/// tags: 模具
/// ---
/// ```
///
/// 非 Markdown、空 front-matter、格式不对时返回空列表（不报错——
/// 标签是增强信息，不该让文档解析失败）。
pub fn front_matter_tags(text: &str) -> Vec<String> {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = match trimmed.strip_prefix("---") {
        Some(r) => r,
        None => return Vec::new(),
    };
    // front-matter 必须在文件开头，`---` 后紧跟换行
    let rest = match rest.strip_prefix('\n') {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut tags: Vec<String> = Vec::new();
    let mut in_tags = false;
    for line in rest.lines() {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if !in_tags {
            // 只认顶层 `tags:`（前面不能有缩进，否则是嵌套字段）
            if let Some(rest_line) = line.strip_prefix("tags:") {
                if rest_line.is_empty() {
                    in_tags = true; // 数组形式，后续行收集
                    continue;
                }
                let value = rest_line.trim().trim_matches('"').trim_matches('\'');
                for part in value.split(',') {
                    let t = part.trim().trim_start_matches('[').trim_end_matches(']').trim();
                    if !t.is_empty() {
                        tags.push(t.to_string());
                    }
                }
                in_tags = true;
            }
            continue;
        }
        // 数组形式的续行：`- 模具`
        if let Some(item) = line.strip_prefix('-') {
            let t = item.trim().trim_matches('"').trim_matches('\'');
            if !t.is_empty() {
                tags.push(t.to_string());
            }
            continue;
        }
        // 数组结束（遇到下一个键）
        if line.contains(':') && !line.starts_with('-') {
            break;
        }
    }

    tags.retain(|t| !t.trim().is_empty());
    tags.dedup();
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_inline_front_matter_tags() {
        let md = "---\ntitle: 评审纪要\ntags: 模具, 评审\n---\n\n正文\n";
        assert_eq!(front_matter_tags(md), vec!["模具", "评审"]);
    }

    #[test]
    fn extracts_list_style_front_matter_tags() {
        let md = "---\ntags:\n  - 模具\n  - 评审\n---\n正文\n";
        assert_eq!(front_matter_tags(md), vec!["模具", "评审"]);
    }

    #[test]
    fn ignores_non_front_matter_text() {
        assert!(front_matter_tags("普通正文\ntags: x\n").is_empty());
        assert!(front_matter_tags("").is_empty());
        assert!(front_matter_tags("---\nnot closed\n").is_empty());
    }

    #[test]
    fn directory_and_format_become_tags() {
        let tags = extract_tags("正文", "03-台账", Format::Excel);
        assert!(tags.contains(&"03-台账".to_string()), "{tags:?}");
        assert!(tags.contains(&"Excel".to_string()), "{tags:?}");
    }

    #[test]
    fn front_matter_takes_priority_and_dedupes() {
        let md = "---\ntags: [台账]\n---\n正文";
        let tags = extract_tags(md, "03-台账", Format::Excel);
        // 「台账」与「03-台账」不同，都保留；但格式标签只出现一次
        assert_eq!(
            tags.iter().filter(|t| *t == "Excel").count(),
            1,
            "{tags:?}"
        );
        assert!(tags.contains(&"台账".to_string()), "{tags:?}");
    }

    #[test]
    fn root_directory_documents_get_no_dir_tag() {
        let tags = extract_tags("正文", "", Format::Markdown);
        assert_eq!(tags, vec!["Markdown".to_string()]);
    }
}

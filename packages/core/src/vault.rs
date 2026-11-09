//! 知识库门面：把存储、索引、元数据、Git、编辑器编排成统一 API。
//!
//! 见技术方案设计 §6 核心数据流：
//! 扫描 → 解析 → 写索引 → 检索 / 双链 / 编辑 / 版本。

use crate::error::{Error, Result};
use crate::format::Format;
use crate::git::{CommitInfo, GitRepo};
use crate::index::{IndexDocument, SearchIndex, TantivyIndex};
use crate::meta::MetaStore;
use crate::types::{DocumentRecord, LinkRecord, SearchHit};
use crate::parse::{self, ExtractedLink};
use crate::scan::{ScanResult, ScannedDoc, scan};
use crate::storage::{FsStorage, Storage};
use std::path::{Path, PathBuf};

/// 索引构建进度。
#[derive(Debug, Clone, Default)]
pub struct IndexProgress {
    pub total: usize,
    pub done: usize,
    pub current: String,
}

/// 索引构建结果。
#[derive(Debug, Clone)]
pub struct IndexReport {
    pub scanned: usize,
    pub indexed: usize,
    pub skipped: usize,
    pub links_found: usize,
    pub elapsed_ms: u128,
}

/// 知识库。
pub struct Vault {
    storage: Box<dyn Storage>,
    meta: MetaStore,
    index: TantivyIndex,
    git: Option<GitRepo>,
    root: Option<PathBuf>,
    /// 索引目录独占锁（决策：文件锁）。持有至 Vault 析构。
    _lock: FileLock,
}

impl Vault {
    /// 打开（并按需创建）一个知识库。
    ///
    /// - `init_git` 为 true 时初始化 Git 仓库并写入 `.gitignore`。
    /// - 索引目录固定为 `<root>/.kbindex/`。
    pub fn open(root: impl AsRef<Path>, init_git: bool) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;

        let kbindex = root.join(crate::KBINDEX_DIR);
        std::fs::create_dir_all(&kbindex).map_err(|e| Error::io(kbindex.clone(), e))?;

        // 索引目录锁：防止多端同时写入损坏索引（决策：文件锁）
        let lock = acquire_lock(&kbindex)?;

        let storage = Box::new(FsStorage::new(&root));
        let meta = MetaStore::open(&kbindex.join("meta.db"))?;
        let index = TantivyIndex::open(&kbindex)?;

        let git = if init_git {
            let repo = GitRepo::init_or_open(&root)?;
            repo.ensure_gitignore()?;
            Some(repo)
        } else {
            GitRepo::open(&root).ok()
        };

        Ok(Self {
            storage,
            meta,
            index,
            git,
            root: Some(root),
            _lock: lock,
        })
    }

    /// 知识库根目录。
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// 索引目录路径。
    pub fn kbindex_dir(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join(crate::KBINDEX_DIR))
    }

    /// 扫描知识库。
    pub fn scan(&self) -> Result<ScanResult> {
        scan(self.storage.as_ref())
    }

    /// 全量重建索引与元数据。
    ///
    /// 全量重建 = 「我确认已看到当前内容」，因此**顺带刷新引用源片段快照基线**
    /// （F-06 进阶②）：之后的增量修改（文件监听 / 编辑保存）不刷新基线，
    /// 于是源文件被改动时能被识别为变更。
    ///
    /// 若调用方只想重建索引、**不想动**变更检测基线（例如导出 Web 产物），
    /// 请改用 [`Self::reindex_keeping_ref_snapshots`]。
    pub fn reindex(
        &mut self,
        mut progress: Option<&mut dyn FnMut(IndexProgress)>,
    ) -> Result<IndexReport> {
        let report = self.reindex_impl(&mut progress)?;
        self.refresh_reference_snapshots()?;
        Ok(report)
    }

    /// 全量重建索引与元数据，但**保留**引用源片段快照基线。
    ///
    /// # 为什么需要这个变体
    ///
    /// 「刷新基线」与「重建索引」是两个**独立语义**，此前被捆在一起，
    /// 于是产生了一个顺带副作用：**导出一份 Web 产物会把 Desktop 侧的
    /// 变更检测基线清零**——跑一次 `mkv export-web`，Desktop 就再也看不出
    /// 「这段引用相对上次改过」，而用户没有任何提示。
    ///
    /// Web 导出只是想把当前内容打包成产物，不该动 Desktop 的基线。
    pub fn reindex_keeping_ref_snapshots(
        &mut self,
        mut progress: Option<&mut dyn FnMut(IndexProgress)>,
    ) -> Result<IndexReport> {
        self.reindex_impl(&mut progress)
    }

    /// 重建索引与元数据的共同实现（不含快照基线处理）。
    fn reindex_impl(
        &mut self,
        progress: &mut Option<&mut dyn FnMut(IndexProgress)>,
    ) -> Result<IndexReport> {
        let started = std::time::Instant::now();
        let scan_result = self.scan()?;

        // 全量重建必须先清空全文索引：否则磁盘上已删除 / 改名的文档
        // 会留下幽灵条目（`upsert` 只按 path 覆盖自己），检索仍命中旧路径。
        // 放在 scan 成功之后，避免扫描失败时把索引清成空库。
        self.index.clear()?;
        self.index.commit()?;

        // 记录文档表与链接
        let mut records: Vec<DocumentRecord> = Vec::new();
        // 每个文档解析出的原始链接（第二阶段再解析目标路径）
        let mut pending: Vec<(String, Vec<ExtractedLink>, String)> = Vec::new();
        let mut link_count = 0usize;

        let total = scan_result.docs.len();
        for (i, doc) in scan_result.docs.iter().enumerate() {
            if let Some(cb) = progress.as_mut() {
                cb(IndexProgress {
                    total,
                    done: i,
                    current: doc.path.clone(),
                });
            }

            let parsed = parse::parse_document(self.storage.as_ref(), &doc.path, doc.format)?;
            let content_hash = hash_text(&parsed.text);

            records.push(DocumentRecord {
                path: doc.path.clone(),
                title: doc.title().to_string(),
                format: format!("{:?}", doc.format),
                size: doc.size,
                modified_at: doc.modified_at,
                content_hash,
            });

            // F-07：标签写入索引与元数据库（tags 表此前有表无数据）。
            let tags = crate::tags::extract_tags(&parsed.text, &doc.dir, doc.format);

            // 写入全文索引
            self.index.upsert(&IndexDocument {
                path: doc.path.clone(),
                title: doc.title().to_string(),
                content: parsed.text.clone(),
                format: format!("{:?}", doc.format),
                modified_at: doc.modified_at,
                tags: tags.clone(),
            })?;
            self.meta.set_tags(&doc.path, &tags)?;

            link_count += parsed.links.len();
            pending.push((
                doc.path.clone(),
                parsed.links,
                doc.format.link_granularity().to_string(),
            ));
        }

        self.index.commit()?;
        self.meta.replace_documents(&records)?;

        // 第二阶段：解析链接目标并写入 links 表。
        // 必须在 documents 表落库之后进行 —— 链接目标要靠文档名/标题反查，
        // 否则 to_path 解析为空，双链无从建立。
        for (from_path, links, granularity) in &pending {
            let rows: Vec<LinkRecord> = links
                .iter()
                .map(|link| LinkRecord {
                    from_path: from_path.clone(),
                    to_path: self.resolve_link_target(&link.target, &records),
                    target: link.target.clone(),
                    granularity: granularity.clone(),
                })
                .collect();
            self.meta.replace_links(from_path, &rows)?;
        }

        self.meta.set_state("schema_version", &crate::SCHEMA_VERSION.to_string())?;
        self.meta.set_state("last_indexed_at", &crate::meta::now_secs().to_string())?;
        // 记录本次索引所依据的词典版本，便于判断索引是否过期
        self.meta
            .set_state("domain_dict_hash", &self.index.domain_dict_hash())?;

        // 注：引用源片段快照基线的刷新**不在这里**。
        // 它与「重建索引」是两个独立语义，由 `reindex` / `reindex_keeping_ref_snapshots`
        // 两个入口分别决定——否则「导出一份 Web 产物」会顺手把 Desktop 侧的
        // 变更检测基线清零（详见后者文档）。

        if let Some(cb) = progress.as_mut() {
            cb(IndexProgress {
                total,
                done: total,
                current: String::new(),
            });
        }

        Ok(IndexReport {
            scanned: scan_result.docs.len(),
            indexed: records.len(),
            skipped: scan_result.skipped,
            links_found: link_count,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    /// 增量更新单个文档（编辑保存后调用）。
    pub fn update_document(&mut self, rel_path: &str) -> Result<()> {
        let Some(format) = Format::from_path(Path::new(rel_path)) else {
            return Err(Error::UnsupportedFormat(rel_path.to_string()));
        };

        let parsed = parse::parse_document(self.storage.as_ref(), rel_path, format)?;
        let title = Path::new(rel_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| rel_path.to_string());
        let dir = match rel_path.rfind('/') {
            Some(idx) => rel_path[..idx].to_string(),
            None => String::new(),
        };

        // F-07：增量更新同样要刷新标签，否则标签与正文会漂移。
        let tags = crate::tags::extract_tags(&parsed.text, &dir, format);

        self.index.upsert(&IndexDocument {
            path: rel_path.to_string(),
            title: title.clone(),
            content: parsed.text.clone(),
            format: format!("{format:?}"),
            modified_at: self.storage.modified_at(rel_path)?.unwrap_or(0),
            tags: tags.clone(),
        })?;
        self.index.commit()?;
        self.meta.set_tags(rel_path, &tags)?;

        let size = self.storage.file_size(rel_path)?.unwrap_or(0);
        let modified_at = self.storage.modified_at(rel_path)?.unwrap_or(0);
        self.meta.upsert_document(&DocumentRecord {
            path: rel_path.to_string(),
            title,
            format: format!("{format:?}"),
            size,
            modified_at,
            content_hash: hash_text(&parsed.text),
        })?;

        let mut links: Vec<LinkRecord> = Vec::new();
        let known = self.meta.list_documents()?;
        for link in &parsed.links {
            links.push(LinkRecord {
                from_path: rel_path.to_string(),
                to_path: self.resolve_link_target(&link.target, &known),
                target: link.target.clone(),
                granularity: format.link_granularity().to_string(),
            });
        }
        self.meta.replace_links(rel_path, &links)?;
        Ok(())
    }

    /// 打开文档进行内置编辑（GridCraft / DeckCraft / WordCraft）。
    pub fn open_editor(&self, rel_path: &str) -> Result<crate::editor::EditSession> {
        let Some(format) = Format::from_path(Path::new(rel_path)) else {
            return Err(Error::UnsupportedFormat(rel_path.to_string()));
        };
        let router = crate::editor::EditorRouter::new();
        match router.route(
            self.storage.as_ref(),
            rel_path,
            format,
            crate::editor::EditMode::Builtin,
        )? {
            crate::editor::EditorTarget::Builtin { session, .. } => Ok(session),
            crate::editor::EditorTarget::External { .. } => {
                Err(Error::Config(format!("{} 无内置编辑器", format.label())))
            }
        }
    }

    /// 构造 F-08 外部唤起目标（纯构造，不启动进程，便于单测）。
    ///
    /// 三重校验：格式在六种受支持格式内、路径不逃逸出知识库根目录、文件真实存在。
    pub fn external_open_target(&self, rel_path: &str) -> Result<crate::editor::EditorTarget> {
        if Format::from_path(Path::new(rel_path)).is_none() {
            return Err(Error::UnsupportedFormat(rel_path.to_string()));
        }
        if rel_path.replace('\\', "/").split('/').any(|s| s == "..") {
            return Err(Error::Config(format!(
                "非法路径（试图逃逸出知识库根目录）: {rel_path}"
            )));
        }
        let abs = self
            .storage
            .absolute_path(rel_path)
            .ok_or_else(|| Error::Config(format!("该后端不支持本地路径: {rel_path}")))?;
        if !abs.exists() {
            return Err(Error::NotFound(abs));
        }
        Ok(crate::editor::EditorTarget::External {
            program: String::new(), // 空 = 系统默认应用
            path: abs,
        })
    }

    /// 用系统默认应用打开原始文档（F-08）。
    ///
    /// 只负责唤起，不等待进程退出；用户保存后由 P0 文件监听增量更新索引与双链。
    pub fn open_externally(&self, rel_path: &str) -> Result<()> {
        let target = self.external_open_target(rel_path)?;
        crate::editor::EditorRouter::launch_external(&target)
    }

    /// 在原生 Office 应用内定位到指定片段（F-06 进阶③）。
    ///
    /// # `locator` 是什么
    ///
    /// 可以是**片段标签**（如「台账 · 第 2 行」）或**引用选择器**（如「第 2 行」）。
    /// 内部**先按标签推导**——Excel 的标签带工作表名，能拿到完整锚点
    /// （`ms-excel:...#台账!A2`）；标签推不出位置时再回退到按选择器推导。
    ///
    /// # 返回
    ///
    /// 返回**唤起方案**而非单纯的成功标记：方案里带
    /// 「能否定位到片段级」与「为什么这样降级」，
    /// UI 据此如实告知用户（Word / Visio / Drawio 没有公开的应用内定位协议，
    /// 只能打开文件——这是硬结论，不假装能定位）。
    pub fn open_in_office(
        &self,
        rel_path: &str,
        locator: &str,
    ) -> Result<crate::office_locator::OfficeLaunchPlan> {
        let Some(platform) = crate::office_locator::current_platform() else {
            return Err(Error::Config(
                "当前平台没有可用的原生应用唤起能力".into(),
            ));
        };
        let Some(abs) = self.storage.absolute_path(rel_path) else {
            return Err(Error::NotFound(rel_path.to_string().into()));
        };
        let format = Format::from_path(Path::new(rel_path)).unwrap_or(Format::Markdown);
        let fragment = match crate::office_locator::fragment_from_label(format, locator) {
            // 标签推不出具体位置（空标签 / 非数字）→ 回退到选择器推导。
            crate::office_locator::Fragment::Document => {
                crate::office_locator::fragment_from_selector(format, locator)
            }
            other => other,
        };
        let plan =
            crate::office_locator::plan_office_launch(format, rel_path, &abs, &fragment, platform);
        crate::editor::EditorRouter::launch_office_plan(&plan)?;
        Ok(plan)
    }

    /// 保存内置编辑结果：写回原生文件 → 增量重建该文档的索引与双链。
    ///
    /// 返回是否成功（写盘成功但索引更新失败时返回错误并说明阶段）。
    pub fn save_editor(&mut self, session: &crate::editor::EditSession) -> Result<()> {
        // 1) 写回原生文件（守"文件优先"：始终读写原生格式）
        session.save(self.storage.as_ref())?;

        // 2) 增量重建该文档的索引与双链（PRD 一致性要求：1s 内更新）
        self.update_document(&session.rel_path)?;
        Ok(())
    }

    /// 读取文档**抽取后的正文**（与全文索引所用内容一致）。
    ///
    /// 与 [`Vault::read_document`]（原始字节）的区别：
    /// 本方法对二进制格式（Excel / Word / PPT）做文本抽取，
    /// 因此预构建索引的倒排表与运行时索引内容一致。
    pub fn read_extracted_text(&self, rel_path: &str) -> Result<String> {
        let Some(format) = Format::from_path(Path::new(rel_path)) else {
            return Err(Error::UnsupportedFormat(rel_path.to_string()));
        };
        parse::extract_text(self.storage.as_ref(), rel_path, format)
            .map_err(|e| Error::Parse {
                path: PathBuf::from(rel_path),
                message: e.to_string(),
            })
    }

    /// 从索引中删除文档。
    pub fn remove_document(&mut self, rel_path: &str) -> Result<()> {
        self.index.delete(rel_path)?;
        self.index.commit()?;
        self.meta.delete_document(rel_path)?;
        Ok(())
    }

    /// 重新解析全库文档的链接目标，修复双链悬空。
    ///
    /// `update_document` 只更新**该文档自己的出链**；但以下场景会影响别人的链接：
    /// - A.md 写了 `[[台账]]`，当时台账.xlsx 还不存在 → `to_path` 为空；
    ///   后来新建台账.xlsx，必须重算 A.md 的出链才能建立反链；
    /// - 文档改名 / 删除后，原先指向它的 `to_path` 变成悬空路径。
    ///
    /// 因此文件监听在「新建 / 删除 / 改名」批次结束后调用本方法；
    /// 纯内容修改走 `update_document` 即可，不必付这份开销。
    /// 返回重算后的链接总数。
    pub fn relink_all(&mut self) -> Result<usize> {
        let docs = self.meta.list_documents()?;
        let mut total = 0usize;

        for doc in &docs {
            let Some(format) = Format::from_path(Path::new(&doc.path)) else {
                continue;
            };
            let parsed = match parse::parse_document(self.storage.as_ref(), &doc.path, format) {
                Ok(p) => p,
                Err(_) => {
                    // 文档已从磁盘消失但元数据尚未清理：清空其出链，
                    // 避免悬空链接污染反链面板。
                    self.meta.replace_links(&doc.path, &[])?;
                    continue;
                }
            };

            let rows: Vec<LinkRecord> = parsed
                .links
                .iter()
                .map(|link| LinkRecord {
                    from_path: doc.path.clone(),
                    to_path: self.resolve_link_target(&link.target, &docs),
                    target: link.target.clone(),
                    granularity: format.link_granularity().to_string(),
                })
                .collect();
            total += rows.len();
            self.meta.replace_links(&doc.path, &rows)?;
        }
        Ok(total)
    }

    /// 全文检索。
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.index.search(query, limit)
    }

    /// 按查询对象检索（F-12 正则模式 + F-07 多维过滤）。
    ///
    /// 正则模式在**原文**上匹配：Tantivy 的分词索引无法承载「跨词的正则」，
    /// 因此正则检索走「候选集 + 原文精筛」两段式——
    /// 先用 Tantivy 取一批候选（保证相关度排序），再用正则逐条在原文中匹配。
    /// 候选集取 `limit * 4`（下限 50）以覆盖正则筛掉的部分。
    pub fn search_query(&self, query: &crate::query::SearchQuery) -> Result<Vec<SearchHit>> {
        if query.is_blank() {
            return Ok(Vec::new());
        }
        let hits = match query.mode {
            crate::query::QueryMode::Terms => self.index.search(&query.text, query.limit)?,
            crate::query::QueryMode::Regex => {
                let re = crate::regex::Regex::new(&query.text)
                    .map_err(|e| Error::Config(format!("正则表达式无效: {e}")))?;
                let candidate_limit = query.limit.saturating_mul(4).max(50);
                let candidates = self.index.search(&query.text, candidate_limit)?;
                let mut out = Vec::new();
                for mut hit in candidates {
                    let Some(text) = self.read_document(&hit.path).ok() else {
                        continue;
                    };
                    let Some(m) = re.find_at(&text, 0) else {
                        continue;
                    };
                    // 命中位置与上下文写回摘要，便于 UI 直接显示
                    hit.snippet = regex_snippet(&text, m.start, m.end);
                    out.push(hit);
                    if out.len() >= query.limit {
                        break;
                    }
                }
                out
            }
        };
        Ok(self.apply_query_filter(hits, &query.filter))
    }

    /// 在结果上应用过滤器（F-07）。
    pub fn apply_query_filter(
        &self,
        hits: Vec<SearchHit>,
        filter: &crate::query::QueryFilter,
    ) -> Vec<SearchHit> {
        if filter.is_empty() {
            return hits;
        }
        hits.into_iter()
            .filter(|h| filter.matches(h, h.modified_at, |p| self.tags_of(p)))
            .collect()
    }

    /// 某文档的标签（F-07）。
    pub fn tags_of(&self, rel_path: &str) -> Vec<String> {
        self.meta.get_tags(rel_path).unwrap_or_default()
    }

    /// 全部标签及文档数（F-07 过滤面板的候选项）。
    pub fn all_tags(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for doc in self.meta.list_documents().unwrap_or_default() {
            for tag in self.meta.get_tags(&doc.path).unwrap_or_default() {
                *counts.entry(tag).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// 文档反链（谁引用了我）。
    pub fn backlinks(&self, rel_path: &str) -> Result<Vec<LinkRecord>> {
        self.meta.backlinks(rel_path)
    }

    /// 文档出链（我引用了谁）。
    pub fn outgoing_links(&self, rel_path: &str) -> Result<Vec<LinkRecord>> {
        self.meta.outgoing_links(rel_path)
    }

    /// 读取文档原始内容。
    pub fn read_document(&self, rel_path: &str) -> Result<String> {
        let bytes = self.storage.read(rel_path)?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// 只读访问底层存储。
    ///
    /// 供需要读**原始字节**的模块使用（流程图节点抽取要读 `.drawio` 的 XML
    /// 与 `.vsdx` 的 zip 包，纯文本抽取拿不到这些信息）。
    ///
    /// 暴露成 `&dyn Storage` 而不是具体类型，是为了不把「本地文件系统」
    /// 这一实现细节泄漏到调用方。
    pub fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }

    /// 跨格式引用视图（F-06），带流程图节点定位（F-11）与源变更差异（F-06 进阶）。
    ///
    /// 每次调用都重新读取源文件，因此源更新后卡片内容会随之刷新。
    pub fn references(&self, rel_path: &str) -> Result<Vec<crate::refs::ReferenceView>> {
        let Ok(source) = self.read_ref_text(rel_path) else {
            return Ok(Vec::new());
        };
        // 没有引用语法的文档不必扫全库（引用解析要读全库正文与节点清单，代价不低）。
        if crate::refs::parse_references(&source).is_empty() {
            return Ok(Vec::new());
        }
        let (all, node_map) = self.reference_sources()?;
        let snapshots = self.meta.list_ref_snapshots().unwrap_or_default();
        Ok(crate::refs::build_reference_views_with_snapshots(
            &source,
            &all,
            &node_map,
            &snapshots,
        ))
    }

    /// 读取**用于引用解析**的文本。
    ///
    /// # 为什么必须是抽取文本而不是原始字节
    ///
    /// 原始字节对 `.xlsx` / `.pptx` / `.docx` 是 **ZIP 二进制**，对 `.vsdx` 同理。
    /// 直接拿它做片段切分，切出来的是二进制乱码——引用卡片显示一堆乱码，
    /// 差异高亮更是拿二进制在比二进制（毫无意义）。
    ///
    /// 2026-11-09 评审实测（修复前）：Desktop 端对 `模具台账.xlsx` 的引用卡片
    /// snippet 是 `i�\u{10}��...`，对 `项目方案.pptx` 是 `PK\x03\x04...`；
    /// 而 Web 端（构建期用抽取文本）显示的是 `台账: PN-1024 | MDL-08`。
    /// 同一功能在两端给出完全不同的内容，Desktop 侧还叠加了
    /// 「基线存二进制 → 与全文永远不等 → 永远报『源已变更』」的假差异。
    ///
    /// 因此：**Desktop 与 Web 必须用同一份文本基准**，否则两端结论会互相矛盾。
    ///
    /// 抽取失败（格式不支持 / 解析出错）时退回原文——纯文本格式仍可用，
    /// 总比整条功能不可用好。
    fn read_ref_text(&self, rel_path: &str) -> Result<String> {
        match self.read_extracted_text(rel_path) {
            Ok(text) => Ok(text),
            Err(_) => self.read_document(rel_path),
        }
    }

    /// 汇总全库正文与流程图节点清单（引用解析的公共输入）。
    fn reference_sources(
        &self,
    ) -> Result<(
        Vec<(String, String, Format)>,
        std::collections::HashMap<String, Vec<crate::diagram::DiagramNode>>,
    )> {
        let mut all: Vec<(String, String, Format)> = Vec::new();
        let mut node_targets: Vec<(String, Format)> = Vec::new();
        for d in self.documents()? {
            let Ok(text) = self.read_ref_text(&d.path) else {
                continue;
            };
            let format = Format::from_path(Path::new(&d.path)).unwrap_or(Format::Markdown);
            node_targets.push((d.path.clone(), format));
            all.push((d.path.clone(), text, format));
        }
        // 节点清单必须读**原始字节**（.drawio 的 XML / .vsdx 的 zip），
        // 抽取文本拿不到这些信息——这一路刻意不走 `read_ref_text`。
        let node_map = crate::diagram::build_node_map(self.storage.as_ref(), &node_targets);
        Ok((all, node_map))
    }

    /// 重建引用源片段快照基线。
    ///
    /// 只在**全量重建**时调用：此刻的源片段内容即成为「上次看到的内容」。
    /// 增量更新不刷新基线，源文件因此被改动时能被识别为变更。
    pub fn refresh_reference_snapshots(&mut self) -> Result<usize> {
        let (all, node_map) = self.reference_sources()?;
        self.meta.clear_ref_snapshots()?;
        let mut count = 0usize;
        for (_path, text, _format) in &all {
            let sources = crate::refs::resolve_reference_sources(text, &all, &node_map);
            for (target, selector, current) in sources {
                let key = crate::refs::snapshot_key(&target, &selector);
                self.meta.set_ref_snapshot(&key, &current)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// 列出全部文档记录。
    pub fn documents(&self) -> Result<Vec<DocumentRecord>> {
        self.meta.list_documents()
    }

    /// 文件是否仍存在于磁盘（文件监听用于把改名事件的旧路径翻转为删除）。
    pub fn file_exists(&self, rel_path: &str) -> bool {
        self.storage.exists(rel_path)
    }

    /// 索引中文本数量。
    pub fn indexed_count(&self) -> Result<u64> {
        self.index.doc_count()
    }

    /// 提交一次 Git 版本。
    pub fn commit(&self, message: &str, author: &str) -> Result<CommitInfo> {
        let git = self.git.as_ref().ok_or_else(|| Error::Config("未启用 Git".into()))?;
        git.commit(message, author)
    }

    /// Git 提交历史。
    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let git = self.git.as_ref().ok_or_else(|| Error::Config("未启用 Git".into()))?;
        git.log(limit)
    }

    /// 回滚到指定提交。
    pub fn rollback(&self, commit_id: &str) -> Result<()> {
        let git = self.git.as_ref().ok_or_else(|| Error::Config("未启用 Git".into()))?;
        git.checkout_commit(commit_id)?;
        Ok(())
    }

    /// 两个提交之间的差异摘要。
    pub fn diff(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let git = self.git.as_ref().ok_or_else(|| Error::Config("未启用 Git".into()))?;
        git.diff_summary(from, to)
    }

    /// 载入自定义领域词典并重建索引。
    ///
    /// 词典会持久化到 `.kbindex/dict.txt`，保证下次打开索引时
    /// 索引与查询使用同一套分词规则（否则检索结果会失真）。
    pub fn apply_domain_dict(&mut self, dict_text: &str) -> Result<usize> {
        let added = self.index.apply_domain_dict(dict_text)?;
        // 词典变化需重建索引才能生效
        self.reindex(None)?;
        // 记录本次索引所依据的词典版本
        self.meta
            .set_state("domain_dict_hash", &self.index.domain_dict_hash())?;
        Ok(added)
    }

    /// 索引是否已过期。
    ///
    /// 领域词典在索引构建后被修改（手工编辑 `dict.txt`）时，
    /// 现有索引的分词规则与词典不一致，需要重建。
    pub fn is_index_stale(&self) -> bool {
        let recorded = self.meta.get_state("domain_dict_hash").ok().flatten();
        match recorded {
            Some(h) => h != self.index.domain_dict_hash(),
            None => !self.index.domain_dict_text().is_empty(),
        }
    }

    /// 解析链接目标为实际文档路径。
    ///
    /// 匹配顺序（先精确后宽松）：
    /// 1. 命中库内实际路径；
    /// 2. 文件名完全一致（含扩展名，如 `ledger.xlsx`）；
    /// 3. 去掉扩展名后主干一致（如 `[[台账]]` → `台账.xlsx`）；
    /// 4. 文档标题一致。
    ///
    /// `docs` 为已知的文档清单，解析时以它为准（重建索引时传入本轮扫描结果）。
    fn resolve_link_target(&self, target: &str, docs: &[DocumentRecord]) -> String {
        let target = target.trim();
        if target.is_empty() {
            return String::new();
        }
        // 直接命中原路径
        if self.storage.exists(target) {
            return target.to_string();
        }
        if docs.iter().any(|d| d.path == target) {
            return target.to_string();
        }

        // 按文件名匹配（忽略目录）
        let file_name = target.rsplit('/').next().unwrap_or(target);
        let file_name = file_name.rsplit('\\').next().unwrap_or(file_name);
        if file_name.is_empty() {
            return String::new();
        }
        for d in docs {
            if file_name_of(&d.path) == file_name {
                return d.path.clone();
            }
        }

        // 去掉扩展名后按主干匹配（[[台账]] → 台账.xlsx）
        let stem = strip_extension(file_name);
        if !stem.is_empty() {
            for d in docs {
                if strip_extension(&file_name_of(&d.path)) == stem {
                    return d.path.clone();
                }
            }
        }

        // 按标题匹配
        for d in docs {
            if d.title == target || d.title == file_name {
                return d.path.clone();
            }
        }

        String::new()
    }

}

/// 取路径中的文件名部分。
fn file_name_of(path: &str) -> &str {
    let p = path.rsplit('/').next().unwrap_or(path);
    p.rsplit('\\').next().unwrap_or(p)
}

/// 去掉扩展名（无扩展名时原样返回）。
fn strip_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && !stem.is_empty() => stem,
        _ => name,
    }
}

/// 索引目录文件锁（决策：文件锁）。
struct FileLock {
    _file: std::fs::File,
}

/// 获取 `.kbindex/.lock` 独占锁。
fn acquire_lock(kbindex: &Path) -> Result<FileLock> {
    use fs2::FileExt;

    let lock_path = kbindex.join(".lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| Error::io(lock_path.clone(), e))?;

    file.try_lock_exclusive().map_err(|_| Error::Locked(lock_path.clone()))?;
    Ok(FileLock { _file: file })
}

/// 计算文本哈希（用于变更检测）。
fn hash_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 便捷函数：从抽取到的链接列表构造未解析的 LinkRecord。
pub fn unresolved_links(from_path: &str, links: &[ExtractedLink], granularity: &str) -> Vec<LinkRecord> {
    links
        .iter()
        .map(|l| LinkRecord {
            from_path: from_path.to_string(),
            to_path: String::new(),
            target: l.target.clone(),
            granularity: granularity.to_string(),
        })
        .collect()
}

/// 生成正则命中处的上下文摘要。
///
/// 命中前后各取一定字符数，换行压成空格，保证 UI 单行显示不串行。
fn regex_snippet(text: &str, start: usize, end: usize) -> String {
    crate::query::regex_snippet(text, start, end)
}

/// 扫描结果到索引文档的辅助转换。
pub fn scanned_to_index_document(doc: &ScannedDoc, content: String) -> IndexDocument {
    IndexDocument {
        path: doc.path.clone(),
        title: doc.title().to_string(),
        content,
        format: format!("{:?}", doc.format),
        modified_at: doc.modified_at,
        tags: crate::tags::extract_tags("", &doc.dir, doc.format),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mkv_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opens_creates_kbindex_and_git() {
        let dir = temp_vault("open");
        let vault = Vault::open(&dir, true).unwrap();
        assert!(dir.join(".kbindex").exists());
        assert!(dir.join(".kbindex/meta.db").exists());
        assert!(dir.join(".git").exists());
        assert!(dir.join(".gitignore").exists());
        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn indexes_and_finds_documents() {
        let dir = temp_vault("index");
        fs::write(dir.join("a.md"), "# 模具分析\n\n这是关于 Moldflow 模流分析的笔记。").unwrap();
        fs::write(dir.join("b.md"), "# 其他\n\n无关内容。").unwrap();

        let mut vault = Vault::open(&dir, true).unwrap();
        let report = vault.reindex(None).unwrap();
        assert_eq!(report.indexed, 2);
        assert_eq!(vault.indexed_count().unwrap(), 2);

        let hits = vault.search("模流分析", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "a.md");

        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_and_resolves_backlinks() {
        let dir = temp_vault("links");
        fs::write(dir.join("target.md"), "# 目标").unwrap();
        fs::write(dir.join("source.md"), "# 源\n\n见 [[target]]。").unwrap();

        let mut vault = Vault::open(&dir, false).unwrap();
        vault.reindex(None).unwrap();

        let backlinks = vault.backlinks("target.md").unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].from_path, "source.md");

        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_commit_and_log() {
        let dir = temp_vault("git");
        fs::write(dir.join("a.md"), "# hello").unwrap();

        let vault = Vault::open(&dir, true).unwrap();
        let c = vault.commit("init", "tester").unwrap();
        assert!(!c.id.is_empty());
        let logs = vault.log(10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "init");

        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：unborn branch（仓库尚无任何提交）必须能完成首次提交，
    /// 且提交后 HEAD 可解析、`git log` 能读到历史。
    ///
    /// 旧实现用 `repo.commit(None, …)`，只创建 commit 对象不更新引用，
    /// HEAD 仍指向不存在的 refs/heads/master，首次提交后 log 直接报错。
    #[test]
    fn git_commit_incrementally_appends_log() {
        let dir = temp_vault("git2");
        fs::write(dir.join("a.md"), "# hello").unwrap();

        let vault = Vault::open(&dir, true).unwrap();
        vault.commit("init", "tester").unwrap();
        fs::write(dir.join("b.md"), "# world").unwrap();
        vault.commit("second", "tester").unwrap();

        let logs = vault.log(10).unwrap();
        assert_eq!(logs.len(), 2, "logs = {logs:?}");
        assert_eq!(logs[0].message, "second");
        assert_eq!(logs[1].message, "init");

        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：领域词典必须持久化，并在重开索引时自动载入。
    ///
    /// 旧实现把词典只放在内存中的分词器实例里，新进程打开索引时退回默认分词器，
    /// 查询与文档切词不一致，tantivy 短语查询直接零命中。
    #[test]
    fn domain_dict_survives_reopen() {
        let dir = temp_vault("dict");
        fs::write(dir.join("a.md"), "# 模流分析-入门\n\n模流分析入门 浇口位置。").unwrap();

        let mut vault = Vault::open(&dir, false).unwrap();
        vault.reindex(None).unwrap();
        vault.apply_domain_dict("模流分析\n浇口\n").unwrap();
        assert!(!vault.search("模流分析", 10).unwrap().is_empty());
        assert!(
            dir.join(".kbindex").join("dict.txt").exists(),
            "领域词典应持久化到 .kbindex/dict.txt"
        );
        drop(vault);

        // 重新打开（模拟新进程）
        let vault2 = Vault::open(&dir, false).unwrap();
        let hits = vault2.search("模流分析", 10).unwrap();
        assert!(!hits.is_empty(), "重开索引后应能检索到文档");
        assert_eq!(hits[0].path, "a.md");
        assert!(!vault2.is_index_stale(), "刚重建的索引不应标记为过期");

        drop(vault2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：手工修改领域词典后，索引应被标记为过期。
    #[test]
    fn detects_stale_index_after_manual_dict_change() {
        let dir = temp_vault("stale");
        fs::write(dir.join("a.md"), "# 模流分析").unwrap();

        let mut vault = Vault::open(&dir, false).unwrap();
        vault.reindex(None).unwrap();
        vault.apply_domain_dict("模流分析\n").unwrap();
        assert!(!vault.is_index_stale());
        drop(vault);

        fs::write(dir.join(".kbindex").join("dict.txt"), "模流分析\n浇口\n").unwrap();
        let vault2 = Vault::open(&dir, false).unwrap();
        assert!(vault2.is_index_stale(), "词典变更后索引应标记为过期");

        drop(vault2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：链接目标解析必须发生在 documents 表落库之后。
    ///
    /// 旧实现边扫描边解析，而解析依赖 `meta.list_documents()`，
    /// 此时表内还是空数据 → to_path 为空 → 反链为 0。
    #[test]
    fn resolves_links_by_title_and_file_name() {
        let dir = temp_vault("resolve");
        fs::write(dir.join("台账.md"), "# 台账\n\n模具台账记录。").unwrap();
        fs::write(dir.join("来源.md"), "# 来源\n\n见 [[台账]] 与 @台账.md。").unwrap();

        let mut vault = Vault::open(&dir, false).unwrap();
        vault.reindex(None).unwrap();

        let outgoing = vault.outgoing_links("来源.md").unwrap();
        assert_eq!(outgoing.len(), 2, "outgoing = {outgoing:?}");
        assert!(
            outgoing.iter().all(|l| l.to_path == "台账.md"),
            "outgoing = {outgoing:?}"
        );

        let backlinks = vault.backlinks("台账.md").unwrap();
        assert_eq!(backlinks.len(), 2, "backlinks = {backlinks:?}");
        assert_eq!(backlinks[0].from_path, "来源.md");

        drop(vault);
        let _ = fs::remove_dir_all(&dir);
    }
}


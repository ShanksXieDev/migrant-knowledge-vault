//! SQLite 元数据存储：文档元信息、标签、双链边、索引状态。
//!
//! 见技术方案设计 §3：`.kbindex/meta.db`。

use crate::error::Result;
use crate::types::{DocumentRecord, LinkRecord};
use rusqlite::{Connection, params};
use std::path::Path;

/// 元数据库。
pub struct MetaStore {
    conn: Connection,
}

impl MetaStore {
    /// 打开（并按需初始化）元数据库。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::Error::io(parent.to_path_buf(), e))?;
        }
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// 内存库（测试用）。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS documents (
                path          TEXT PRIMARY KEY,
                title         TEXT NOT NULL,
                format        TEXT NOT NULL,
                size          INTEGER NOT NULL DEFAULT 0,
                modified_at   INTEGER NOT NULL DEFAULT 0,
                content_hash  TEXT NOT NULL DEFAULT '',
                indexed_at    INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS links (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                from_path   TEXT NOT NULL,
                to_path     TEXT NOT NULL DEFAULT '',
                target      TEXT NOT NULL,
                granularity TEXT NOT NULL DEFAULT 'block'
            );

            CREATE INDEX IF NOT EXISTS idx_links_from ON links(from_path);
            CREATE INDEX IF NOT EXISTS idx_links_to   ON links(to_path);

            CREATE TABLE IF NOT EXISTS tags (
                path TEXT NOT NULL,
                tag  TEXT NOT NULL,
                PRIMARY KEY (path, tag)
            );

            CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);

            CREATE TABLE IF NOT EXISTS state (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ref_snapshots (
                key  TEXT PRIMARY KEY,
                text TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    /// 全量替换文档记录（重建时使用）。
    pub fn replace_documents(&mut self, docs: &[DocumentRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM documents", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO documents (path, title, format, size, modified_at, content_hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let now = now_secs() as i64;
            for d in docs {
                stmt.execute(params![
                    d.path,
                    d.title,
                    d.format.clone(),
                    d.size as i64,
                    d.modified_at as i64,
                    d.content_hash.clone(),
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 写入/更新单个文档。
    pub fn upsert_document(&mut self, doc: &DocumentRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO documents (path, title, format, size, modified_at, content_hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
                title=excluded.title, format=excluded.format, size=excluded.size,
                modified_at=excluded.modified_at, content_hash=excluded.content_hash,
                indexed_at=excluded.indexed_at",
            params![
                doc.path.clone(),
                doc.title.clone(),
                doc.format.clone(),
                doc.size as i64,
                doc.modified_at as i64,
                doc.content_hash.clone(),
                now_secs() as i64
            ],
        )?;
        Ok(())
    }

    /// 删除文档及其出链。
    pub fn delete_document(&mut self, path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM documents WHERE path = ?1", params![path])?;
        self.conn
            .execute("DELETE FROM links WHERE from_path = ?1", params![path])?;
        Ok(())
    }

    /// 列出全部文档。
    pub fn list_documents(&self) -> Result<Vec<DocumentRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, title, format, size, modified_at, content_hash FROM documents ORDER BY path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DocumentRecord {
                path: row.get(0)?,
                title: row.get(1)?,
                format: row.get(2)?,
                size: row.get::<_, i64>(3)? as u64,
                modified_at: row.get::<_, i64>(4)? as u64,
                content_hash: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 查询文档。
    pub fn get_document(&self, path: &str) -> Result<Option<DocumentRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, title, format, size, modified_at, content_hash FROM documents WHERE path = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |row| {
            Ok(DocumentRecord {
                path: row.get(0)?,
                title: row.get(1)?,
                format: row.get(2)?,
                size: row.get::<_, i64>(3)? as u64,
                modified_at: row.get::<_, i64>(4)? as u64,
                content_hash: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// 全量替换某个文档的出链。
    pub fn replace_links(&mut self, from_path: &str, links: &[LinkRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM links WHERE from_path = ?1", params![from_path])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO links (from_path, to_path, target, granularity) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for l in links {
                stmt.execute(params![
                    l.from_path,
                    l.to_path,
                    l.target,
                    l.granularity
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 某文档的出链。
    pub fn outgoing_links(&self, path: &str) -> Result<Vec<LinkRecord>> {
        self.query_links(
            "SELECT from_path, to_path, target, granularity FROM links WHERE from_path = ?1",
            params![path],
        )
    }

    /// 某文档的反链（谁引用了我）。
    pub fn backlinks(&self, path: &str) -> Result<Vec<LinkRecord>> {
        self.query_links(
            "SELECT from_path, to_path, target, granularity FROM links WHERE to_path = ?1",
            params![path],
        )
    }

    fn query_links(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<LinkRecord>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(p, |row| {
            Ok(LinkRecord {
                from_path: row.get(0)?,
                to_path: row.get(1)?,
                target: row.get(2)?,
                granularity: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 设置标签。
    pub fn set_tags(&mut self, path: &str, tags: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM tags WHERE path = ?1", params![path])?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO tags (path, tag) VALUES (?1, ?2)")?;
            for t in tags {
                stmt.execute(params![path, t])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 读取标签。
    pub fn get_tags(&self, path: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM tags WHERE path = ?1 ORDER BY tag")?;
        let rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 写入状态键值（如 schema_version）。
    pub fn set_state(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 读取状态键值。
    pub fn get_state(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM state WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// 写入引用源片段快照（F-06 进阶：源变更差异高亮）。
    ///
    /// 快照记录的是「上次看到的源片段原文」，
    /// 之后源文件被修改时即可与当前内容做行级差异比对。
    pub fn set_ref_snapshot(&mut self, key: &str, text: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ref_snapshots (key, text) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET text = excluded.text",
            params![key, text],
        )?;
        Ok(())
    }

    /// 读取引用源片段快照。
    pub fn get_ref_snapshot(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT text FROM ref_snapshots WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// 列出全部引用快照。
    pub fn list_ref_snapshots(&self) -> Result<std::collections::HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, text FROM ref_snapshots")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = std::collections::HashMap::new();
        for r in rows {
            let (k, v) = r?;
            out.insert(k, v);
        }
        Ok(out)
    }

    /// 清空全部引用快照（重建基线时用）。
    pub fn clear_ref_snapshots(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM ref_snapshots", [])?;
        Ok(())
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str) -> DocumentRecord {
        DocumentRecord {
            path: path.into(),
            title: "t".into(),
            format: "Markdown".to_string(),
            size: 1,
            modified_at: 0,
            content_hash: "h".into(),
        }
    }

    #[test]
    fn documents_roundtrip() {
        let mut store = MetaStore::open_in_memory().unwrap();
        store.upsert_document(&rec("a.md")).unwrap();
        let got = store.get_document("a.md").unwrap().unwrap();
        assert_eq!(got.path, "a.md");
        assert_eq!(store.list_documents().unwrap().len(), 1);
        store.delete_document("a.md").unwrap();
        assert!(store.get_document("a.md").unwrap().is_none());
    }

    #[test]
    fn links_and_backlinks() {
        let mut store = MetaStore::open_in_memory().unwrap();
        store
            .replace_links(
                "a.md",
                &[LinkRecord {
                    from_path: "a.md".into(),
                    to_path: "b.md".into(),
                    target: "b".into(),
                    granularity: "block".into(),
                }],
            )
            .unwrap();
        assert_eq!(store.outgoing_links("a.md").unwrap().len(), 1);
        assert_eq!(store.backlinks("b.md").unwrap().len(), 1);
        assert_eq!(store.backlinks("c.md").unwrap().len(), 0);
    }
}

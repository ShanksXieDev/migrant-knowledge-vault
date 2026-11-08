use std::path::PathBuf;

/// 统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误 [{path}]: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("IO 错误: {0}")]
    PlainIo(#[from] std::io::Error),

    #[cfg(feature = "native")]
    #[error("索引错误: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[cfg(feature = "native")]
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[cfg(feature = "native")]
    #[error("Git 错误: {0}")]
    Git(#[from] git2::Error),

    #[error("解析错误 [{path}]: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("索引被锁定: {0}")]
    Locked(PathBuf),

    #[error("不支持的文件格式: {0}")]
    UnsupportedFormat(String),

    #[error("未找到文档: {0}")]
    NotFound(PathBuf),

    #[error("配置错误: {0}")]
    Config(String),

    #[error("其他错误: {0}")]
    Other(String),

    #[error("解析错误: {0}")]
    Anyhow(#[from] anyhow::Error),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    pub fn parse(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Error::Parse {
            path: path.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

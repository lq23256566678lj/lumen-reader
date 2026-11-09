use std::path::PathBuf;

use thiserror::Error;

/// Errors surfaced by the parser layer. Kept separate from UI/cloud errors so
/// that a bad book can never take down the whole library import.
#[derive(Debug, Error)]
pub enum BookError {
    #[error("不支持的电子书格式：{0}")]
    UnsupportedFormat(String),

    #[error("文件不存在：{0}")]
    FileNotFound(PathBuf),

    #[error("文件受 DRM 保护（{0}）。本阅读器不会移除 DRM，请使用已授权的无 DRM 副本。")]
    DrmProtected(String),

    #[error("PDF 已加密或需要密码，无法打开")]
    PdfEncrypted,

    #[error(
        "PDFium 未找到：请把 pdfium 动态库放到程序目录或 pdfium/ 目录，或设置 PDFIUM_DYNAMIC_LIB_PATH"
    )]
    PdfiumUnavailable,

    #[error("PDF 解析失败：{0}")]
    Pdf(String),

    #[error("章节索引越界：{0}")]
    ChapterOutOfRange(usize),

    #[error("解析失败：{0}")]
    Parse(String),

    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),

    #[error("压缩包损坏或受密码保护：{0}")]
    Archive(String),
}

impl From<ebook_rs::EbookError> for BookError {
    fn from(value: ebook_rs::EbookError) -> Self {
        use ebook_rs::EbookError;
        match value {
            EbookError::DrmProtected(message) => Self::DrmProtected(message),
            EbookError::Io(message) => Self::Parse(message),
            EbookError::Zip(message) => Self::Archive(message),
            other => Self::Parse(other.to_string()),
        }
    }
}

impl From<pdfium_render::prelude::PdfiumError> for BookError {
    fn from(value: pdfium_render::prelude::PdfiumError) -> Self {
        Self::Pdf(value.to_string())
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("无法定位用户数据目录")]
    NoDataDir,

    #[error("无法创建数据目录 {path}：{source}")]
    CreateDataDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("无法读取 {path}：{source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("无法写入 {path}：{source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON 数据格式错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Book(#[from] BookError),

    #[error("找不到可用的系统字体，请安装至少一种 TTF/OTF 字体")]
    NoSystemFont,

    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("尚未配置 WebDAV")]
    NotConfigured,

    #[error("WebDAV 地址无效：{0}")]
    InvalidUrl(String),

    #[error("WebDAV 认证失败（HTTP {0}）")]
    Unauthorized(u16),

    #[error("WebDAV 请求失败（HTTP {0}）：{1}")]
    Http(u16, String),

    #[error("网络请求失败：{0}")]
    Network(String),

    #[error("远端数据格式错误：{0}")]
    InvalidData(String),
}

impl From<reqwest::Error> for CloudError {
    fn from(value: reqwest::Error) -> Self {
        Self::Network(value.to_string())
    }
}

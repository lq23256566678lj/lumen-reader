use std::path::{Path, PathBuf};

use pdfium_render::prelude::{
    PdfBookmark, PdfDocumentMetadataTagType, PdfRenderConfig, Pdfium, PdfiumError,
};

use crate::error::BookError;
use crate::parser::book_source::BookSource;
use crate::state::models::{BookFormat, BookMetadata, ChapterBody, RenderedPage, TocEntry};

pub struct PdfSource {
    path: PathBuf,
    metadata: BookMetadata,
    toc: Vec<TocEntry>,
    page_count: usize,
}

impl PdfSource {
    pub fn open(path: &Path) -> Result<Self, BookError> {
        let pdfium = bind_pdfium()?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(map_pdfium_error)?;

        let page_count = document.pages().len() as usize;
        if page_count == 0 {
            return Err(BookError::Pdf("PDF 没有页面".to_owned()));
        }

        let fallback = BookMetadata::fallback(path, BookFormat::Pdf);
        let pdf_meta = document.metadata();
        let title = pdf_meta
            .get(PdfDocumentMetadataTagType::Title)
            .map(|tag| tag.value().trim().to_owned())
            .filter(|title| !title.is_empty())
            .map_or_else(|| fallback.title.clone(), |title| title);
        let author = pdf_meta
            .get(PdfDocumentMetadataTagType::Author)
            .map(|tag| tag.value().trim().to_owned())
            .filter(|author| !author.is_empty())
            .map_or_else(String::new, |author| author);
        let metadata = BookMetadata {
            title,
            author,
            publisher: String::new(),
            language: String::new(),
            description: String::new(),
            identifier: String::new(),
            format: BookFormat::Pdf,
        };

        let mut toc = Vec::new();
        for bookmark in document.bookmarks().iter() {
            let title = bookmark
                .title()
                .map(|title| title.trim().to_owned())
                .filter(|title| !title.is_empty())
                .map_or_else(|| "未命名章节".to_owned(), |title| title);
            let chapter = bookmark
                .destination()
                .and_then(|destination| destination.page_index().ok())
                .map_or(0, |index| index as usize)
                .min(page_count - 1);
            toc.push(TocEntry {
                title,
                level: bookmark_depth(&bookmark),
                chapter,
                fraction: 0.0,
            });
        }
        if toc.is_empty() {
            toc = (0..page_count)
                .map(|index| TocEntry {
                    title: format!("第 {} 页", index + 1),
                    level: 0,
                    chapter: index,
                    fraction: 0.0,
                })
                .collect();
        }

        Ok(Self {
            path: path.to_path_buf(),
            metadata,
            toc,
            page_count,
        })
    }
}

impl BookSource for PdfSource {
    fn metadata(&self) -> &BookMetadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn chapter_count(&self) -> usize {
        self.page_count
    }

    fn chapter_content(&self, index: usize) -> Result<ChapterBody, BookError> {
        if index >= self.page_count {
            return Err(BookError::ChapterOutOfRange(index));
        }
        Ok(ChapterBody::Page)
    }

    fn is_paged(&self) -> bool {
        true
    }

    fn render_page(&self, index: usize, max_width: u32) -> Result<Option<RenderedPage>, BookError> {
        if index >= self.page_count {
            return Err(BookError::ChapterOutOfRange(index));
        }
        let pdfium = bind_pdfium()?;
        let document = pdfium
            .load_pdf_from_file(&self.path, None)
            .map_err(map_pdfium_error)?;
        let page = document
            .pages()
            .get(index as i32)
            .map_err(map_pdfium_error)?;
        let config = PdfRenderConfig::new().set_target_width(max_width.clamp(320, 2600) as i32);
        let image = page
            .render_with_config(&config)
            .map_err(map_pdfium_error)?
            .as_image()
            .map_err(map_pdfium_error)?;
        let rgba = image.into_rgba8();
        let (width, height) = rgba.dimensions();
        Ok(Some(RenderedPage {
            width,
            height,
            rgba: rgba.into_raw(),
        }))
    }
}

fn bookmark_depth(bookmark: &PdfBookmark<'_>) -> usize {
    let mut depth = 0;
    let mut current = bookmark.clone();
    while let Some(parent) = current.parent() {
        depth += 1;
        if depth >= 8 {
            break;
        }
        current = parent;
    }
    depth
}

fn map_pdfium_error(error: PdfiumError) -> BookError {
    let text = error.to_string();
    if text.to_ascii_lowercase().contains("password")
        || text.to_ascii_lowercase().contains("encrypted")
    {
        BookError::PdfEncrypted
    } else {
        BookError::Pdf(text)
    }
}

/// Binds Pdfium from, in order: the environment override, the executable
/// directory, `./pdfium`, and the project's `pdfium/` directory. If none of
/// those contain a library, fall back to the system loader.
pub fn bind_pdfium() -> Result<Pdfium, BookError> {
    let mut directories: Vec<PathBuf> = Vec::new();
    if let Ok(configured) = std::env::var("PDFIUM_DYNAMIC_LIB_PATH") {
        if !configured.trim().is_empty() {
            directories.push(PathBuf::from(configured));
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            directories.push(parent.to_path_buf());
            directories.push(parent.join("pdfium"));
        }
    }
    directories.push(PathBuf::from("pdfium"));
    directories.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pdfium"));

    for directory in directories {
        let library = Pdfium::pdfium_platform_library_name_at_path(&directory);
        if !library.is_file() {
            continue;
        }
        match Pdfium::bind_to_library(&library) {
            Ok(bindings) => {
                log::info!("using Pdfium from {}", library.display());
                return Ok(Pdfium::new(bindings));
            }
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                // pdfium-render keeps a process-global bindings set; re-use
                // it instead of trying to load the DLL again.
                return Ok(Pdfium::default());
            }
            Err(error) => {
                log::warn!("failed to bind Pdfium at {}: {error}", library.display());
            }
        }
    }

    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(error) => {
            log::warn!("Pdfium unavailable: {error}");
            Err(BookError::PdfiumUnavailable)
        }
    }
}

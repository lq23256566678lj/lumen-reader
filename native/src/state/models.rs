use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Formats the reader advertises. Format detection is centralized here so the
/// UI never has to know how a file is parsed.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Mobi,
    Azw,
    Azw3,
    Pdf,
    Fb2,
    Cbz,
    Txt,
}

impl BookFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "epub" | "kepub" => Some(Self::Epub),
            "mobi" => Some(Self::Mobi),
            "azw" => Some(Self::Azw),
            "azw3" => Some(Self::Azw3),
            "pdf" => Some(Self::Pdf),
            "fb2" => Some(Self::Fb2),
            "cbz" => Some(Self::Cbz),
            "txt" | "text" => Some(Self::Txt),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Mobi => "MOBI",
            Self::Azw => "AZW",
            Self::Azw3 => "AZW3",
            Self::Pdf => "PDF",
            Self::Fb2 => "FB2",
            Self::Cbz => "CBZ",
            Self::Txt => "TXT",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Epub => egui::Color32::from_rgb(92, 160, 130),
            Self::Mobi | Self::Azw | Self::Azw3 => egui::Color32::from_rgb(216, 154, 74),
            Self::Pdf => egui::Color32::from_rgb(205, 92, 92),
            Self::Fb2 => egui::Color32::from_rgb(120, 130, 210),
            Self::Cbz => egui::Color32::from_rgb(192, 112, 178),
            Self::Txt => egui::Color32::from_rgb(140, 150, 160),
        }
    }

    pub fn extensions() -> Vec<&'static str> {
        vec![
            "epub", "kepub", "mobi", "azw", "azw3", "pdf", "fb2", "cbz", "txt",
        ]
    }
}

/// Metadata as returned by any parser. Cover bytes stay out of the persisted
/// library JSON; they are written as a PNG next to the data file.
#[derive(Clone, Debug)]
pub struct BookMetadata {
    pub title: String,
    pub author: String,
    pub publisher: String,
    pub language: String,
    pub description: String,
    pub identifier: String,
    pub format: BookFormat,
}

impl BookMetadata {
    pub fn fallback(path: &Path, format: BookFormat) -> Self {
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.trim().to_owned())
            .filter(|stem| !stem.is_empty())
            .map_or_else(|| "未命名书籍".to_owned(), |stem| stem);
        Self {
            title,
            author: String::new(),
            publisher: String::new(),
            language: String::new(),
            description: String::new(),
            identifier: String::new(),
            format,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub title: String,
    pub level: usize,
    pub chapter: usize,
    /// Position within the chapter, 0..=1. Reserved for CFI-driven jumps.
    pub fraction: f32,
}

#[derive(Clone, Debug)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// Premultiplied? No: straight RGBA, matching `ColorImage::from_rgba_unmultiplied`.
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum ChapterBody {
    Text(String),
    /// Reflowable text plus illustrations embedded in the chapter source.
    Rich {
        text: String,
        images: Vec<Vec<u8>>,
    },
    /// The chapter is a page image; call `BookSource::render_page` for pixels.
    Page,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ReadingProgress {
    pub chapter: usize,
    pub fraction: f32,
    /// Pixel offset inside the scroll area. Helps resume exactly where the
    /// reader left off, even if the window size changed slightly.
    pub scroll_offset: f32,
    /// Unix seconds. Used by the cloud merge.
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BookRecord {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    /// Folder/collection id. `None` means the book is not in a custom folder.
    #[serde(default)]
    pub collection_id: Option<String>,
    pub format: BookFormat,
    /// `None` for a cloud-only shelf entry whose book file is not present yet.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub cover_file: Option<String>,
    pub chapter_count: usize,
    #[serde(default)]
    pub progress: ReadingProgress,
    pub added_at: u64,
    #[serde(default)]
    pub last_opened_at: Option<u64>,
}

/// A user-created bookshelf folder. It only groups local records; moving a
/// book into a collection never moves or copies its file.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub deleted: bool,
}

impl Collection {
    pub fn is_active(&self) -> bool {
        !self.deleted
    }
}

impl BookRecord {
    pub fn display_author(&self) -> &str {
        if self.author.trim().is_empty() {
            "未知作者"
        } else {
            &self.author
        }
    }

    pub fn overall_progress(&self) -> f32 {
        if self.chapter_count == 0 {
            return 0.0;
        }
        if self.chapter_count == 1 {
            return self.progress.fraction.clamp(0.0, 1.0);
        }
        let chapter = self.progress.chapter.min(self.chapter_count - 1) as f32;
        ((chapter + self.progress.fraction.clamp(0.0, 1.0)) / self.chapter_count as f32)
            .clamp(0.0, 1.0)
    }

    pub fn is_available(&self) -> bool {
        self.path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bookmark {
    pub id: String,
    pub book_id: String,
    pub chapter: usize,
    pub fraction: f32,
    pub label: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub deleted: bool,
}

impl Bookmark {
    pub fn is_active(&self) -> bool {
        !self.deleted
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LibraryData {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default)]
    pub books: Vec<BookRecord>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub collections: Vec<Collection>,
}

fn schema_version() -> u32 {
    2
}

impl Default for LibraryData {
    fn default() -> Self {
        Self {
            version: schema_version(),
            books: Vec::new(),
            bookmarks: Vec::new(),
            collections: Vec::new(),
        }
    }
}

impl LibraryData {
    pub fn book(&self, id: &str) -> Option<&BookRecord> {
        self.books.iter().find(|book| book.id == id)
    }

    pub fn book_mut(&mut self, id: &str) -> Option<&mut BookRecord> {
        self.books.iter_mut().find(|book| book.id == id)
    }

    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.collections
            .iter()
            .find(|collection| collection.id == id && collection.is_active())
    }

    pub fn collection_mut(&mut self, id: &str) -> Option<&mut Collection> {
        self.collections
            .iter_mut()
            .find(|collection| collection.id == id && collection.is_active())
    }

    pub fn active_collections(&self) -> Vec<&Collection> {
        let mut result: Vec<&Collection> = self
            .collections
            .iter()
            .filter(|collection| collection.is_active())
            .collect();
        result.sort_by(|left, right| left.name.cmp(&right.name));
        result
    }

    pub fn collection_book_count(&self, id: &str) -> usize {
        self.books
            .iter()
            .filter(|book| book.collection_id.as_deref() == Some(id))
            .count()
    }

    /// Collection tombstones stay in the data for cloud merging, but must not
    /// leave books pointing at a dead folder.
    pub fn clear_deleted_collection_links(&mut self) {
        let active: Vec<String> = self
            .collections
            .iter()
            .filter(|collection| collection.is_active())
            .map(|collection| collection.id.clone())
            .collect();
        for book in &mut self.books {
            if let Some(collection_id) = book.collection_id.as_deref() {
                if !active.iter().any(|id| id == collection_id) {
                    book.collection_id = None;
                }
            }
        }
    }

    pub fn active_bookmarks(&self, book_id: &str) -> Vec<&Bookmark> {
        let mut result: Vec<&Bookmark> = self
            .bookmarks
            .iter()
            .filter(|bookmark| bookmark.book_id == book_id && bookmark.is_active())
            .collect();
        result.sort_by_key(|bookmark| (bookmark.chapter, (bookmark.fraction * 1000.0) as u32));
        result
    }

    pub fn add_bookmark(&mut self, bookmark: Bookmark) {
        if let Some(existing) = self
            .bookmarks
            .iter_mut()
            .find(|existing| existing.id == bookmark.id)
        {
            *existing = bookmark;
        } else {
            self.bookmarks.push(bookmark);
        }
    }

    pub fn remove_bookmark(&mut self, id: &str) {
        let now = now_secs();
        if let Some(bookmark) = self.bookmarks.iter_mut().find(|bookmark| bookmark.id == id) {
            bookmark.deleted = true;
            bookmark.updated_at = now;
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    Scroll,
    Page,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scroll => "滚动",
            Self::Page => "翻页",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Scroll => Self::Page,
            Self::Page => Self::Scroll,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_line_width")]
    pub line_width: f32,
    #[serde(default = "default_theme")]
    pub theme: ThemeMode,
    #[serde(default = "default_view_mode")]
    pub view_mode: ViewMode,
    #[serde(default)]
    pub last_book_id: Option<String>,
    #[serde(default = "default_true")]
    pub restore_last_book: bool,
    #[serde(default = "default_true")]
    pub toc_open: bool,
}

fn default_font_size() -> f32 {
    19.0
}

fn default_line_width() -> f32 {
    760.0
}

fn default_theme() -> ThemeMode {
    ThemeMode::Dark
}

fn default_view_mode() -> ViewMode {
    ViewMode::Scroll
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            font_size: default_font_size(),
            line_width: default_line_width(),
            theme: default_theme(),
            view_mode: default_view_mode(),
            last_book_id: None,
            restore_last_book: true,
            toc_open: true,
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

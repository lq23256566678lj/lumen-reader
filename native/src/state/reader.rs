use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::error::BookError;
use crate::parser::{self, Source};
use crate::state::models::{
    BookRecord, Bookmark, ChapterBody, ReadingProgress, RenderedPage, ViewMode, now_secs,
};

pub struct ReaderSession {
    pub book_id: String,
    pub source: Arc<dyn Source>,
    pub chapter: usize,
    pub loaded_chapter: Option<usize>,
    pub text: Option<String>,
    pub images: Vec<Vec<u8>>,
    pub chapter_error: Option<String>,
    chapter_rx: Option<Receiver<(usize, Result<ChapterBody, String>)>>,
    loading_chapter: Option<usize>,
    pub fraction: f32,
    pub scroll_offset: f32,
    pub page_index: usize,
    pub view: ViewMode,
    pub pagination: Option<Pagination>,
    pub restored_page: bool,
    pub restore_scroll_from_fraction: bool,
    pub scroll_restore_attempts: u8,
    pub scroll_generation: u64,
    pub dirty: bool,
}

impl ReaderSession {
    pub fn open(record: &BookRecord, view: ViewMode) -> Result<Self, BookError> {
        let path = record
            .path
            .as_deref()
            .ok_or_else(|| BookError::Parse("云端书目尚未导入本地文件".to_owned()))?;
        let source: Arc<dyn Source> = Arc::from(parser::open_book(Path::new(path))?);
        let chapter_count = source.chapter_count();
        if chapter_count == 0 {
            return Err(BookError::Parse("没有可阅读章节".to_owned()));
        }
        // Chapter normalization can change the chapter count. Preserve the
        // reader's approximate place by mapping overall book progress into the
        // new chapter list instead of trusting the old spine index.
        let (chapter, start_fraction) =
            if record.chapter_count > 0 && record.chapter_count != chapter_count {
                let overall = (record.progress.chapter as f32
                    + record.progress.fraction.clamp(0.0, 1.0))
                    / record.chapter_count as f32;
                let scaled = (overall.clamp(0.0, 1.0) * chapter_count as f32)
                    .clamp(0.0, chapter_count.saturating_sub(1) as f32);
                let mapped = scaled.floor() as usize;
                (mapped.min(chapter_count - 1), scaled.fract())
            } else {
                (
                    record.progress.chapter.min(chapter_count - 1),
                    record.progress.fraction.clamp(0.0, 1.0),
                )
            };
        // Fraction survives window resizes and font changes; the cached pixel
        // offset is only a hint. Restoring by fraction is more robust.
        let restore_scroll_from_fraction = start_fraction > 0.001;
        let mut session = Self {
            book_id: record.id.clone(),
            source,
            chapter,
            loaded_chapter: None,
            text: None,
            images: Vec::new(),
            chapter_error: None,
            chapter_rx: None,
            loading_chapter: None,
            fraction: start_fraction,
            scroll_offset: record.progress.scroll_offset.max(0.0),
            page_index: 0,
            view,
            pagination: None,
            restored_page: false,
            restore_scroll_from_fraction,
            scroll_restore_attempts: 0,
            scroll_generation: 0,
            dirty: false,
        };
        session.ensure_chapter();
        Ok(session)
    }

    pub fn chapter_count(&self) -> usize {
        self.source.chapter_count()
    }

    pub fn is_paged_format(&self) -> bool {
        self.source.is_paged()
    }

    pub fn ensure_chapter(&mut self) {
        if self.loaded_chapter == Some(self.chapter) {
            return;
        }
        self.loaded_chapter = Some(self.chapter);
        self.text = None;
        self.images.clear();
        self.chapter_error = None;
        self.pagination = None;
        self.restored_page = false;
        self.chapter_rx = None;
        self.loading_chapter = None;

        // Page-image formats are cheap to switch; reflowable chapters can
        // parse tens of MB of XHTML, so never do that on the UI thread.
        if self.source.is_paged() {
            return;
        }

        let chapter = self.chapter;
        let source = Arc::clone(&self.source);
        let (sender, receiver) = mpsc::channel();
        self.chapter_rx = Some(receiver);
        self.loading_chapter = Some(chapter);
        let spawned = std::thread::Builder::new()
            .name(format!("lumen-chapter-{chapter}"))
            .spawn(move || {
                let result = source
                    .chapter_content(chapter)
                    .map_err(|error| error.to_string());
                let _ = sender.send((chapter, result));
            });
        if let Err(error) = spawned {
            self.chapter_rx = None;
            self.loading_chapter = None;
            self.chapter_error = Some(format!("无法启动章节加载线程：{error}"));
        }
    }

    /// Called once per frame by the app. It only checks a channel, so it is
    /// safe to call even while the reader is idle.
    pub fn poll_chapter(&mut self) {
        let mut disconnected = false;
        let received = {
            let Some(receiver) = self.chapter_rx.as_ref() else {
                return;
            };
            match receiver.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    None
                }
            }
        };

        if disconnected {
            self.chapter_rx = None;
            self.loading_chapter = None;
            if self.text.is_none() {
                self.chapter_error = Some("章节加载线程意外退出".to_owned());
            }
            return;
        }

        let Some((index, result)) = received else {
            return;
        };
        self.chapter_rx = None;
        self.loading_chapter = None;
        if index != self.chapter {
            return;
        }
        // A freshly loaded chapter must start at the top unless we are
        // explicitly restoring a saved fraction. Bumping the generation also
        // gives the scroll area a fresh state id.
        if !self.restore_scroll_from_fraction {
            self.fraction = 0.0;
            self.scroll_offset = 0.0;
            self.scroll_restore_attempts = 0;
            self.scroll_generation = self.scroll_generation.saturating_add(1);
        }
        match result {
            Ok(ChapterBody::Text(text)) => {
                self.text = Some(text);
                self.images.clear();
                self.chapter_error = None;
            }
            Ok(ChapterBody::Rich { text, images }) => {
                self.text = Some(text);
                self.images = images;
                self.chapter_error = None;
            }
            Ok(ChapterBody::Page) => {}
            Err(error) => self.chapter_error = Some(error),
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading_chapter.is_some() || self.chapter_rx.is_some()
    }

    pub fn reload_chapter(&mut self) {
        self.loaded_chapter = None;
        self.ensure_chapter();
    }

    pub fn go_to_chapter(&mut self, index: usize) {
        let index = index.min(self.chapter_count().saturating_sub(1));
        if index == self.chapter {
            return;
        }
        self.chapter = index;
        self.fraction = 0.0;
        self.scroll_offset = 0.0;
        self.page_index = 0;
        self.restore_scroll_from_fraction = false;
        self.scroll_restore_attempts = 0;
        self.scroll_generation = self.scroll_generation.saturating_add(1);
        self.ensure_chapter();
        self.dirty = true;
    }

    pub fn go_to_toc(&mut self, chapter: usize, fraction: f32) {
        if chapter != self.chapter {
            self.chapter = chapter.min(self.chapter_count().saturating_sub(1));
            self.ensure_chapter();
        }
        self.fraction = fraction.clamp(0.0, 1.0);
        self.page_index = 0;
        self.scroll_offset = 0.0;
        self.restored_page = false;
        self.restore_scroll_from_fraction = self.fraction > 0.001;
        self.scroll_restore_attempts = 0;
        self.scroll_generation = self.scroll_generation.saturating_add(1);
        self.pagination = None;
        self.dirty = true;
    }

    pub fn next_chapter(&mut self) {
        if self.chapter + 1 < self.chapter_count() {
            self.go_to_chapter(self.chapter + 1);
        }
    }

    pub fn previous_chapter(&mut self) {
        if self.chapter > 0 {
            self.go_to_chapter(self.chapter - 1);
        }
    }

    pub fn chapter_title(&self) -> String {
        let mut best: Option<&crate::state::models::TocEntry> = None;
        for entry in self.source.toc() {
            if entry.chapter <= self.chapter {
                if best.map_or(true, |current| entry.chapter >= current.chapter) {
                    best = Some(entry);
                }
            }
        }
        best.map_or_else(
            || format!("第 {} 节", self.chapter + 1),
            |entry| entry.title.clone(),
        )
    }

    pub fn overall_progress(&self) -> f32 {
        let count = self.chapter_count();
        if count == 0 {
            return 0.0;
        }
        if count == 1 {
            return self.fraction.clamp(0.0, 1.0);
        }
        ((self.chapter as f32 + self.fraction.clamp(0.0, 1.0)) / count as f32).clamp(0.0, 1.0)
    }

    pub fn progress(&self) -> ReadingProgress {
        ReadingProgress {
            chapter: self.chapter,
            fraction: self.fraction.clamp(0.0, 1.0),
            scroll_offset: self.scroll_offset.max(0.0),
            updated_at: now_secs(),
        }
    }

    pub fn update_scroll(&mut self, offset: f32, content_height: f32, viewport_height: f32) {
        self.scroll_offset = offset.max(0.0);
        let max_offset = (content_height - viewport_height).max(1.0);
        let fraction = (self.scroll_offset / max_offset).clamp(0.0, 1.0);
        if (fraction - self.fraction).abs() > 0.001 {
            self.fraction = fraction;
            self.dirty = true;
        }
    }

    pub fn set_page(&mut self, page: usize, page_count: usize) {
        self.page_index = page;
        let fraction = if page_count == 0 {
            0.0
        } else {
            (page as f32 / page_count as f32).clamp(0.0, 1.0)
        };
        if (fraction - self.fraction).abs() > 0.001 {
            self.fraction = fraction;
            self.dirty = true;
        }
    }

    pub fn restore_page(&mut self, page_count: usize) {
        if self.restored_page {
            return;
        }
        self.restored_page = true;
        if page_count == 0 {
            self.page_index = 0;
            return;
        }
        let page = (self.fraction * page_count as f32).round() as usize;
        self.page_index = page.min(page_count - 1);
    }

    pub fn next_page(&mut self, page_count: usize) {
        if self.page_index.saturating_add(1) < page_count {
            self.page_index = self.page_index.saturating_add(1);
            self.set_page(self.page_index, page_count);
        } else {
            self.next_chapter();
        }
    }

    pub fn previous_page(&mut self, page_count: usize) {
        if self.page_index > 0 {
            self.page_index -= 1;
            self.set_page(self.page_index, page_count);
        } else if self.chapter > 0 {
            self.previous_chapter();
            // The UI re-paginates and clamps to the last page next frame.
            self.page_index = usize::MAX;
        }
    }

    pub fn render_page(
        &self,
        index: usize,
        max_width: u32,
    ) -> Result<Option<RenderedPage>, BookError> {
        self.source.render_page(index, max_width)
    }

    pub fn bookmark_label(&self) -> String {
        format!(
            "{} · {:.0}%",
            self.chapter_title(),
            self.overall_progress() * 100.0
        )
    }

    pub fn make_bookmark(&self) -> Bookmark {
        let now = now_secs();
        Bookmark {
            id: format!("{}-{}-{now}", self.book_id, self.chapter),
            book_id: self.book_id.clone(),
            chapter: self.chapter,
            fraction: self.fraction.clamp(0.0, 1.0),
            label: self.bookmark_label(),
            created_at: now,
            updated_at: now,
            deleted: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaginationKey {
    pub width: u32,
    pub height: u32,
    pub font_size_tenths: u32,
}

#[derive(Clone, Debug)]
pub struct Pagination {
    pub key: PaginationKey,
    pub pages: Vec<Range<usize>>,
}

impl Pagination {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page_text<'a>(&self, text: &'a str, page: usize) -> &'a str {
        self.pages
            .get(page)
            .and_then(|range| text.get(range.clone()))
            .map_or("", |slice| slice)
    }
}

/// Approximate reflow pagination. It estimates glyph widths rather than
/// laying out the whole book; only the current chapter is ever paginated.
pub fn paginate_text(
    text: &str,
    usable_width: f32,
    usable_height: f32,
    font_size: f32,
) -> Pagination {
    let font_size = font_size.max(12.0);
    // Work in em units: one CJK glyph is ~1.0em wide, typical Latin glyphs
    // are ~0.55em. Dividing by font size avoids cramming twice as many CJK
    // characters into a line as the renderer actually fits.
    let max_columns = (usable_width / font_size).max(8.0);
    let line_height = font_size * 1.55;
    let lines_per_page = (usable_height / line_height).floor().max(1.0);
    let mut pages = Vec::new();
    let mut page_start = 0;
    let mut lines = 0.0_f32;
    let mut column = 0.0_f32;

    for (index, character) in text.char_indices() {
        if character == '\n' {
            lines += 1.0;
            column = 0.0;
        } else {
            let width = glyph_columns(character);
            if column + width > max_columns {
                lines += 1.0;
                column = width;
            } else {
                column += width;
            }
        }

        if lines >= lines_per_page {
            let end = index + character.len_utf8();
            if end > page_start {
                pages.push(page_start..end);
                page_start = end;
            }
            lines = 0.0;
            column = 0.0;
        }
    }

    if page_start < text.len() || pages.is_empty() {
        pages.push(page_start..text.len());
    }

    let key = PaginationKey {
        width: usable_width.round().max(0.0) as u32,
        height: usable_height.round().max(0.0) as u32,
        font_size_tenths: (font_size * 10.0).round().max(0.0) as u32,
    };
    Pagination { key, pages }
}

fn glyph_columns(character: char) -> f32 {
    if character.is_whitespace() {
        0.32
    } else if character.is_ascii() {
        0.55
    } else if (character as u32) < 0x2E80 {
        0.85
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_is_contiguous_and_covers_all_text() {
        let text = "第一章 开始\n\n这是一段用于测试分页的中文文字。The quick brown fox jumps over the lazy dog.\n"
            .repeat(240);
        let pagination = paginate_text(&text, 700.0, 560.0, 19.0);
        assert!(pagination.page_count() > 1);
        let mut cursor = 0;
        for range in &pagination.pages {
            assert_eq!(range.start, cursor);
            assert!(range.end >= range.start);
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
            cursor = range.end;
        }
        assert_eq!(cursor, text.len());
    }

    #[test]
    fn pagination_handles_empty_text() {
        let pagination = paginate_text("", 420.0, 300.0, 18.0);
        assert_eq!(pagination.page_count(), 1);
        assert_eq!(
            pagination.pages.first().map(|range| range.clone()),
            Some(0..0)
        );
    }

    #[test]
    fn chapter_loading_runs_off_the_ui_thread() {
        let path = std::env::temp_dir().join(format!(
            "lumen-async-chapter-{}-{}.txt",
            std::process::id(),
            now_secs()
        ));
        assert!(std::fs::write(&path, "第一章\n\n后台加载测试。").is_ok());
        let record = BookRecord {
            id: "async-test".to_owned(),
            title: "异步测试".to_owned(),
            author: String::new(),
            collection_id: None,
            format: crate::state::models::BookFormat::Txt,
            path: Some(path.to_string_lossy().into_owned()),
            file_size: 32,
            cover_file: None,
            chapter_count: 1,
            progress: ReadingProgress::default(),
            added_at: now_secs(),
            last_opened_at: None,
        };

        let mut session = match ReaderSession::open(&record, ViewMode::Scroll) {
            Ok(session) => session,
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                panic!("reader should open: {error}");
            }
        };
        for _ in 0..200 {
            session.poll_chapter();
            if session.text.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let loaded = session.text.is_some();
        let _ = std::fs::remove_file(&path);
        assert!(loaded, "chapter should be delivered by the worker thread");
    }
}

use crate::error::BookError;
use crate::state::models::{BookMetadata, ChapterBody, RenderedPage, TocEntry};

/// The format-agnostic reading contract. The UI only talks to this trait; it
/// never matches on EPUB/PDF/CBZ/etc.
pub trait BookSource: Send + Sync {
    fn metadata(&self) -> &BookMetadata;
    fn toc(&self) -> &[TocEntry];
    fn chapter_count(&self) -> usize;
    fn chapter_content(&self, index: usize) -> Result<ChapterBody, BookError>;

    /// True for PDF and comic archives where each chapter is a page image.
    fn is_paged(&self) -> bool {
        false
    }

    fn cover(&self) -> Option<&[u8]> {
        None
    }

    /// Renders one page at (approximately) `max_width` pixels. Returns `None`
    /// for reflowable formats.
    fn render_page(
        &self,
        _index: usize,
        _max_width: u32,
    ) -> Result<Option<RenderedPage>, BookError> {
        Ok(None)
    }
}

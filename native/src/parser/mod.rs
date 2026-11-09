pub mod book_source;
mod drm;
mod ebook;
pub(crate) mod html_text;
mod pdf;

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use book_source::BookSource;
use ebook::EbookSource;
use pdf::PdfSource;

use crate::error::BookError;
use crate::state::models::{BookFormat, RenderedPage};

pub use book_source::BookSource as Source;

/// Opens any supported book. DRM checks happen before a parser is invoked.
pub fn open_book(path: &Path) -> Result<Box<dyn BookSource>, BookError> {
    if !path.is_file() {
        return Err(BookError::FileNotFound(path.to_path_buf()));
    }
    let format = detect_format(path)?;
    drm::check(path, format)?;

    let source: Box<dyn BookSource> = match format {
        BookFormat::Pdf => Box::new(PdfSource::open(path)?),
        _ => Box::new(EbookSource::open(path, format)?),
    };
    Ok(source)
}

/// Returns a cover as encoded PNG bytes. Paged formats render their first page
/// when the container does not expose a dedicated cover image.
pub fn extract_cover(source: &dyn BookSource) -> Option<Vec<u8>> {
    if let Some(bytes) = source.cover() {
        if !bytes.is_empty() {
            return Some(bytes.to_vec());
        }
    }
    if !source.is_paged() {
        return None;
    }

    let page: RenderedPage = source.render_page(0, 360).ok().flatten()?;
    let rgba = image::RgbaImage::from_raw(page.width, page.height, page.rgba)?;
    let dynamic = image::DynamicImage::ImageRgba8(rgba);
    let mut cursor = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    Some(cursor.into_inner())
}

pub fn detect_format(path: &Path) -> Result<BookFormat, BookError> {
    if let Some(format) = BookFormat::from_path(path) {
        return Ok(format);
    }

    let head = read_head(path, 1024)?;
    if head.starts_with(b"%PDF") {
        return Ok(BookFormat::Pdf);
    }
    if head.starts_with(b"PK\x03\x04") {
        if zip_has_container(path) {
            return Ok(BookFormat::Epub);
        }
        return Ok(BookFormat::Cbz);
    }
    if head.len() >= 68 && &head[60..68] == b"BOOKMOBI" {
        return Ok(BookFormat::Mobi);
    }
    if head
        .windows(b"FictionBook".len())
        .any(|window| window == b"FictionBook")
    {
        return Ok(BookFormat::Fb2);
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map_or_else(|| "无扩展名".to_owned(), |value| format!(".{value}"));
    Err(BookError::UnsupportedFormat(format!(
        "{} ({extension})",
        path.display()
    )))
}

fn read_head(path: &Path, limit: usize) -> Result<Vec<u8>, BookError> {
    let mut file = File::open(path)?;
    let mut head = vec![0_u8; limit];
    let read = file.read(&mut head)?;
    head.truncate(read);
    Ok(head)
}

fn zip_has_container(path: &Path) -> bool {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return false,
    };
    for index in 0..archive.len() {
        let entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(_) => return false,
        };
        let name = match entry.name() {
            Ok(name) => name,
            Err(_) => return false,
        };
        if name.eq_ignore_ascii_case("META-INF/container.xml") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::ChapterBody;

    #[test]
    fn opens_utf8_txt_through_the_parser_layer() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "lumen-reader-test-{}-{}.txt",
            std::process::id(),
            crate::state::models::now_secs()
        ));
        let body = "第一章 测试\n\nHello from Lumen Reader.\n\n第二章\n\n第二段文字。";
        std::fs::write(&path, body)?;
        let source = open_book(&path);
        let _ = std::fs::remove_file(&path);
        let source = source?;
        assert!(source.chapter_count() >= 1);
        assert_eq!(source.metadata().format, BookFormat::Txt);
        match source.chapter_content(0)? {
            ChapterBody::Text(text) => assert!(!text.is_empty()),
            other => {
                return Err(format!("expected text chapter, got {other:?}").into());
            }
        }
        Ok(())
    }
}

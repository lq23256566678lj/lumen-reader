use std::collections::HashMap;
use std::path::Path;

use ebook_rs::Book;
use ebook_rs::EpubArchive;
use ebook_rs::nav::NavPoint;

use crate::error::BookError;
use crate::parser::book_source::BookSource;
use crate::state::models::{BookFormat, BookMetadata, ChapterBody, RenderedPage, TocEntry};

/// Adapter over `ebook-rs` for every reflowable format plus CBZ. The parser
/// itself is format-agnostic from the UI's point of view.
pub struct EbookSource {
    /// Reflowable books are normalized once, then this can be dropped so the
    /// hydrated XHTML (including base64-inlined images) is not kept in RAM.
    book: Option<Book>,
    metadata: BookMetadata,
    toc: Vec<TocEntry>,
    chapter_count: usize,
    cover: Option<Vec<u8>>,
    comic: bool,
    /// TXT files often arrive as a single huge section. Splitting into chunks
    /// keeps layout and memory bounded while preserving a simple chapter API.
    virtual_text: Option<Vec<String>>,
    /// Illustrations aligned with `virtual_text`; empty when a chapter has none.
    virtual_images: Option<Vec<Vec<Vec<u8>>>>,
}

impl EbookSource {
    pub fn open(path: &Path, format: BookFormat) -> Result<Self, BookError> {
        let fallback = BookMetadata::fallback(path, format);
        // EPUB is the one format where ebook-rs exposes a genuinely lazy
        // archive path; other formats keep their native parser behavior.
        let book = if format == BookFormat::Epub {
            let archive = EpubArchive::open(path)?;
            Book::from_archive_lazy(archive, &fallback.title)?
        } else {
            Book::from_file(path)?
        };
        let raw_meta = book.metadata();

        let title = if raw_meta.title.trim().is_empty() {
            fallback.title.clone()
        } else {
            raw_meta.title.trim().to_owned()
        };
        let metadata = BookMetadata {
            title,
            author: raw_meta.creator().trim().to_owned(),
            publisher: raw_meta.publisher().map_or_else(String::new, str::to_owned),
            language: raw_meta.language().trim().to_owned(),
            description: raw_meta
                .description
                .clone()
                .map_or_else(String::new, |text| text.trim().to_owned()),
            identifier: raw_meta
                .identifier
                .clone()
                .map_or_else(String::new, |text| text.trim().to_owned()),
            format,
        };

        let comic = format == BookFormat::Cbz;
        let mut chapter_count = book.sections().len().max(book.spine().len());
        if chapter_count == 0 {
            return Err(BookError::Parse("没有找到可阅读章节".to_owned()));
        }

        let mut virtual_text = None;
        let mut toc = if comic {
            comic_toc(chapter_count)
        } else {
            build_toc(&book, chapter_count)
        };

        if format == BookFormat::Txt {
            if let Ok(section) = book.get_section(0) {
                let chunks = split_text_chunks(
                    &crate::parser::html_text::to_paragraph_text(&section.raw_html),
                    24_000,
                );
                if chunks.len() > 1 {
                    chapter_count = chunks.len();
                    toc = chunks
                        .iter()
                        .enumerate()
                        .map(|(index, chunk)| TocEntry {
                            title: chunk_title(chunk, index),
                            level: 0,
                            chapter: index,
                            fraction: 0.0,
                        })
                        .collect();
                    virtual_text = Some(chunks);
                }
            }
        }

        // Spine files are a poor chapter unit: EPUBs often put a whole book in
        // one XHTML file, or split front matter into several almost-empty
        // files. Build balanced chapters from headings and merge the scraps.
        let mut virtual_images = None;
        if format != BookFormat::Txt && !comic {
            if let Some((chapters, images, normalized_toc)) =
                normalize_reflowable(&book, &toc, chapter_count)
            {
                chapter_count = chapters.len();
                toc = normalized_toc;
                virtual_text = Some(chapters);
                virtual_images = Some(images);
            }
        }

        let cover = book.cover_image().map(|(bytes, _mime)| bytes);
        let drop_book = virtual_text.is_some() && !comic;

        Ok(Self {
            book: if drop_book { None } else { Some(book) },
            metadata,
            toc,
            chapter_count,
            cover,
            comic,
            virtual_text,
            virtual_images,
        })
    }

    fn decode_page(&self, bytes: &[u8], max_width: u32) -> Result<RenderedPage, BookError> {
        let image = image::load_from_memory(bytes)
            .map_err(|error| BookError::Parse(format!("页面图片解码失败：{error}")))?;
        let target_width = max_width.clamp(240, 2600);
        let image = if image.width() > target_width {
            let ratio = target_width as f32 / image.width() as f32;
            let target_height = ((image.height() as f32 * ratio).round() as u32).clamp(1, 8000);
            image.resize_exact(
                target_width,
                target_height,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let rgba = image.into_rgba8();
        let (width, height) = rgba.dimensions();
        Ok(RenderedPage {
            width,
            height,
            rgba: rgba.into_raw(),
        })
    }
}

impl BookSource for EbookSource {
    fn metadata(&self) -> &BookMetadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn chapter_count(&self) -> usize {
        self.chapter_count
    }

    fn chapter_content(&self, index: usize) -> Result<ChapterBody, BookError> {
        if index >= self.chapter_count {
            return Err(BookError::ChapterOutOfRange(index));
        }
        if self.comic {
            return Ok(ChapterBody::Page);
        }
        if let Some(chunks) = &self.virtual_text {
            let text = chunks
                .get(index)
                .map_or_else(String::new, |chunk| chunk.trim().to_owned());
            let images = self
                .virtual_images
                .as_ref()
                .and_then(|all| all.get(index))
                .cloned()
                .unwrap_or_default();
            if images.is_empty() {
                return Ok(ChapterBody::Text(text));
            }
            return Ok(ChapterBody::Rich { text, images });
        }

        let Some(book) = self.book.as_ref() else {
            return Ok(ChapterBody::Text("（本章暂无可用文本）".to_owned()));
        };
        let section = book
            .get_section(index)
            .map_err(|error| BookError::Parse(error.to_string()))?;
        let image_html = if section.processed_html.is_empty() {
            &section.raw_html
        } else {
            &section.processed_html
        };
        let (marked_text, images) =
            crate::parser::html_text::to_paragraph_text_with_images(image_html);
        let text = if marked_text.trim().is_empty() {
            section.plain_text.trim().to_owned()
        } else {
            marked_text
        };
        if text.is_empty() {
            Ok(ChapterBody::Text("（本章暂无可用文本）".to_owned()))
        } else if images.is_empty() {
            Ok(ChapterBody::Text(text))
        } else {
            Ok(ChapterBody::Rich { text, images })
        }
    }

    fn is_paged(&self) -> bool {
        self.comic
    }

    fn cover(&self) -> Option<&[u8]> {
        self.cover.as_deref()
    }

    fn render_page(&self, index: usize, max_width: u32) -> Result<Option<RenderedPage>, BookError> {
        if !self.comic {
            return Ok(None);
        }
        if index >= self.chapter_count {
            return Err(BookError::ChapterOutOfRange(index));
        }
        let Some(book) = self.book.as_ref() else {
            return Err(BookError::Parse("页面图片数据不可用".to_owned()));
        };

        let images = ebook_rs::CbzBook::prefetch_page_images(book, index, 1);
        let bytes = images
            .iter()
            .find(|(page, _, _)| *page == index)
            .or_else(|| images.first())
            .map(|(_, _, bytes)| bytes.clone())
            .ok_or_else(|| BookError::Parse("漫画页图片读取失败".to_owned()))?;
        self.decode_page(&bytes, max_width).map(Some)
    }
}

#[derive(Clone, Debug)]
struct ChapterDraft {
    title: String,
    level: usize,
    text: String,
    images: Vec<Vec<u8>>,
}

/// Rebuild a chapter-local image vector from the positional markers in text.
fn assign_images(text: &str, source: &[Vec<u8>]) -> (String, Vec<Vec<u8>>) {
    use crate::parser::html_text::{IMAGE_END, IMAGE_START};
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut images = Vec::new();
    let mut index = 0usize;
    let mut found_marker = false;
    while index < chars.len() {
        if chars[index] == IMAGE_START {
            found_marker = true;
            let mut end = index + 1;
            while end < chars.len() && chars[end] != IMAGE_END {
                end += 1;
            }
            if end < chars.len() {
                let digits: String = chars[index + 1..end].iter().collect();
                if let Ok(source_index) = digits.parse::<usize>() {
                    if let Some(bytes) = source.get(source_index) {
                        let local_index = images.len();
                        images.push(bytes.clone());
                        output.push(IMAGE_START);
                        output.push_str(&local_index.to_string());
                        output.push(IMAGE_END);
                    }
                }
                index = end + 1;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    if !found_marker && !source.is_empty() {
        for (local_index, bytes) in source.iter().enumerate() {
            output.push(IMAGE_START);
            output.push_str(&local_index.to_string());
            output.push(IMAGE_END);
            images.push(bytes.clone());
        }
    }
    (output, images)
}

fn shift_image_markers(text: &str, offset: usize) -> String {
    use crate::parser::html_text::{IMAGE_END, IMAGE_START};
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == IMAGE_START {
            let mut end = index + 1;
            while end < chars.len() && chars[end] != IMAGE_END {
                end += 1;
            }
            if end < chars.len() {
                let digits: String = chars[index + 1..end].iter().collect();
                if let Ok(value) = digits.parse::<usize>() {
                    output.push(IMAGE_START);
                    output.push_str(&(value + offset).to_string());
                    output.push(IMAGE_END);
                    index = end + 1;
                    continue;
                }
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn append_draft(target: &mut ChapterDraft, source: ChapterDraft) {
    if !target.text.trim().is_empty() && !source.text.trim().is_empty() {
        target.text.push_str("\n\n");
    }
    let shifted = shift_image_markers(&source.text, target.images.len());
    target.text.push_str(&shifted);
    target.images.extend(source.images);
}

/// Converts spine files into readable units. A spine file may be a cover, a
/// table of contents, one chapter, several chapters, or an entire book. This
/// routine discards front matter, merges very short scraps, and splits large
/// files at strong heading lines.
fn normalize_reflowable(
    book: &Book,
    old_toc: &[TocEntry],
    section_count: usize,
) -> Option<(Vec<String>, Vec<Vec<Vec<u8>>>, Vec<TocEntry>)> {
    if section_count == 0 {
        return None;
    }

    let mut section_titles: HashMap<usize, (String, usize)> = HashMap::new();
    for entry in old_toc {
        section_titles
            .entry(entry.chapter)
            .or_insert_with(|| (entry.title.clone(), entry.level));
    }

    let mut front_matter = Vec::new();
    let mut drafts = Vec::new();
    let mut seen_story = false;
    for index in 0..section_count {
        let Ok(section) = book.get_section(index) else {
            continue;
        };
        let base_title = section_titles.get(&index).map_or_else(
            || format!("第 {} 节", index + 1),
            |(title, _)| title.clone(),
        );
        let level = section_titles.get(&index).map_or(0, |(_, level)| *level);
        let image_html = if section.processed_html.is_empty() {
            &section.raw_html
        } else {
            &section.processed_html
        };
        let (converted, images) =
            crate::parser::html_text::to_paragraph_text_with_images(image_html);
        let text = if converted.trim().is_empty() {
            section.plain_text.trim().to_owned()
        } else {
            converted
        };
        let visible_text = crate::parser::html_text::strip_image_markers(&text);
        if visible_text.trim().is_empty() && images.is_empty() {
            continue;
        }

        let story_start = is_story_title(&base_title) || visible_text.chars().count() >= 280;
        let is_front = is_front_matter_title(&base_title)
            || is_toc_like(&visible_text)
            || visible_text.trim().is_empty()
            || (!seen_story && !story_start);
        if is_front {
            let title = if images.is_empty() || !base_title.starts_with("第 ") {
                base_title
            } else {
                format!("彩页 {}", front_matter.len() + 1)
            };
            front_matter.push(ChapterDraft {
                title,
                level: 0,
                text,
                images,
            });
            continue;
        }
        seen_story = true;
        drafts.extend(split_section(&text, &base_title, level, images));
    }

    if let Some((cover, _)) = book.cover_image() {
        let already_present = front_matter
            .iter()
            .any(|chapter| chapter.images.iter().any(|image| image == &cover));
        if !already_present {
            front_matter.insert(
                0,
                ChapterDraft {
                    title: "封面".to_owned(),
                    level: 0,
                    text: String::new(),
                    images: vec![cover],
                },
            );
        }
    }

    let front_matter = consolidate_color_pages(front_matter);
    let mut chapters = front_matter;
    chapters.extend(merge_short_chapters(drafts));
    if chapters.is_empty() {
        return None;
    }

    let toc = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| TocEntry {
            title: chapter.title.clone(),
            level: chapter.level.min(8),
            chapter: index,
            fraction: 0.0,
        })
        .collect();
    let mut texts = Vec::with_capacity(chapters.len());
    let mut images = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        texts.push(chapter.text);
        images.push(chapter.images);
    }
    Some((texts, images, toc))
}

fn consolidate_color_pages(chapters: Vec<ChapterDraft>) -> Vec<ChapterDraft> {
    let mut result: Vec<ChapterDraft> = Vec::new();
    let mut color_slot: Option<usize> = None;

    for chapter in chapters {
        let is_color_page = chapter.title == "彩页"
            || chapter.title.starts_with("彩页 ")
            || (chapter.title != "封面"
                && !chapter.images.is_empty()
                && chapter.text.trim().is_empty());
        if !is_color_page {
            result.push(chapter);
            continue;
        }

        let slot = if let Some(slot) = color_slot {
            slot
        } else {
            result.push(ChapterDraft {
                title: "彩页".to_owned(),
                level: 0,
                text: String::new(),
                images: Vec::new(),
            });
            let slot = result.len() - 1;
            color_slot = Some(slot);
            slot
        };
        let target = &mut result[slot];
        append_draft(target, chapter);
    }
    result
}

fn split_section(
    text: &str,
    base_title: &str,
    level: usize,
    images: Vec<Vec<u8>>,
) -> Vec<ChapterDraft> {
    let mut text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let mut headings = strong_headings(text);
    if headings.len() == 1 {
        if let Some((start, _)) = headings.first() {
            if *start > 0 && is_toc_like(&text[..*start]) {
                text = text[*start..].trim_start();
                headings = strong_headings(text);
            }
        }
    }
    let skip = toc_heading_skip(&headings);
    if skip > 0 && skip < headings.len() {
        let start = headings[skip].0;
        text = text[start..].trim_start();
        headings = strong_headings(text);
    }

    let mut drafts = Vec::new();
    if headings.len() >= 2 {
        let first_start = headings.first().map_or(0, |(start, _)| *start);
        let mut leading = String::new();
        if first_start > 0 {
            let prefix = text[..first_start].trim();
            if is_toc_like(prefix) {
                // A table of contents at the top of the file is not story.
            } else if prefix.chars().count() >= 200 {
                drafts.push(ChapterDraft {
                    title: base_title.to_owned(),
                    level,
                    text: prefix.to_owned(),
                    images: Vec::new(),
                });
            } else {
                leading = prefix.to_owned();
            }
        }
        for (position, (start, heading)) in headings.iter().enumerate() {
            let end = headings
                .get(position + 1)
                .map_or(text.len(), |(next, _)| *next);
            let chunk = text[*start..end].trim();
            if chunk.is_empty() {
                continue;
            }
            let body = if position == 0 && !leading.is_empty() {
                format!("{leading}\n\n{chunk}")
            } else {
                chunk.to_owned()
            };
            drafts.push(ChapterDraft {
                title: heading.clone(),
                level,
                text: body,
                images: Vec::new(),
            });
        }
    } else {
        let title = headings
            .first()
            .map_or_else(|| base_title.to_owned(), |(_, heading)| heading.clone());
        drafts.push(ChapterDraft {
            title,
            level,
            text: text.to_owned(),
            images: Vec::new(),
        });
    }

    let mut result = Vec::new();
    for mut draft in drafts {
        let (text, chapter_images) = assign_images(&draft.text, &images);
        draft.text = text;
        draft.images = chapter_images;
        result.extend(split_oversized(draft));
    }
    result
}

fn strong_headings(text: &str) -> Vec<(usize, String)> {
    let mut headings = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if is_strong_heading(trimmed) {
            headings.push((
                offset,
                crate::parser::html_text::strip_style_markers(trimmed)
                    .trim()
                    .to_owned(),
            ));
        }
        offset += line.len();
    }
    headings
}

fn is_strong_heading(line: &str) -> bool {
    let cleaned = crate::parser::html_text::strip_style_markers(line);
    let trimmed = cleaned.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > 60 {
        return false;
    }

    if trimmed.starts_with('第') {
        let prefix: String = trimmed.chars().take(14).collect();
        if ['章', '节', '回', '卷', '部', '篇']
            .iter()
            .any(|marker| prefix.contains(*marker))
        {
            return true;
        }
    }

    let lower = trimmed.to_ascii_lowercase();
    if (lower.starts_with("chapter") || lower.starts_with("part") || lower.starts_with("book "))
        && trimmed.chars().any(|character| character.is_ascii_digit())
    {
        return true;
    }

    matches!(
        trimmed,
        "序章" | "楔子" | "引子" | "尾声" | "后记" | "番外" | "前言" | "后记："
    )
}

fn toc_heading_skip(headings: &[(usize, String)]) -> usize {
    if headings.len() < 3 {
        return 0;
    }
    let mut run = 0;
    for window in headings.windows(2) {
        let gap = window[1].0.saturating_sub(window[0].0);
        if gap < 300 {
            run += 1;
        } else {
            break;
        }
    }
    if run >= 2 { run } else { 0 }
}

fn is_toc_like(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() < 3 || lines.len() > 100 || text.chars().count() > 5000 {
        return false;
    }
    let short = lines
        .iter()
        .filter(|line| line.chars().count() <= 40)
        .count();
    let headings = lines.iter().filter(|line| is_strong_heading(line)).count();
    short * 100 >= lines.len() * 80 && headings >= 2
}

fn split_oversized(draft: ChapterDraft) -> Vec<ChapterDraft> {
    const MAX_CHARS: usize = 16_000;
    const TARGET_CHARS: usize = 9_000;
    if draft.text.chars().count() <= MAX_CHARS {
        return vec![draft];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < draft.text.len() {
        let remaining = &draft.text[start..];
        if remaining.chars().count() <= MAX_CHARS {
            let tail = remaining.trim();
            if !tail.is_empty() {
                chunks.push(tail.to_owned());
            }
            break;
        }

        let mut split_at = None;
        let mut count = 0;
        for (offset, character) in remaining.char_indices() {
            count += 1;
            if count >= TARGET_CHARS && character == '\n' {
                split_at = Some(start + offset + character.len_utf8());
                break;
            }
        }
        let end = split_at.unwrap_or_else(|| {
            let mut byte = start;
            for (index, (offset, _)) in remaining.char_indices().enumerate() {
                if index + 1 >= TARGET_CHARS {
                    byte = start + offset;
                    break;
                }
            }
            if byte == start {
                draft.text.len()
            } else {
                byte
            }
        });
        let chunk = draft.text[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_owned());
        }
        if end <= start {
            break;
        }
        start = end;
    }

    if chunks.len() <= 1 {
        return vec![draft];
    }
    let images = draft.images.clone();
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let (text, chunk_images) = assign_images(&text, &images);
            ChapterDraft {
                title: if index == 0 {
                    draft.title.clone()
                } else {
                    format!("{}（{}）", draft.title, index + 1)
                },
                level: draft.level,
                text,
                images: chunk_images,
            }
        })
        .collect()
}

fn merge_short_chapters(drafts: Vec<ChapterDraft>) -> Vec<ChapterDraft> {
    const MIN_CHARS: usize = 280;
    let mut chapters: Vec<ChapterDraft> = Vec::new();
    let mut pending: Option<ChapterDraft> = None;

    for mut draft in drafts {
        let length = draft.text.chars().count();
        if draft.text.trim().is_empty() {
            continue;
        }
        if length < MIN_CHARS {
            if let Some(previous) = chapters.last_mut() {
                append_draft(previous, draft);
            } else {
                pending = Some(draft);
            }
            continue;
        }
        if let Some(short) = pending.take() {
            let mut combined = short;
            append_draft(&mut combined, draft);
            draft = combined;
        }
        chapters.push(draft);
    }

    if let Some(short) = pending {
        if let Some(previous) = chapters.last_mut() {
            append_draft(previous, short);
        } else {
            chapters.push(short);
        }
    }
    chapters
}

fn is_story_title(title: &str) -> bool {
    let normalized: String = title
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if matches!(
        normalized.as_str(),
        "序章" | "楔子" | "引子" | "尾声" | "终章" | "后记" | "番外"
    ) {
        return true;
    }
    if normalized.starts_with('第')
        && ['章', '节', '回', '卷', '部', '篇']
            .iter()
            .any(|marker| normalized.contains(*marker))
    {
        return true;
    }
    (normalized.starts_with("chapter") || normalized.starts_with("part"))
        && normalized
            .chars()
            .any(|character| character.is_ascii_digit())
}

fn is_front_matter_title(title: &str) -> bool {
    let normalized: String = title
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "封面"
            | "目录"
            | "版权"
            | "版权页"
            | "书名页"
            | "扉页"
            | "内容简介"
            | "简介"
            | "作者"
            | "作者介绍"
            | "制作信息"
            | "出版信息"
            | "作者简介"
            | "人物介绍"
            | "登场人物"
            | "彩页"
            | "插图"
            | "关于本书"
            | "cover"
            | "contents"
            | "toc"
            | "copyright"
            | "titlepage"
    ) || normalized.starts_with("copyright")
        || normalized.starts_with("版权")
}

fn comic_toc(chapter_count: usize) -> Vec<TocEntry> {
    (0..chapter_count)
        .map(|index| TocEntry {
            title: format!("第 {} 页", index + 1),
            level: 0,
            chapter: index,
            fraction: 0.0,
        })
        .collect()
}

fn build_toc(book: &Book, chapter_count: usize) -> Vec<TocEntry> {
    let flat = NavPoint::flatten(book.toc());
    let mut entries = Vec::new();

    for item in flat {
        let target = normalize_href(&item.href);
        let target = if target.is_empty() {
            normalize_href(&item.full_path)
        } else {
            target
        };
        if target.is_empty() {
            continue;
        }

        let chapter = book
            .spine()
            .iter()
            .position(|spine| href_matches(&spine.href, &target))
            .or_else(|| {
                book.sections()
                    .iter()
                    .position(|section| href_matches(&section.href, &target))
            });

        if let Some(chapter) = chapter {
            entries.push(TocEntry {
                title: if item.label.trim().is_empty() {
                    format!("第 {} 节", chapter + 1)
                } else {
                    item.label.trim().to_owned()
                },
                level: item.depth.min(8),
                chapter,
                fraction: 0.0,
            });
        }
    }

    if entries.is_empty() {
        fallback_toc(chapter_count)
    } else {
        entries
    }
}

fn fallback_toc(chapter_count: usize) -> Vec<TocEntry> {
    (0..chapter_count)
        .map(|index| TocEntry {
            title: format!("第 {} 节", index + 1),
            level: 0,
            chapter: index,
            fraction: 0.0,
        })
        .collect()
}

fn normalize_href(href: &str) -> String {
    let without_fragment = href.split(['#', '?']).next().map_or("", |part| part);
    let decoded = urlencoding::decode(without_fragment)
        .ok()
        .map_or_else(|| without_fragment.to_owned(), |value| value.into_owned());
    decoded
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

fn href_matches(candidate: &str, target: &str) -> bool {
    let candidate = normalize_href(candidate);
    if candidate.is_empty() || target.is_empty() {
        return false;
    }
    candidate == target
        || candidate.ends_with(&format!("/{target}"))
        || target.ends_with(&format!("/{candidate}"))
}

fn split_text_chunks(text: &str, target_chars: usize) -> Vec<String> {
    let target = target_chars.max(4_000);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if current_chars > 0 && current_chars + line_chars > target {
            chunks.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push_str(line);
        current_chars += line_chars;
    }
    if !current.trim().is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{ChapterDraft, is_front_matter_title, merge_short_chapters, split_section};
    use crate::parser::book_source::BookSource;

    #[test]
    fn splits_a_single_file_at_strong_chapter_headings() {
        let text = "第一章 起点\n\n这是第一章的正文。\n\n第二章 远行\n\n这是第二章的正文。";
        let drafts = split_section(text, "第 1 节", 0, Vec::new());
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "第一章 起点");
        assert_eq!(drafts[1].title, "第二章 远行");
        assert!(drafts[0].text.contains("第一章的正文"));
        assert!(drafts[1].text.contains("第二章的正文"));
    }

    #[test]
    fn drops_a_table_of_contents_prefix_before_the_real_heading() {
        let text = "第六章 回想场面\n终章之一 或者五五章\n后记\n参考资料\n序章\n\n六月中旬，放学后的夕阳……";
        let drafts = split_section(text, "第 1 节", 0, Vec::new());
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "序章");
        assert!(!drafts[0].text.contains("第六章"));
        assert!(drafts[0].text.contains("六月中旬"));
    }

    #[test]
    fn short_scraps_merge_into_a_neighboring_chapter() {
        let chapters = merge_short_chapters(vec![
            ChapterDraft {
                title: "扉页".to_owned(),
                level: 0,
                text: "书名\n作者".to_owned(),
                images: Vec::new(),
            },
            ChapterDraft {
                title: "第一章".to_owned(),
                level: 0,
                text: "正文".repeat(500),
                images: Vec::new(),
            },
        ]);
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].text.contains("书名"));
    }

    #[test]
    fn skips_known_front_matter_titles() {
        assert!(is_front_matter_title("目 录"));
        assert!(is_front_matter_title("版权页"));
        assert!(is_front_matter_title("Cover"));
        assert!(!is_front_matter_title("第一章"));
    }

    #[test]
    fn normalizes_sample_epub_into_non_empty_chapters() {
        let path = std::path::Path::new("samples/sample.epub");
        if !path.is_file() {
            return;
        }
        let source = match super::EbookSource::open(path, crate::state::models::BookFormat::Epub) {
            Ok(source) => source,
            Err(error) => panic!("sample should open: {error}"),
        };
        assert!(source.chapter_count() >= 3);
        for index in 0..source.chapter_count() {
            match source.chapter_content(index) {
                Ok(crate::state::models::ChapterBody::Text(text)) => {
                    assert!(!text.trim().is_empty(), "chapter {index} is empty");
                }
                Ok(crate::state::models::ChapterBody::Rich { text, images }) => {
                    assert!(
                        !text.trim().is_empty() || !images.is_empty(),
                        "chapter {index} has neither text nor images"
                    );
                }
                Ok(other) => panic!("expected text chapter, got {other:?}"),
                Err(error) => panic!("chapter {index} failed: {error}"),
            }
        }
    }
}

fn chunk_title(chunk: &str, index: usize) -> String {
    let first = chunk
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(String::new, |line| line.to_owned());
    if first.is_empty() {
        format!("第 {} 节", index + 1)
    } else {
        format!(
            "{} · {}",
            index + 1,
            first.chars().take(26).collect::<String>()
        )
    }
}

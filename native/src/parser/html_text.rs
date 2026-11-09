//! Small HTML-to-readable-text converter.
//!
//! `ebook-rs` exposes `Section::plain_text`, but it intentionally replaces all
//! markup with spaces. That is useful for search, but it turns paragraphs into
//! one endless line in a reader. This converter keeps block boundaries and
//! `<br>` as line breaks, strips scripts/styles, and decodes common entities.
//! It also extracts base64-inlined illustrations for the reader.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use base64::Engine;

/// Convert section XHTML to text with paragraph breaks preserved.
pub const COLOR_START: char = '\u{E000}';
pub const COLOR_END: char = '\u{E001}';
/// Inline illustration marker. The payload is a decimal index into the
/// chapter's image vector. Keeping the marker directly in the text lets the
/// reader render images between paragraphs instead of dumping every image at
/// the top of the chapter.
pub const IMAGE_START: char = '\u{E002}';
pub const IMAGE_END: char = '\u{E003}';

pub fn to_paragraph_text(html: &str) -> String {
    let mut raw = String::with_capacity(html.len() / 2);
    let bytes = html.as_bytes();
    let mut index = 0usize;
    let mut color_stack: Vec<bool> = Vec::new();

    while index < html.len() {
        if !html.is_char_boundary(index) {
            index += 1;
            continue;
        }

        if bytes[index] == b'<' {
            if html[index..].starts_with("<!--") {
                index = html[index + 4..]
                    .find("-->")
                    .map_or(html.len(), |offset| index + 4 + offset + 3);
                continue;
            }

            let Some(tag_end) = find_tag_end(html, index) else {
                break;
            };
            let tag_body = &html[index + 1..tag_end];
            let trimmed = tag_body.trim();
            let closing = trimmed.starts_with('/');
            let name = tag_name(trimmed);
            let self_closing = trimmed.ends_with('/')
                || matches!(name.as_str(), "br" | "hr" | "img" | "meta" | "link");
            let color = if !closing && !self_closing {
                tag_color(trimmed)
            } else {
                None
            };

            if !closing && matches!(name.as_str(), "script" | "style" | "head" | "title" | "svg") {
                index = find_close_tag(html, &name, tag_end + 1).map_or(html.len(), |end| end);
                continue;
            }

            if !closing || !self_closing {
                match name.as_str() {
                    "p" | "div" | "section" | "article" | "header" | "footer" | "blockquote"
                    | "pre" | "ul" | "ol" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        block_break(&mut raw);
                    }
                    "br" => line_break(&mut raw),
                    "li" => {
                        line_break(&mut raw);
                        if !closing {
                            raw.push_str("• ");
                        }
                    }
                    "tr" => line_break(&mut raw),
                    "td" | "th" => raw.push(' '),
                    "hr" => block_break(&mut raw),
                    _ => {}
                }
            }

            if !closing && !self_closing {
                let has_color = color.is_some();
                if let Some(value) = color {
                    raw.push(COLOR_START);
                    raw.push_str(&value);
                }
                color_stack.push(has_color);
            } else if closing {
                if color_stack.pop().unwrap_or(false) {
                    raw.push(COLOR_END);
                }
            }

            index = tag_end + 1;
            continue;
        }

        let Some(character) = html[index..].chars().next() else {
            break;
        };
        raw.push(character);
        index += character.len_utf8();
    }

    for active in color_stack.into_iter().rev() {
        if active {
            raw.push(COLOR_END);
        }
    }

    normalize_text(&decode_entities(&raw))
}

/// Convert XHTML to text while replacing each inline image with a positional
/// marker. `images` is ordered exactly like the markers in the returned text.
pub fn to_paragraph_text_with_images(html: &str) -> (String, Vec<Vec<u8>>) {
    let mut modified = String::with_capacity(html.len());
    let mut images = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = html[cursor..].find('<') {
        let start = cursor + relative;
        modified.push_str(&html[cursor..start]);
        let Some(tag_end) = find_tag_end(html, start) else {
            modified.push_str(&html[start..]);
            cursor = html.len();
            break;
        };
        let body = &html[start + 1..tag_end];
        let trimmed = body.trim();
        let name = tag_name(trimmed);
        if !trimmed.starts_with('/') && matches!(name.as_str(), "img" | "image") {
            if let Some(bytes) = image_source(trimmed).and_then(|source| decode_data_image(&source)) {
                let index = images.len();
                images.push(bytes);
                modified.push(IMAGE_START);
                modified.push_str(&index.to_string());
                modified.push(IMAGE_END);
            }
            cursor = tag_end + 1;
            continue;
        }
        modified.push_str(&html[start..=tag_end]);
        cursor = tag_end + 1;
    }
    modified.push_str(&html[cursor..]);
    (to_paragraph_text(&modified), images)
}

pub fn strip_image_markers(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_marker = false;
    for character in text.chars() {
        if character == IMAGE_START {
            in_marker = true;
        } else if character == IMAGE_END {
            in_marker = false;
        } else if !in_marker {
            output.push(character);
        }
    }
    output
}

pub fn strip_style_markers(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == COLOR_START && index + 7 <= chars.len() {
            index += 7;
        } else if chars[index] == COLOR_END {
            index += 1;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

/// Extract base64-inlined illustrations from a chapter. `ebook-rs` inlines
/// resources by default, so most EPUB/MOBI images arrive as `data:` URIs.
pub fn extract_images(html: &str) -> Vec<Vec<u8>> {
    let mut images = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0usize;

    while index < html.len() {
        if !html.is_char_boundary(index) {
            index += 1;
            continue;
        }
        if html.as_bytes()[index] != b'<' {
            index += html[index..]
                .chars()
                .next()
                .map_or(1, |character| character.len_utf8());
            continue;
        }
        let Some(tag_end) = find_tag_end(html, index) else {
            break;
        };
        let body = &html[index + 1..tag_end];
        let trimmed = body.trim();
        let name = tag_name(trimmed);
        if !trimmed.starts_with('/') && matches!(name.as_str(), "img" | "image") {
            if let Some(source) = image_source(trimmed) {
                if let Some(bytes) = decode_data_image(&source) {
                    let mut hasher = DefaultHasher::new();
                    bytes.hash(&mut hasher);
                    if seen.insert(hasher.finish()) {
                        images.push(bytes);
                    }
                }
            }
        }
        index = tag_end + 1;
    }

    // CSS backgrounds and SVG resources are often not `<img>` tags. Scan all
    // data URIs as a fallback so colored/fixed-layout pages are not lost.
    let mut cursor = 0usize;
    while let Some(position) = html[cursor..].find("data:image/") {
        let start = cursor + position;
        let rest = &html[start..];
        let end = rest
            .find(|character: char| {
                matches!(character, '"' | '\'' | ')' | '>' | ' ' | '\n' | '\r' | '\t')
            })
            .unwrap_or(rest.len());
        if end > 0 {
            if let Some(bytes) = decode_data_image(&rest[..end]) {
                let mut hasher = DefaultHasher::new();
                bytes.hash(&mut hasher);
                if seen.insert(hasher.finish()) {
                    images.push(bytes);
                }
            }
        }
        cursor = start + end.max(1);
    }
    images
}

fn image_source(tag_body: &str) -> Option<String> {
    let lower = tag_body.to_ascii_lowercase();
    for name in ["xlink:href", "src", "href"] {
        let needle = format!("{name}=");
        let Some(position) = lower.find(&needle) else {
            continue;
        };
        let mut value = tag_body[position + needle.len()..].trim_start();
        let quote = value.chars().next()?;
        if quote == '"' || quote == '\'' {
            value = &value[quote.len_utf8()..];
            let end = value.find(quote)?;
            return Some(value[..end].trim().to_owned());
        }
        let end = value
            .find(|character: char| character.is_whitespace() || character == '>')
            .unwrap_or(value.len());
        return Some(value[..end].trim().to_owned());
    }
    None
}

fn decode_data_image(source: &str) -> Option<Vec<u8>> {
    let source = source.trim();
    let marker = ";base64,";
    let data = source.strip_prefix("data:image/")?;
    let position = data.find(marker)?;
    let payload: String = data[position + marker.len()..]
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()
}

fn tag_color(tag_body: &str) -> Option<String> {
    if let Some(style) = attribute_value(tag_body, "style") {
        for declaration in style.split(';') {
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("color") {
                if let Some(color) = parse_css_color(value) {
                    return Some(color);
                }
            }
        }
    }
    attribute_value(tag_body, "color").and_then(|value| parse_css_color(&value))
}

fn attribute_value(tag_body: &str, name: &str) -> Option<String> {
    let lower = tag_body.to_ascii_lowercase();
    let needle = format!("{name}=");
    let position = lower.find(&needle)?;
    let mut value = tag_body[position + needle.len()..].trim_start();
    let quote = value.chars().next()?;
    if quote == '"' || quote == '\'' {
        value = &value[quote.len_utf8()..];
        let end = value.find(quote)?;
        Some(value[..end].trim().to_owned())
    } else {
        let end = value
            .find(|character: char| character.is_whitespace() || character == '>')
            .unwrap_or(value.len());
        Some(value[..end].trim().to_owned())
    }
}

fn parse_css_color(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if let Some(hex) = value.strip_prefix('#') {
        let hex = hex.trim();
        if hex.len() == 3 {
            let mut expanded = String::with_capacity(6);
            for character in hex.chars() {
                expanded.push(character);
                expanded.push(character);
            }
            return Some(expanded.to_ascii_uppercase());
        }
        if hex.len() == 6 && hex.chars().all(|character| character.is_ascii_hexdigit()) {
            return Some(hex.to_ascii_uppercase());
        }
        return None;
    }

    let lower = value.to_ascii_lowercase();
    if let Some(inner) = lower
        .strip_prefix("rgb(")
        .or_else(|| lower.strip_prefix("rgba("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() >= 3 {
            let red = parts[0].parse::<u8>().ok()?;
            let green = parts[1].parse::<u8>().ok()?;
            let blue = parts[2].parse::<u8>().ok()?;
            return Some(format!("{red:02X}{green:02X}{blue:02X}"));
        }
    }

    let named = match lower.as_str() {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "orange" => "FFA500",
        "purple" => "800080",
        "pink" => "FFC0CB",
        "gray" | "grey" => "808080",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "brown" => "A52A2A",
        _ => return None,
    };
    Some(named.to_owned())
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn find_close_tag(html: &str, name: &str, from: usize) -> Option<usize> {
    let needle = format!("</{name}");
    let needle_bytes = needle.as_bytes();
    let bytes = html.as_bytes();
    let mut index = from;
    while index + needle_bytes.len() <= bytes.len() {
        if bytes[index..index + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes) {
            if let Some(end) = find_tag_end(html, index) {
                return Some(end + 1);
            }
            return None;
        }
        index += 1;
    }
    None
}

fn tag_name(tag_body: &str) -> String {
    let trimmed = tag_body.trim_start_matches('/').trim_start();
    let mut name = String::new();
    for character in trimmed.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == ':' {
            name.push(character.to_ascii_lowercase());
        } else {
            break;
        }
    }
    name
}

fn block_break(output: &mut String) {
    trim_end_spaces(output);
    if output.is_empty() {
        return;
    }
    let trailing = output
        .chars()
        .rev()
        .take_while(|character| *character == '\n')
        .count();
    for _ in trailing..2 {
        output.push('\n');
    }
}

fn line_break(output: &mut String) {
    trim_end_spaces(output);
    if output.is_empty() {
        return;
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn trim_end_spaces(output: &mut String) {
    while output.ends_with(' ') || output.ends_with('\t') || output.ends_with('\r') {
        output.pop();
    }
}

fn normalize_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut pending_spaces = 0usize;
    let mut pending_newlines = 0usize;

    for character in input.chars() {
        if character == '\r' {
            continue;
        }
        if character == '\n' {
            pending_newlines = pending_newlines.saturating_add(1);
            pending_spaces = 0;
            continue;
        }
        if character.is_whitespace() {
            if pending_newlines == 0 {
                pending_spaces = 1;
            }
            continue;
        }

        if !output.is_empty() {
            if pending_newlines > 0 {
                let count = if pending_newlines >= 2 { 2 } else { 1 };
                for _ in 0..count {
                    output.push('\n');
                }
            } else if pending_spaces > 0 {
                output.push(' ');
            }
        }
        pending_spaces = 0;
        pending_newlines = 0;
        output.push(character);
    }

    output
}

fn decode_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        let candidate = &rest[position..];
        let Some(end) = candidate.find(';') else {
            output.push_str(candidate);
            return output;
        };
        if end > 32 {
            output.push('&');
            rest = &candidate[1..];
            continue;
        }

        let entity = &candidate[1..end];
        if let Some(character) = decode_entity(entity) {
            output.push(character);
            rest = &candidate[end + 1..];
        } else {
            output.push('&');
            rest = &candidate[1..];
        }
    }

    output.push_str(rest);
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(decimal) = entity.strip_prefix('#') {
        return decimal.parse::<u32>().ok().and_then(char::from_u32);
    }

    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "hellip" => Some('…'),
        "middot" => Some('·'),
        "bull" => Some('•'),
        "copy" => Some('©'),
        "reg" => Some('®'),
        "deg" => Some('°'),
        "times" => Some('×'),
        "divide" => Some('÷'),
        "laquo" => Some('«'),
        "raquo" => Some('»'),
        "ldquo" => Some('“'),
        "rdquo" => Some('”'),
        "lsquo" => Some('‘'),
        "rsquo" => Some('’'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::to_paragraph_text;

    #[test]
    fn keeps_paragraphs_and_headings() {
        let html = "<html><head><title>隐藏</title><style>p{color:red}</style></head>\
                    <body><h1>第一章</h1><p>第一段</p><p>第二段<br/>续行</p></body></html>";
        assert_eq!(to_paragraph_text(html), "第一章\n\n第一段\n\n第二段\n续行");
    }

    #[test]
    fn decodes_entities_and_ignores_scripts() {
        let html = "<p>A&nbsp;&amp;&nbsp;B &#20320;&#x597d;</p><script>alert('x')</script>";
        assert_eq!(to_paragraph_text(html), "A & B 你好");
    }

    #[test]
    fn plain_text_without_tags_is_preserved() {
        assert_eq!(to_paragraph_text("第一行\n第二行"), "第一行\n第二行");
    }

    #[test]
    fn preserves_inline_colors_as_markers() {
        let html = "<p><span style=\"color:#f00\">红色</span>黑色</p>";
        let text = to_paragraph_text(html);
        assert!(text.contains(super::COLOR_START));
        assert!(text.contains("FF0000"));
        assert_eq!(super::strip_style_markers(&text), "红色黑色");
    }

    #[test]
    fn extracts_and_deduplicates_inline_images() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl2nS8AAAAASUVORK5CYII=";
        let html = format!(
            "<p>插图</p><img src=\"data:image/png;base64,{png}\"/>\
             <p>重复</p><img src='data:image/png;base64,{png}'/>"
        );
        let images = super::extract_images(&html);
        assert_eq!(images.len(), 1);
        assert!(images[0].starts_with(b"\x89PNG"));
    }
}

use std::cmp::Ordering;

#[derive(Clone, Debug)]
struct SeriesParts {
    base: String,
    volume: i64,
}

/// Human-friendly title sorting that keeps multi-volume works together:
///
/// - `上 / 中 / 下` and `前 / 后 / 续 / 终` are treated as volume markers.
/// - `第1部 / 第2部`, `1 / 2 / 10`, `第一卷 / 第二卷` sort numerically.
/// - Titles with the same normalized base are ordered by volume before the
///   rest of the title.
pub fn smart_title_cmp(left: &str, right: &str) -> Ordering {
    let left_parts = parse_series(left);
    let right_parts = parse_series(right);

    natural_cmp(&left_parts.base, &right_parts.base)
        .then_with(|| left_parts.volume.cmp(&right_parts.volume))
        .then_with(|| natural_cmp(left, right))
}

fn parse_series(title: &str) -> SeriesParts {
    let trimmed = title
        .trim()
        .trim_end_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '）' | ')' | ']' | '】' | '》' | '。' | '.' | '!' | '！'
                )
        })
        .to_owned();

    let mut core = trimmed.as_str();
    let mut had_unit_suffix = false;
    while let Some(last) = core.chars().last() {
        if matches!(last, '册' | '部' | '卷' | '集') {
            core = trim_last_char(core);
            had_unit_suffix = true;
        } else {
            break;
        }
    }

    let mut base = core;
    let mut volume = 0_i64;

    if let Some(last) = core.chars().last() {
        if let Some(marker) = marker_volume(last) {
            volume = marker;
            base = trim_last_char(core);
        }
    }

    if volume == 0 {
        let (before, digits) = split_trailing_digits(core);
        if !digits.is_empty() && !before.trim().is_empty() {
            if let Ok(number) = digits.parse::<i64>() {
                volume = number;
                base = strip_volume_prefix(before);
            }
        }
    }

    if volume == 0 {
        if let Some((before, number)) = split_trailing_chinese_number(core, had_unit_suffix) {
            volume = number;
            base = strip_volume_prefix(before);
        }
    }

    base = base
        .trim()
        .trim_end_matches(|character: char| {
            matches!(character, '（' | '(' | '[' | '【' | '-' | '_' | '·')
        })
        .trim();

    SeriesParts {
        base: normalize_base(base),
        volume,
    }
}

fn marker_volume(character: char) -> Option<i64> {
    match character {
        '上' | '前' => Some(1),
        '中' => Some(2),
        '下' | '后' => Some(3),
        '续' => Some(4),
        '终' | '完' => Some(5),
        _ => None,
    }
}

fn split_trailing_digits(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    let mut start = bytes.len();
    while start > 0 && bytes[start - 1].is_ascii_digit() {
        start -= 1;
    }
    (&text[..start], &text[start..])
}

fn split_trailing_chinese_number(text: &str, had_unit_suffix: bool) -> Option<(&str, i64)> {
    const NUMERALS: &str = "零〇一二三四五六七八九十百两";
    let mut start = text.len();
    for (index, character) in text.char_indices().rev() {
        if NUMERALS.contains(character) {
            start = index;
        } else {
            break;
        }
    }
    if start == text.len() {
        return None;
    }

    let before = &text[..start];
    let marked = had_unit_suffix || before.ends_with('第') || before.ends_with('之');
    if !marked {
        return None;
    }
    let number = chinese_number(&text[start..])?;
    let before = if before.ends_with('第') || before.ends_with('之') {
        trim_last_char(before)
    } else {
        before
    };
    Some((before, number))
}

fn chinese_number(text: &str) -> Option<i64> {
    let mut total = 0_i64;
    let mut current = 0_i64;
    for character in text.chars() {
        match character {
            '零' | '〇' => {}
            '一' => current = 1,
            '二' | '两' => current = 2,
            '三' => current = 3,
            '四' => current = 4,
            '五' => current = 5,
            '六' => current = 6,
            '七' => current = 7,
            '八' => current = 8,
            '九' => current = 9,
            '十' => {
                total += current.max(1) * 10;
                current = 0;
            }
            '百' => {
                total += current.max(1) * 100;
                current = 0;
            }
            _ => return None,
        }
    }
    total += current;
    (total > 0).then_some(total)
}

fn strip_volume_prefix(text: &str) -> &str {
    let trimmed = text.trim_end();
    if trimmed.ends_with('第') || trimmed.ends_with('之') {
        trim_last_char(trimmed).trim_end()
    } else {
        trimmed
    }
}

fn trim_last_char(text: &str) -> &str {
    let mut chars = text.char_indices();
    let _ = chars.next_back();
    chars.as_str()
}

fn normalize_base(text: &str) -> String {
    text.chars()
        .filter(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '-' | '_'
                        | '/'
                        | '\\'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                        | '《'
                        | '》'
                        | '·'
                        | '.'
                        | ','
                        | '，'
                        | '。'
                        | ':'
                        | '：'
                )
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    for (left_token, right_token) in left_tokens.iter().zip(right_tokens.iter()) {
        let ordering = match (left_token, right_token) {
            (NaturalToken::Number(left_number), NaturalToken::Number(right_number)) => {
                left_number.cmp(right_number)
            }
            (NaturalToken::Number(_), NaturalToken::Text(_)) => Ordering::Less,
            (NaturalToken::Text(_), NaturalToken::Number(_)) => Ordering::Greater,
            (NaturalToken::Text(left_text), NaturalToken::Text(right_text)) => {
                left_text.cmp(right_text)
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left_tokens.len().cmp(&right_tokens.len())
}

#[derive(Debug, PartialEq, Eq)]
enum NaturalToken {
    Number(u64),
    Text(String),
}

fn tokenize(text: &str) -> Vec<NaturalToken> {
    let mut tokens = Vec::new();
    let mut number = String::new();
    let mut text_run = String::new();

    let flush_number = |tokens: &mut Vec<NaturalToken>, number: &mut String| {
        if !number.is_empty() {
            if let Ok(value) = number.parse::<u64>() {
                tokens.push(NaturalToken::Number(value));
            }
            number.clear();
        }
    };
    let flush_text = |tokens: &mut Vec<NaturalToken>, text_run: &mut String| {
        if !text_run.is_empty() {
            tokens.push(NaturalToken::Text(std::mem::take(text_run)));
        }
    };

    for character in text.chars() {
        if character.is_ascii_digit() {
            flush_text(&mut tokens, &mut text_run);
            number.push(character);
        } else {
            flush_number(&mut tokens, &mut number);
            for lower in character.to_lowercase() {
                text_run.push(lower);
            }
        }
    }
    flush_number(&mut tokens, &mut number);
    flush_text(&mut tokens, &mut text_run);
    tokens
}

#[cfg(test)]
mod tests {
    use super::smart_title_cmp;

    fn sorted(titles: &[&str]) -> Vec<String> {
        let mut values: Vec<String> = titles.iter().map(|title| (*title).to_owned()).collect();
        values.sort_by(|left, right| smart_title_cmp(left, right));
        values
    }

    #[test]
    fn orders_volume_markers() {
        assert_eq!(
            sorted(&["星河 下", "星河 上", "星河 中"]),
            vec!["星河 上", "星河 中", "星河 下"]
        );
    }

    #[test]
    fn orders_numbers_numerically() {
        assert_eq!(
            sorted(&["星河 10", "星河 2", "星河 1"]),
            vec!["星河 1", "星河 2", "星河 10"]
        );
    }

    #[test]
    fn orders_chinese_volume_suffixes() {
        assert_eq!(
            sorted(&["长夜 第二部", "长夜 第一部", "长夜 第三部"]),
            vec!["长夜 第一部", "长夜 第二部", "长夜 第三部"]
        );
    }

    #[test]
    fn orders_chinese_prefix_and_unit_suffix() {
        assert_eq!(
            sorted(&["山海 第三卷", "山海 第一卷", "山海 第二卷"]),
            vec!["山海 第一卷", "山海 第二卷", "山海 第三卷"]
        );
    }
}

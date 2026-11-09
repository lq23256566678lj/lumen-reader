use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Rect, Response, RichText, Sense, Stroke, Ui, Vec2,
};

use crate::state::models::BookFormat;
use crate::ui::theme::Palette;

pub fn centered<R>(ui: &mut Ui, max_width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let available = ui.available_width();
    let width = available.min(max_width).max(0.0);
    let margin = ((available - width) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            add_contents(ui)
        })
        .inner
    })
    .inner
}

pub fn cover_placeholder(
    ui: &mut Ui,
    title: &str,
    format: BookFormat,
    size: Vec2,
    palette: Palette,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_cover_placeholder(ui, rect, title, format, palette);
    response
}

/// Paints the generated cover inside an already allocated rectangle. This is
/// also used when a real cover needs a letterboxed empty background.
pub fn paint_cover_placeholder(
    ui: &Ui,
    rect: Rect,
    title: &str,
    format: BookFormat,
    palette: Palette,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), palette.card_hover);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );

    let initial: String = title
        .chars()
        .find(|character| !character.is_whitespace())
        .map_or_else(|| "书".to_owned(), |character| character.to_string());
    painter.text(
        rect.center() - Vec2::new(0.0, 18.0),
        Align2::CENTER_CENTER,
        initial,
        FontId::proportional(42.0),
        format.color(),
    );
    painter.text(
        rect.center() + Vec2::new(0.0, 30.0),
        Align2::CENTER_CENTER,
        format.label(),
        FontId::proportional(13.0),
        palette.muted,
    );
}

pub fn pill(ui: &mut Ui, text: &str, color: Color32, _palette: Palette) -> Response {
    Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        })
        .response
        .on_hover_text(text)
}

pub fn stat_card(ui: &mut Ui, value: &str, label: &str, palette: Palette) {
    Frame::new()
        .fill(palette.card)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).size(21.0).strong().color(palette.text));
                ui.label(RichText::new(label).size(11.5).color(palette.muted));
            });
        });
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn relative_time(timestamp: u64) -> String {
    let now = crate::state::models::now_secs();
    if timestamp == 0 {
        return "从未".to_owned();
    }
    let seconds = now.saturating_sub(timestamp);
    if seconds < 60 {
        "刚刚".to_owned()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else if seconds < 86_400 * 30 {
        format!("{} 天前", seconds / 86_400)
    } else {
        format!("{} 个月前", seconds / (86_400 * 30))
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, character) in text.chars().enumerate() {
        if index >= max_chars {
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}

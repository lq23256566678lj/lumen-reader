use egui::{Color32, Stroke, Visuals};

use crate::state::models::ThemeMode;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub input: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub border: Color32,
    pub paper: Color32,
    pub paper_text: Color32,
    pub success: Color32,
    pub danger: Color32,
}

pub fn palette(mode: ThemeMode) -> Palette {
    match mode {
        ThemeMode::Dark => Palette {
            bg: Color32::from_rgb(17, 19, 22),
            panel: Color32::from_rgb(23, 26, 30),
            card: Color32::from_rgb(30, 34, 40),
            card_hover: Color32::from_rgb(38, 43, 50),
            input: Color32::from_rgb(27, 30, 35),
            text: Color32::from_rgb(232, 229, 224),
            muted: Color32::from_rgb(151, 158, 167),
            accent: Color32::from_rgb(218, 171, 94),
            accent_soft: Color32::from_rgb(74, 58, 35),
            border: Color32::from_rgb(45, 50, 58),
            paper: Color32::from_rgb(29, 32, 37),
            paper_text: Color32::from_rgb(235, 230, 219),
            success: Color32::from_rgb(126, 184, 138),
            danger: Color32::from_rgb(218, 119, 119),
        },
        ThemeMode::Light => Palette {
            bg: Color32::from_rgb(243, 240, 233),
            panel: Color32::from_rgb(251, 249, 244),
            card: Color32::from_rgb(255, 255, 255),
            card_hover: Color32::from_rgb(246, 241, 231),
            input: Color32::from_rgb(255, 253, 248),
            text: Color32::from_rgb(37, 34, 29),
            muted: Color32::from_rgb(119, 109, 95),
            accent: Color32::from_rgb(151, 101, 39),
            accent_soft: Color32::from_rgb(239, 220, 191),
            border: Color32::from_rgb(222, 213, 199),
            paper: Color32::from_rgb(255, 253, 247),
            paper_text: Color32::from_rgb(44, 40, 34),
            success: Color32::from_rgb(60, 132, 78),
            danger: Color32::from_rgb(177, 67, 67),
        },
    }
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    // modern-egui installs the shared typography/layout and semantic palettes.
    // We then apply the reader's paper-like colors on top of it.
    modern_egui::theme::apply(ctx);
    let palette = palette(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.panel;
    visuals.extreme_bg_color = palette.input;
    visuals.faint_bg_color = palette.card;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.noninteractive.bg_fill = palette.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.bg_fill = palette.card;
    visuals.widgets.inactive.weak_bg_fill = palette.card;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.hovered.bg_fill = palette.card_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.card_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent.gamma_multiply(0.7));
    visuals.widgets.active.bg_fill = palette.accent_soft;
    visuals.widgets.active.weak_bg_fill = palette.accent_soft;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        #[cfg(debug_assertions)]
        {
            style.debug.show_unaligned = false;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = style;
        }
    });
}

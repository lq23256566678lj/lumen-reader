use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};

use crate::error::AppError;

#[derive(Clone, Debug)]
pub struct FontInfo {
    pub path: PathBuf,
    pub family: String,
}

/// Loads system fonts at runtime. No font bytes are embedded in the binary.
pub fn install(ctx: &egui::Context) -> Result<FontInfo, AppError> {
    let mut paths = candidate_fonts();
    if paths.is_empty() {
        paths = scan_common_font_directories();
    }
    if paths.is_empty() {
        return Err(AppError::NoSystemFont);
    }

    let mut definitions = FontDefinitions::default();
    let mut loaded: Vec<(String, PathBuf)> = Vec::new();

    for path in paths {
        if loaded.len() >= 4 {
            break;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) if bytes.len() > 1024 => bytes,
            _ => continue,
        };
        let name = format!("system-font-{}", loaded.len());
        definitions
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        loaded.push((name, path));
    }

    if loaded.is_empty() {
        return Err(AppError::NoSystemFont);
    }

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let names = definitions.families.entry(family).or_default();
        names.clear();
        for (name, _) in &loaded {
            names.push(name.clone());
        }
    }

    ctx.set_fonts(definitions);

    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.5));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(24.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(14.0));
        style.spacing.item_spacing = egui::vec2(9.0, 9.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        // egui enables this debug overlay in debug builds; it paints orange
        // "Unaligned" guides over otherwise fine layouts.
        #[cfg(debug_assertions)]
        {
            style.debug.show_unaligned = false;
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = style;
        }
    });

    let (family, path) = loaded.first().map_or_else(
        || ("system".to_owned(), PathBuf::new()),
        |(name, path)| (name.clone(), path.clone()),
    );
    Ok(FontInfo { path, family })
}

fn candidate_fonts() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let windows_dir =
            std::env::var("WINDIR").map_or_else(|_| "C:\\Windows".to_owned(), |dir| dir);
        let fonts = Path::new(&windows_dir).join("Fonts");
        for name in [
            "msyh.ttc",
            "msyhbd.ttc",
            "simhei.ttf",
            "simsun.ttc",
            "segoeui.ttf",
            "arial.ttf",
            "consola.ttf",
        ] {
            paths.push(fonts.join(name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/Menlo.ttc",
        ] {
            paths.push(PathBuf::from(path));
        }
    }

    #[cfg(target_os = "linux")]
    {
        for path in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
        ] {
            paths.push(PathBuf::from(path));
        }
    }

    paths.into_iter().filter(|path| path.is_file()).collect()
}

fn scan_common_font_directories() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(windows_dir) = std::env::var("WINDIR") {
            roots.push(Path::new(&windows_dir).join("Fonts"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/System/Library/Fonts"));
        roots.push(PathBuf::from("/Library/Fonts"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local/share/fonts"));
        roots.push(home.join("Library/Fonts"));
    }
    #[cfg(target_os = "linux")]
    {
        roots.push(PathBuf::from("/usr/share/fonts"));
        roots.push(PathBuf::from("/usr/local/share/fonts"));
    }

    let mut files = Vec::new();
    for root in roots {
        collect_fonts(&root, &mut files, 0);
        if files.len() >= 8 {
            break;
        }
    }
    files.sort();
    files
}

fn collect_fonts(directory: &Path, files: &mut Vec<PathBuf>, depth: usize) {
    if depth > 3 || files.len() >= 8 {
        return;
    }
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, files, depth + 1);
        } else if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            if matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc"
            ) {
                files.push(path);
                if files.len() >= 8 {
                    return;
                }
            }
        }
    }
}

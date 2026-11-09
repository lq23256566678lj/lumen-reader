#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![allow(
    clippy::unnecessary_option_map_or_else,
    clippy::unnecessary_result_map_or_else,
    clippy::map_or_identity
)]

use eframe::egui;
use lumen_reader::app::LumenApp;

fn main() -> eframe::Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lumen Reader · 微光阅读")
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Lumen Reader",
        options,
        Box::new(|cc| {
            let app = LumenApp::new(cc)
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })?;
            Ok(Box::new(app))
        }),
    )
}

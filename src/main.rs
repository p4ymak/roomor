#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;
mod chat;
mod emoji;
use app::Roomor;
use eframe::egui;
#[cfg(test)]
mod tests;

fn main() -> Result<(), eframe::Error> {
    #[cfg(debug_assertions)]
    #[cfg(not(target_os = "android"))]
    {
        std::env::set_var("RUST_BACKTRACE", "1");
        std::env::set_var("RUST_LOG", "roomor");
        env_logger::init();
    }

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../icon/128x128.png"))
        .unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_taskbar(true)
            .with_app_id("roomor")
            .with_decorations(true)
            .with_transparent(false)
            .with_resizable(true)
            .with_maximized(false)
            .with_drag_and_drop(true)
            .with_inner_size([340.0, 624.0])
            .with_min_inner_size([280.0, 280.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Roomor",
        options,
        Box::new(|cc| Ok(Box::new(Roomor::new(cc)))),
    )
}

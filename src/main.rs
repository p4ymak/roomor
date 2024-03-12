#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;
mod chat;
use app::Roomor;
#[cfg(not(target_arch = "wasm32"))]
use eframe::egui;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
    #[cfg(debug_assertions)]
    {
        std::env::set_var("RUST_BACKTRACE", "1");
        std::env::set_var("RUST_LOG", "roomor");
        env_logger::init();
    }

    let icon = egui::viewport::IconData {
        rgba: image::load_from_memory(include_bytes!("../icon/128x128.png"))
            .expect("Icon exists")
            .to_rgba8()
            .to_vec(),
        width: 128,
        height: 128,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_decorations(true)
            .with_transparent(false)
            .with_resizable(true)
            .with_maximized(false)
            .with_drag_and_drop(true)
            .with_inner_size([340.0, 600.0])
            .with_min_inner_size([280.0, 280.0]),
        ..Default::default()
    };

    eframe::run_native("Roomor", options, Box::new(|cc| Box::new(Roomor::new(cc))))
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "the_canvas_id", // hardcode it
                web_options,
                Box::new(|cc| Box::new(Roomor::new(cc))),
            )
            .await
            .expect("failed to start");
    });
}

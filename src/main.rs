mod app;
mod chat;
use app::ChatApp;
use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_transparent(false)
            .with_resizable(true)
            .with_maximized(false)
            .with_drag_and_drop(true)
            .with_inner_size([300.0, 600.0])
            .with_min_inner_size([280.0, 280.0])
            .with_always_on_top(),
        ..Default::default()
    };

    eframe::run_native("Roomor", options, Box::new(|cc| Box::new(ChatApp::new(cc))))
}

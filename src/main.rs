mod app;
mod chat;
use app::Roomor;
use eframe::egui;

pub const HOMEPAGE_LINK: &str = "https://www.p4ymak.su";
pub const SOURCE_LINK: &str = "https://www.github.com/p4ymak/roomor";
pub const DONATION_LINK: &str = "https://www.donationalerts.com/r/p4ymak";

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
            .with_taskbar(true)
            .with_app_id("roomor")
            .with_decorations(true)
            .with_transparent(false)
            .with_resizable(true)
            .with_maximized(false)
            .with_drag_and_drop(true)
            .with_inner_size([340.0, 620.0])
            .with_min_inner_size([280.0, 280.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Roomor",
        options,
        Box::new(|cc| Ok(Box::new(Roomor::new(cc)))),
    )
}

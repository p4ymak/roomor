mod app;
mod chat;
mod emoji;

#[cfg(target_os = "android")]
use egui_winit::winit;

pub const HOMEPAGE_LINK: &str = "https://www.p4ymak.su";
pub const SOURCE_LINK: &str = "https://www.github.com/p4ymak/roomor";
pub const DONATION_LINK: &str = "https://www.donationalerts.com/r/p4ymak";

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use eframe::Renderer;
    use winit::platform::android::EventLoopBuilderExtAndroid;

    std::env::set_var("RUST_BACKTRACE", "full");
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_android_app(app);
        })),
        renderer: Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Roomor",
        options,
        Box::new(|cc| Ok(Box::new(app::Roomor::new(cc)))),
    );
}

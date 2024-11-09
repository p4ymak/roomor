mod app;
mod chat;
mod emoji;

#[cfg(target_os = "android")]
pub use egui_winit::winit::{
    self,
    platform::android::{activity::AndroidApp, EventLoopBuilderExtAndroid},
};

pub const HOMEPAGE_LINK: &str = "https://www.p4ymak.su";
pub const SOURCE_LINK: &str = "https://www.github.com/p4ymak/roomor";
pub const DONATION_LINK: &str = "https://www.donationalerts.com/r/p4ymak";

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    std::env::set_var("RUST_BACKTRACE", "full");
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    let android_app = app.clone();

    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_android_app(app);
        })),
        ..Default::default()
    };

    eframe::run_native(
        "Roomor",
        options,
        Box::new(|cc| Ok(Box::new(app::Roomor::new_android(cc, android_app)))),
    )
    .ok();
}

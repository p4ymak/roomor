use eframe::egui::Context;
#[cfg(not(target_arch = "wasm32"))]
use notify_rust::Notification;
use rodio::{source::SineWave, OutputStreamHandle, Source};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub trait Repaintable
where
    Self: Clone + Sync + Send + 'static,
{
    fn request_repaint(&self) {}
    fn notify(&self, _text: &str) {}
}

#[derive(Clone)]
pub struct Notifier {
    ctx: Context,
    audio: Option<OutputStreamHandle>,
    play_audio: Arc<AtomicBool>,
    d_bus: Arc<AtomicBool>,
}
impl Notifier {
    pub fn new(
        ctx: &Context,
        audio: Option<OutputStreamHandle>,
        play_audio: Arc<AtomicBool>,
        d_bus: Arc<AtomicBool>,
    ) -> Self {
        Notifier {
            ctx: ctx.clone(),
            audio,
            play_audio,
            d_bus,
        }
    }

    pub fn play_sound(&self) {
        if let Some(audio) = &self.audio {
            let mix = SineWave::new(432.0)
                .take_duration(Duration::from_secs_f32(0.2))
                .amplify(0.20)
                .fade_in(Duration::from_secs_f32(0.2))
                .buffered()
                .reverb(Duration::from_secs_f32(0.5), 0.2);
            let mix = SineWave::new(564.0)
                .take_duration(Duration::from_secs_f32(0.2))
                .amplify(0.10)
                .fade_in(Duration::from_secs_f32(0.2))
                .buffered()
                .reverb(Duration::from_secs_f32(0.3), 0.2)
                .mix(mix);
            audio.play_raw(mix).ok();
        }
    }
}

impl Repaintable for Notifier {
    fn request_repaint(&self) {
        self.ctx.request_repaint();
    }
    fn notify(&self, text: &str) {
        self.ctx.request_repaint();
        if self.play_audio.load(Ordering::Relaxed) {
            self.play_sound();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.d_bus.load(Ordering::Relaxed) {
            Notification::new().summary("Roomor").body(text).show().ok();
        }
    }
}

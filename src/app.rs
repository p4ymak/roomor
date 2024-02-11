use crate::chat::Repaintable;

use super::chat::{message::Message, Recepients, UdpChat};
use directories::ProjectDirs;
use eframe::{egui, CreationContext};
use egui::*;

pub struct ChatApp {
    init: bool,
    chat: UdpChat,
    text: String,
}

impl eframe::App for ChatApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.chat.message = Message::exit();
        self.chat.send(Recepients::All);
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.init {
            self.setup(ctx);
        }
        self.chat.receive();
        self.draw(ctx);
        self.handle_keys(ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // NOTE: a bright gray makes the shadows of the windows look weird.
        // We use a bit of transparency so that if the user switches on the
        // `transparent()` option they get immediate results.
        egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()

        // _visuals.window_fill() would also be a natural choice
    }

    fn persist_egui_memory(&self) -> bool {
        true
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        let db_path = ProjectDirs::from("com", "p4ymak", env!("CARGO_PKG_NAME")).map(|p| {
            std::fs::create_dir_all(p.data_dir()).ok();
            p.data_dir().join("history.db")
        });
        ChatApp {
            init: false,
            chat: UdpChat::new("XXX".to_string(), 4444, db_path),
            text: String::new(),
        }
    }
}
impl Repaintable for egui::Context {
    fn request_repaint(&self) {
        self.request_repaint()
    }
}
impl ChatApp {
    pub fn new(_cc: &CreationContext) -> Self {
        ChatApp::default()
    }
    fn setup(&mut self, ctx: &egui::Context) {
        self.chat.prelude(ctx);
        self.init = true;
    }
    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            i.raw.events.iter().for_each(|event| match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => self.send(),
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => self.chat.clear_history(),

                _ => (),
            })
        })
    }
    fn send(&mut self) {
        if !self.text.trim().is_empty() {
            self.chat.message = Message::text(&self.text);
            self.chat.send(Recepients::Peers);
        }
        self.text = String::new();
    }
    fn draw(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("socket").show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(Align::LEFT), |ui| {
                ui.add(egui::Label::new(format!("Online: {}", self.chat.peers.len())).wrap(false));
                ui.label(format!("{}:{}", self.chat.ip, self.chat.port));
                ui.label(&self.chat.db_status);
            });
        });
        egui::TopBottomPanel::bottom("text intput")
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([true; 2])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .desired_width(ui.available_width()),
                        )
                        .request_focus();
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_width(f32::INFINITY)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    self.chat.history.iter().for_each(|m| {
                        let (direction, fill_color) = match &m.0 {
                            x if x == &self.chat.ip => (
                                egui::Direction::RightToLeft,
                                egui::Color32::from_rgb(70, 70, 70),
                            ),
                            _ => (
                                egui::Direction::LeftToRight,
                                egui::Color32::from_rgb(42, 42, 42),
                            ),
                        };
                        ui.with_layout(
                            egui::Layout::from_main_dir_and_cross_align(
                                direction,
                                egui::Align::Min,
                            ),
                            |line| {
                                if m.0 != self.chat.ip {
                                    line.add(
                                        egui::Label::new(&m.1).wrap(false).sense(Sense::click()),
                                    )
                                    .clicked();
                                }
                                if line
                                    .add(egui::Button::new(&m.1).wrap(true).fill(fill_color))
                                    .clicked()
                                {
                                    self.text.push_str(&m.1);
                                }
                            },
                        );
                    });
                });
        });
    }
}

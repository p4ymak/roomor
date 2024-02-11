use crate::chat::{Repaintable, TextMessage};

use super::chat::{message::Message, Recepients, UdpChat};
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
        } else {
            self.chat.receive();
            self.draw(ctx);
            self.handle_keys(ctx);
        }
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
        // let db_path = ProjectDirs::from("com", "p4ymak", env!("CARGO_PKG_NAME")).map(|p| {
        //     std::fs::create_dir_all(p.data_dir()).ok();
        //     p.data_dir().join("history.db")
        // });
        ChatApp {
            init: false,
            chat: UdpChat::new("XXX".to_string(), 4444),
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.heading("Port");
                ui.add(egui::DragValue::new(&mut self.chat.port));
                ui.heading("Name");
                ui.text_edit_singleline(&mut self.chat.name);
                if ui.button("Connect").clicked() {
                    self.chat.prelude(ctx);
                    self.init = true;
                }
            })
        });
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
                ui.add(egui::Label::new(format!("Online: {}", self.chat.peers.len())).wrap(false))
                    .on_hover_ui(|h| {
                        for (ip, name) in self.chat.peers.iter() {
                            h.label(format!("{ip} - {name}"));
                        }
                    });
                ui.label(format!("{}:{}", self.chat.ip, self.chat.port));
                // ui.label(&self.chat.db_status);
            });
        });
        egui::TopBottomPanel::bottom("text intput")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .frame(false)
                        .desired_width(ctx.available_rect().width()),
                )
                .request_focus();
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                // .max_width(f32::INFINITY)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    self.chat.history.iter().for_each(|m| {
                        let ip_str = &m.ip().to_string();
                        let incoming = (m.ip() != self.chat.ip)
                            .then_some(self.chat.peers.get(&m.ip()).unwrap_or(ip_str));
                        m.draw(ui, incoming);
                    });
                });
        });
    }
}

impl TextMessage {
    fn draw(&self, ui: &mut egui::Ui, incoming: Option<&String>) {
        let (direction, _fill_color) = if incoming.is_some() {
            (
                egui::Direction::LeftToRight,
                egui::Color32::from_rgb(42, 42, 42),
            )
        } else {
            (
                egui::Direction::RightToLeft,
                egui::Color32::from_rgb(70, 70, 70),
            )
        };
        ui.with_layout(
            egui::Layout::from_main_dir_and_cross_align(direction, egui::Align::Min)
                .with_main_wrap(true),
            |line| {
                let mut rounding = Rounding::same(12.0);
                if incoming.is_some() {
                    rounding.sw = 0.0;
                } else {
                    rounding.se = 0.0;
                }
                egui::Frame::group(line.style())
                    .rounding(rounding)
                    .stroke(Stroke::new(
                        1.0,
                        Color32::from_additive_luminance(
                            self.ip().octets().last().cloned().unwrap_or(0),
                        ),
                    ))
                    .show(line, |g| {
                        if let Some(name) = incoming {
                            g.vertical(|g| {
                                g.label(name)
                                    .on_hover_text_at_pointer(self.ip().to_string());
                                g.heading(self.text());
                            });
                        } else {
                            g.heading(self.text());
                        }
                    });
            },
        );
    }
}

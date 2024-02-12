use super::chat::{
    message::{Message, MAX_TEXT_SIZE},
    MessageContent, Peer, Recepients, Repaintable, TextMessage, UdpChat,
};
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
        self.top_panel(ctx);
        if !self.init {
            self.setup(ctx);
        } else {
            if self.chat.receive() {
                ctx.request_repaint();
            }
            self.draw(ctx);
        }
        self.handle_keys(ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        ChatApp {
            init: false,
            chat: UdpChat::new(String::new(), 4444),
            text: String::with_capacity(MAX_TEXT_SIZE),
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
                let rmr = egui::RichText::new("RMЯ")
                    .monospace()
                    .size(ui.available_width() * 0.4);
                ui.heading(rmr);
                ui.group(|ui| {
                    ui.heading("Port");
                    ui.add(egui::DragValue::new(&mut self.chat.port));
                    ui.heading("Display Name");
                    ui.text_edit_singleline(&mut self.chat.name).request_focus();
                });
            });
        });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            i.raw.events.iter().for_each(|event| match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => {
                    if self.init {
                        self.send()
                    } else {
                        self.chat.prelude(ctx);
                        self.init = true;
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => self.chat.clear_history(),
                _ => (),
            })
        })
    }

    fn limit_text(&mut self) {
        self.text = self.text.trim_end_matches('\n').to_string();
        self.text.truncate(MAX_TEXT_SIZE);
    }

    fn send(&mut self) {
        if !self.text.trim().is_empty() {
            self.chat.message = Message::text(&self.text);
            self.chat.send(Recepients::Peers);
        }
        self.text = String::new();
    }

    fn top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|h| {
                if h.button(egui::RichText::new("-").monospace()).clicked() {
                    let ppp = h.ctx().pixels_per_point().max(0.75);
                    h.ctx().set_pixels_per_point(ppp - 0.25);
                    h.ctx().request_repaint();
                }
                if h.button(egui::RichText::new("=").monospace()).clicked() {
                    h.ctx().set_pixels_per_point(1.0);
                    h.ctx().request_repaint();
                }
                if h.button(egui::RichText::new("+").monospace()).clicked() {
                    let ppp = h.ctx().pixels_per_point();
                    h.ctx().set_pixels_per_point(ppp + 0.25);
                    h.ctx().request_repaint();
                }
                egui::widgets::global_dark_light_mode_switch(h);
                h.separator();
                self.sound_mute_button(h);
                if self.init {
                    h.separator();
                    h.add(
                        egui::Label::new(format!(
                            "Online: {}",
                            self.chat.peers.values().filter(|p| p.is_online()).count()
                        ))
                        .wrap(false),
                    )
                    .on_hover_ui(|h| {
                        for (ip, peer) in self.chat.peers.iter() {
                            let mut label = egui::RichText::new(format!("{ip} - {}", peer.name()));
                            if !peer.is_online() {
                                label = label.weak();
                            }
                            h.label(label);
                        }
                    });
                    h.separator();
                    if self.chat.name.is_empty() {
                        h.label(format!("{}:{}", self.chat.ip, self.chat.port));
                    } else {
                        h.label(&self.chat.name).on_hover_ui_at_pointer(|h| {
                            h.label(format!("{}:{}", self.chat.ip, self.chat.port));
                        });
                    }
                }
            });
        });
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("text intput")
            .resizable(false)
            .show(ctx, |ui| {
                let y = ui.max_rect().min.y;
                let rect = ui.clip_rect();
                let len = self.text.len();
                if len > 0 {
                    ui.painter().hline(
                        rect.min.x..=(rect.max.x * (len as f32 / MAX_TEXT_SIZE as f32)),
                        y,
                        ui.style().visuals.widgets.inactive.fg_stroke,
                    );
                }
                ui.horizontal(|h| {
                    self.limit_text();
                    h.add(
                        egui::TextEdit::multiline(&mut self.text)
                            .frame(false)
                            .desired_rows(3)
                            .desired_width(h.available_rect_before_wrap().width()),
                    )
                    .request_focus();
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    self.chat.history.iter().for_each(|m| {
                        m.draw(ui, self.chat.peers.get(&m.ip()));
                    });
                });
        });
    }
    fn sound_mute_button(&mut self, ui: &mut egui::Ui) {
        let play_audio = self
            .chat
            .play_audio
            .load(std::sync::atomic::Ordering::Relaxed);
        let icon = if play_audio {
            egui::RichText::new("🔉").monospace()
        } else {
            egui::RichText::new("🔇").monospace()
        };
        if ui.button(icon).clicked() {
            self.chat
                .play_audio
                .store(!play_audio, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl TextMessage {
    fn draw(&self, ui: &mut egui::Ui, incoming: Option<&Peer>) {
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
                let mut rounding = Rounding::same(line.style().visuals.window_rounding.nw * 2.0);
                if incoming.is_some() {
                    rounding.sw = 0.0;
                } else {
                    rounding.se = 0.0;
                }
                egui::Frame::group(line.style())
                    .rounding(rounding)
                    .stroke(line.style().visuals.widgets.inactive.fg_stroke)
                    .show(line, |g| {
                        if let Some(peer) = incoming {
                            g.vertical(|g| {
                                g.horizontal(|h| {
                                    if peer.name().is_empty() {
                                        let mut label = egui::RichText::new(self.ip().to_string());
                                        if !peer.is_online() {
                                            label = label.weak();
                                        }
                                        h.label(label);
                                    } else {
                                        let mut label = egui::RichText::new(peer.name());
                                        if peer.is_online() {
                                            label = label.strong();
                                        }
                                        h.label(label)
                                            .on_hover_text_at_pointer(self.ip().to_string());
                                    }
                                    match self.content() {
                                        MessageContent::Joined => {
                                            h.label("joined..");
                                        }
                                        MessageContent::Left => {
                                            h.label("left..");
                                        }
                                        _ => (),
                                    }
                                });
                                self.draw_text(g);
                            });
                        } else {
                            self.draw_text(g);
                        }
                    });
            },
        );
    }
}

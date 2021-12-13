use super::chat::{Command, Recepients, UdpChat};
use eframe::{egui, epi};
use egui::*;
use epi::Storage;

pub struct ChatApp {
    chat: UdpChat,
    text: String,
}

impl epi::App for ChatApp {
    fn name(&self) -> &str {
        "UDP Chat"
    }
    fn warm_up_enabled(&self) -> bool {
        true
    }
    // fn persist_native_window(&self) -> bool {
    //     false
    // }
    // fn persist_egui_memory(&self) -> bool {
    //     false
    // }
    // fn auto_save_interval(&self) -> Duration {
    //     Duration::MAX
    // }
    fn setup(
        &mut self,
        _ctx: &egui::CtxRef,
        _frame: &mut epi::Frame<'_>,
        _storage: Option<&dyn Storage>,
    ) {
        self.chat.connect();
    }
    fn on_exit(&mut self) {
        self.chat.message = Command::Exit;
        self.chat.send(Recepients::All);
    }

    fn update(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
        self.chat.read();
        self.draw(ctx);
        self.handle_keys(ctx);
        ctx.request_repaint();
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        ChatApp {
            chat: UdpChat::new("XXX".to_string(), 4444),
            text: String::new(),
        }
    }
}
impl ChatApp {
    fn handle_keys(&mut self, ctx: &egui::CtxRef) {
        for event in &ctx.input().raw.events {
            match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => self.send(),
                _ => (),
            }
        }
    }
    fn send(&mut self) {
        if !self.text.trim().is_empty() {
            self.chat.message = Command::Text(self.text.clone());
            self.chat.send(Recepients::Peers);
        }
        self.text = String::new();
    }
    fn draw(&mut self, ctx: &egui::CtxRef) {
        egui::TopBottomPanel::top("socket").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(), |ui| {
                ui.add(
                    egui::Label::new(format!("Online: {}", self.chat.peers.len()))
                        .wrap(false)
                        .strong(), // .sense(Sense::click()),
                );
                ui.label(format!("{}:{}", self.chat.ip, self.chat.port));
            });
        });
        egui::TopBottomPanel::bottom("my_panel").show(ctx, |ui| {
            let message_box = ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_width(f32::INFINITY)
                    .text_style(egui::TextStyle::Heading)
                    .id(egui::Id::new("text_input")),
            );
            message_box.request_focus();
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_width(f32::INFINITY)
                .stick_to_bottom()
                .always_show_scroll(true)
                .show(ui, |ui| {
                    ui.with_layout(
                        egui::Layout::from_main_dir_and_cross_align(
                            egui::Direction::BottomUp,
                            egui::Align::Max,
                        ),
                        |ui| {
                            self.chat.history.iter().for_each(|m| {
                                let direction = match &m.0 {
                                    x if x == &self.chat.ip => egui::Direction::RightToLeft,
                                    _ => egui::Direction::LeftToRight,
                                };
                                ui.with_layout(
                                    egui::Layout::from_main_dir_and_cross_align(
                                        direction,
                                        egui::Align::Max,
                                    ),
                                    |line| {
                                        line.add(
                                            egui::Label::new(&m.0)
                                                .wrap(false)
                                                .strong()
                                                .sense(Sense::click()),
                                        )
                                        .clicked();
                                        line.add(
                                            egui::Button::new(&m.1)
                                                .wrap(true)
                                                .text_style(egui::TextStyle::Heading)
                                                .fill(egui::Color32::from_rgb(42, 42, 42)),
                                        );
                                    },
                                );
                            });
                        },
                    );
                });
        });
    }
}

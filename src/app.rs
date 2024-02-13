use super::chat::{
    message::MAX_TEXT_SIZE,
    notifier::{Notifier, Repaintable},
    utf8_truncate, BackEvent, ChatEvent, FrontEvent, MessageContent, TextMessage, UdpChat,
};
use eframe::{egui, CreationContext};
use egui::*;
use flume::{Receiver, Sender};
use rodio::{OutputStream, OutputStreamHandle};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    net::Ipv4Addr,
    sync::{atomic::AtomicBool, Arc},
    thread,
};

pub const FONT_SCALE: f32 = 1.5;

pub struct Roomor {
    name: String,
    ip: Ipv4Addr,
    port: u16,
    chat_init: Option<UdpChat>,
    history: Vec<TextMessage>,
    peers: BTreeMap<Ipv4Addr, Peer>,
    text: String,
    _audio: Option<OutputStream>,
    audio_handler: Option<OutputStreamHandle>,
    play_audio: Arc<AtomicBool>,
    back_rx: Receiver<BackEvent>,
    back_tx: Sender<ChatEvent>,
}

impl eframe::App for Roomor {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.back_tx.send(ChatEvent::Front(FrontEvent::Exit)).ok();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.top_panel(ctx);
        if self.chat_init.is_some() {
            self.setup(ctx);
        } else {
            self.read_events();
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

impl Default for Roomor {
    fn default() -> Self {
        let (_audio, audio_handler) = match OutputStream::try_default() {
            Ok((audio, audio_handler)) => (Some(audio), Some(audio_handler)),
            Err(_) => (None, None),
        };
        let (front_tx, back_rx) = flume::unbounded();
        let play_audio = Arc::new(AtomicBool::new(true));
        let chat = UdpChat::new(String::new(), 4444, front_tx, play_audio.clone());
        let back_tx = chat.tx();
        Roomor {
            name: String::default(),
            ip: Ipv4Addr::UNSPECIFIED,
            port: 4444,
            chat_init: Some(chat),
            history: vec![],
            peers: BTreeMap::<Ipv4Addr, Peer>::new(),
            text: String::with_capacity(MAX_TEXT_SIZE),
            _audio,
            audio_handler,
            play_audio,
            back_tx,
            back_rx,
        }
    }
}

impl Roomor {
    pub fn new(_cc: &CreationContext) -> Self {
        Roomor::default()
    }

    fn read_events(&mut self) {
        for event in self.back_rx.try_iter() {
            match event {
                BackEvent::PeerJoined((r_ip, name)) => {
                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        ip.insert(Peer::new(name));
                        self.history.push(TextMessage::enter(r_ip));
                    } else if let Some(peer) = self.peers.get_mut(&r_ip) {
                        if !peer.online {
                            peer.online = true;
                            self.history.push(TextMessage::enter(r_ip));
                        }
                        if peer.name != name {
                            // FIXME
                            peer.name = name;
                        }
                    }
                }
                BackEvent::PeerLeft(ip) => {
                    self.history.push(TextMessage::exit(ip));
                    self.peers.entry(ip).and_modify(|p| p.online = false);
                }
                BackEvent::Message(msg) => self.history.push(msg),
                BackEvent::MyIp(ip) => self.ip = ip,
            }
        }
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
                    ui.add(egui::DragValue::new(&mut self.port));
                    ui.heading("Display Name");
                    ui.text_edit_singleline(&mut self.name).request_focus();
                });
            });
        });
    }
    fn init_chat(&mut self, ctx: &egui::Context) {
        if let Some(mut init) = self.chat_init.take() {
            let ctx = Notifier::new(ctx, self.audio_handler.clone());
            init.prelude(&self.name, self.port);
            thread::spawn(move || init.run(&ctx));
        }
        self.back_tx
            .send(ChatEvent::Front(FrontEvent::Enter(self.name.to_string())))
            .ok();
    }
    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            i.raw.events.iter().for_each(|event| match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => {
                    if self.chat_init.is_some() {
                        self.init_chat(ctx);
                    } else {
                        self.send();
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => self.history.clear(),
                _ => (),
            })
        })
    }

    fn limit_text(&mut self) {
        self.text = self.text.trim_end_matches('\n').to_string();
        utf8_truncate(&mut self.text, MAX_TEXT_SIZE);
    }

    fn send(&mut self) {
        if !self.text.trim().is_empty() && self.peers.values().any(|p| p.is_online()) {
            self.back_tx
                .send(ChatEvent::Front(FrontEvent::Send(
                    self.text.trim().to_string(),
                )))
                .ok();
            self.text.clear();
        }
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
                if self.chat_init.is_none() {
                    h.separator();
                    h.add(
                        egui::Label::new(format!(
                            "Online: {}",
                            self.peers.values().filter(|p| p.is_online()).count()
                        ))
                        .wrap(false),
                    )
                    .on_hover_ui(|h| {
                        for (ip, peer) in self.peers.iter() {
                            let mut label = egui::RichText::new(format!("{ip} - {}", peer.name()));
                            if !peer.is_online() {
                                label = label.weak();
                            }
                            h.label(label);
                        }
                    });
                    h.separator();
                    if self.name.is_empty() {
                        h.label(format!("{}:{}", self.ip, self.port));
                    } else {
                        h.label(&self.name).on_hover_ui_at_pointer(|h| {
                            h.label(format!("{}:{}", self.ip, self.port));
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
                for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                    font_id.size *= FONT_SCALE;
                }
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
                    self.history.iter().for_each(|m| {
                        m.draw(ui, self.peers.get(&m.ip()));
                    });
                });
        });
    }
    fn sound_mute_button(&mut self, ui: &mut egui::Ui) {
        let play_audio = self.play_audio.load(std::sync::atomic::Ordering::Relaxed);
        let icon = if play_audio {
            egui::RichText::new("🔉").monospace()
        } else {
            egui::RichText::new("🔇").monospace()
        };
        if ui.button(icon).clicked() {
            self.play_audio
                .store(!play_audio, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl Repaintable for egui::Context {
    fn request_repaint(&self) {
        self.request_repaint()
    }
}

pub struct Peer {
    name: String,
    online: bool,
}
impl Peer {
    pub fn new(name: impl Into<String>) -> Self {
        Peer {
            name: name.into(),
            online: true,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn is_online(&self) -> bool {
        self.online
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
    pub fn draw_text(&self, ui: &mut eframe::egui::Ui) {
        if let MessageContent::Text(content) = self.content() {
            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                font_id.size *= FONT_SCALE;
            }
            if content.starts_with("http") {
                if let Some((link, text)) = content.split_once(' ') {
                    ui.hyperlink(link);
                    ui.label(text);
                } else {
                    ui.hyperlink(content);
                }
            } else {
                ui.label(content);
            }
        }
    }
}

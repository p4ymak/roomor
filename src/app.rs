use super::chat::{
    message::{MAX_EMOJI_SIZE, MAX_NAME_SIZE, MAX_TEXT_SIZE},
    notifier::{Notifier, Repaintable},
    utf8_truncate, BackEvent, ChatEvent, FrontEvent, TextMessage, UdpChat,
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
pub const EMOJI_SCALE: f32 = 4.0;

pub struct Roomor {
    name: String,
    ip: Ipv4Addr,
    port: u16,
    emoji_mode: bool,
    chat_init: Option<UdpChat>,
    history: Vec<TextMessage>,
    peers: BTreeMap<Ipv4Addr, Peer>,
    text: String,
    _audio: Option<OutputStream>,
    audio_handler: Option<OutputStreamHandle>,
    play_audio: Arc<AtomicBool>,
    d_bus: Arc<AtomicBool>,
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
        let d_bus = Arc::new(AtomicBool::new(true));
        let chat = UdpChat::new(String::new(), 4444, front_tx);
        let back_tx = chat.tx();
        Roomor {
            name: String::default(),
            ip: Ipv4Addr::UNSPECIFIED,
            port: 4444,
            chat_init: Some(chat),
            history: vec![],
            peers: BTreeMap::<Ipv4Addr, Peer>::new(),
            text: String::with_capacity(MAX_TEXT_SIZE),
            emoji_mode: false,
            _audio,
            audio_handler,
            play_audio,
            d_bus,
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
                        ip.insert(Peer::new(name.as_ref()));
                        self.history.push(TextMessage::enter(
                            r_ip,
                            name.clone().unwrap_or(r_ip.to_string()),
                        ));
                    } else if let Some(peer) = self.peers.get_mut(&r_ip) {
                        if !peer.is_online() {
                            self.history.push(TextMessage::enter(
                                r_ip,
                                name.clone().unwrap_or(r_ip.to_string()),
                            ));
                        }
                        if name.is_some() {
                            peer.name = name;
                        }
                        peer.online = true;
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
                    self.font_multiply(ui);
                    ui.heading("Port");
                    ui.add(egui::DragValue::new(&mut self.port));
                    ui.heading("Display Name");
                    limit_text(&mut self.name, MAX_NAME_SIZE);
                    ui.text_edit_singleline(&mut self.name).request_focus();
                });
            });
        });
    }

    fn init_chat(&mut self, ctx: &egui::Context) {
        if let Some(mut init) = self.chat_init.take() {
            let ctx = Notifier::new(
                ctx,
                self.audio_handler.clone(),
                self.play_audio.clone(),
                self.d_bus.clone(),
            );
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
                        if !self.name.trim().is_empty() {
                            self.init_chat(ctx);
                        }
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

    fn send(&mut self) {
        if !self.text.trim().is_empty() {
            // && self.peers.values().any(|p| p.is_online()) {
            let txt = self.text.trim().to_string();
            let msg_ty = match self.emoji_mode {
                true => FrontEvent::Icon(txt),
                false => FrontEvent::Text(txt),
            };
            self.back_tx.send(ChatEvent::Front(msg_ty)).ok();
            self.text.clear();
        }
    }

    fn top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|h| {
                // GUI Settings
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

                // Notifications
                h.separator();
                atomic_button(&self.play_audio, '♪', h);
                atomic_button(&self.d_bus, '⚑', h);

                // Online Summary
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
                            let name = match peer.name() {
                                Some(name) => format!("{ip} - {name}"),
                                None => format!("{ip}"),
                            };
                            let mut label = egui::RichText::new(name);
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
                self.emoji_mode = self.text.starts_with(' ');
                let limit = match self.emoji_mode {
                    true => MAX_EMOJI_SIZE,
                    false => MAX_TEXT_SIZE,
                };
                limit_text(&mut self.text, limit);
                self.font_multiply(ui);

                let y = ui.max_rect().min.y;
                let rect = ui.clip_rect();
                let len = self.text.len();
                if len > 0 {
                    ui.painter().hline(
                        rect.min.x..=(rect.max.x * (len as f32 / limit as f32)),
                        y,
                        ui.style().visuals.widgets.inactive.fg_stroke,
                    );
                }

                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .frame(false)
                        .cursor_at_end(true)
                        .desired_rows(if self.emoji_mode { 1 } else { 4 })
                        .desired_width(ui.available_rect_before_wrap().width()),
                )
                .request_focus();
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    self.history.iter().for_each(|m| {
                        m.draw(ui, self.peers.get(&m.ip()));
                    });
                });
        });
    }

    fn font_multiply(&self, ui: &mut egui::Ui) {
        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
            let emoji_scale = if self.emoji_mode { 4.0 } else { 1.0 };
            font_id.size *= FONT_SCALE * emoji_scale;
        }
    }
}

impl Repaintable for egui::Context {
    fn request_repaint(&self) {
        self.request_repaint()
    }
}

pub struct Peer {
    name: Option<String>,
    online: bool,
}
impl Peer {
    pub fn new(name: Option<impl Into<String>>) -> Self {
        Peer {
            name: name.map(|n| n.into()),
            online: true,
        }
    }
    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
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
                                    match peer.name() {
                                        None => {
                                            let mut label =
                                                egui::RichText::new(self.ip().to_string());
                                            if !peer.is_online() {
                                                label = label.weak();
                                            }
                                            h.label(label);
                                        }
                                        Some(name) => {
                                            let mut label = egui::RichText::new(name);
                                            if peer.is_online() {
                                                label = label.strong();
                                            }
                                            h.label(label)
                                                .on_hover_text_at_pointer(self.ip().to_string());
                                        }
                                    }
                                    match self.content() {
                                        FrontEvent::Enter(_) => {
                                            h.label("joined..");
                                        }
                                        FrontEvent::Exit => {
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
        match self.content() {
            FrontEvent::Text(content) => {
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
            FrontEvent::Icon(content) => {
                for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                    font_id.size *= FONT_SCALE * EMOJI_SCALE;
                }
                ui.label(content);
            }
            _ => (),
        }
    }
}

fn atomic_button(value: &Arc<AtomicBool>, icon: char, ui: &mut egui::Ui) {
    let val = value.load(std::sync::atomic::Ordering::Relaxed);
    let mut icon = egui::RichText::new(icon).monospace();
    if !val {
        icon = icon.weak();
    }
    if ui
        .button(icon)
        .on_hover_text_at_pointer("Pop Notifications")
        .clicked()
    {
        value.store(!val, std::sync::atomic::Ordering::Relaxed);
    }
}
fn limit_text(text: &mut String, limit: usize) {
    *text = text.trim_end_matches('\n').to_string();
    utf8_truncate(text, limit);
}

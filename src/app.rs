use crate::chat::{Content, FrontEvent};

use super::chat::{
    message::{MAX_EMOJI_SIZE, MAX_NAME_SIZE, MAX_TEXT_SIZE},
    networker::TIMEOUT,
    notifier::{Notifier, Repaintable},
    peers::{Peer, PeersMap},
    utf8_truncate, BackEvent, ChatEvent, Recepients, TextMessage, UdpChat,
};

use eframe::{egui, CreationContext};
use egui::*;
use flume::{Receiver, Sender};
use rodio::{OutputStream, OutputStreamHandle};
use std::{
    collections::BTreeMap,
    net::Ipv4Addr,
    sync::{atomic::AtomicBool, Arc},
    thread,
    time::SystemTime,
};

pub const FONT_SCALE: f32 = 1.5;
pub const EMOJI_SCALE: f32 = 4.0;

pub struct Chats {
    active_chat: Recepients,
    peers: PeersMap,
    order: Vec<Recepients>,
    chats: BTreeMap<Recepients, ChatHistory>,
}
impl Chats {
    pub fn new() -> Self {
        let mut chats = BTreeMap::new();
        chats.insert(Recepients::Peers, ChatHistory::new(Recepients::Peers));
        Chats {
            active_chat: Recepients::Peers,
            peers: PeersMap::new(),
            order: vec![],
            chats,
        }
    }

    fn get_mut_public(&mut self) -> &mut ChatHistory {
        self.chats
            .get_mut(&Recepients::Peers)
            .expect("Public Exists")
    }

    fn get_mut_peer(&mut self, ip: Ipv4Addr) -> &mut ChatHistory {
        self.chats
            .entry(Recepients::One(ip))
            .or_insert(ChatHistory::new(Recepients::One(ip)))
    }

    fn get_mut_active(&mut self) -> &mut ChatHistory {
        self.chats
            .get_mut(&self.active_chat)
            .expect("Active Exists")
    }

    fn get_active(&self) -> &ChatHistory {
        self.chats.get(&self.active_chat).expect("Active Exists")
    }

    pub fn compose_message(&mut self) -> Option<TextMessage> {
        let chat = self.get_mut_active();
        let trimmed = chat.input.trim().to_string();
        (!trimmed.is_empty()).then_some(
            // && self.peers.values().any(|p| p.is_online()) {
            {
                chat.input.clear();
                if chat.emoji_mode {
                    TextMessage::out_message(Content::Icon(trimmed), chat.recepients)
                } else {
                    TextMessage::out_message(Content::Text(trimmed), chat.recepients)
                }
            },
        )
    }

    pub fn peer_joined(&mut self, ip: Ipv4Addr, name: Option<String>) {
        if self.peers.peer_joined(ip, name.as_ref()) {
            let msg = TextMessage::in_enter(ip, name.unwrap_or(ip.to_string()));
            self.get_mut_public().history.push(msg.clone());
            self.get_mut_peer(ip).history.push(msg);
        }
    }

    pub fn peer_left(&mut self, ip: Ipv4Addr) {
        self.get_mut_public().history.push(TextMessage::in_exit(ip));
        self.get_mut_peer(ip).history.push(TextMessage::in_exit(ip));
        self.peers.peer_exited(ip);
    }

    pub fn take_message(&mut self, msg: TextMessage) {
        if msg.is_public() {
            self.get_mut_public().history.push(msg);
        } else {
            self.get_mut_peer(msg.ip()).history.push(msg);
        }
    }

    fn recalculate_order(&mut self) {
        let mut order = self
            .chats
            .values()
            .filter_map(|c| c.history.last().map(|m| (m.time(), c.recepients)))
            .collect::<Vec<_>>();
        order.sort_by(|a, b| b.0.cmp(&a.0));
        self.order = order.into_iter().map(|o| o.1).collect();
    }

    pub fn draw_history(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                self.get_active().history.iter().for_each(|m| {
                    let peer = m
                        .is_incoming()
                        .then_some(self.peers.0.get(&m.ip()))
                        .flatten();
                    m.draw(ui, peer);
                });
            });
    }

    pub fn draw_list(&mut self, ui: &mut egui::Ui) {
        ui.selectable_value(&mut self.active_chat, Recepients::Peers, "PUBLIC");
        egui::ScrollArea::both().show(ui, |ui| {
            for peer in self.order.iter() {
                if let Recepients::One(ip) = peer {
                    ui.selectable_value(
                        &mut self.active_chat,
                        *peer,
                        self.peers.get_display_name(*ip),
                    );
                }
            }
        });
    }
}

pub struct ChatHistory {
    recepients: Recepients,
    emoji_mode: bool,
    input: String,
    history: Vec<TextMessage>,
}

impl ChatHistory {
    pub fn new(recepients: Recepients) -> Self {
        ChatHistory {
            recepients,
            emoji_mode: false,
            input: String::with_capacity(MAX_TEXT_SIZE),
            history: vec![],
        }
    }

    fn font_multiply(&self, ui: &mut egui::Ui) {
        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
            let emoji_scale = if self.emoji_mode { 4.0 } else { 1.0 };
            font_id.size *= FONT_SCALE * emoji_scale;
        }
    }

    pub fn draw_input(&mut self, ui: &mut egui::Ui) {
        self.emoji_mode = self.input.starts_with(' ');
        let limit = match self.emoji_mode {
            true => MAX_EMOJI_SIZE,
            false => MAX_TEXT_SIZE,
        };
        limit_text(&mut self.input, limit);
        self.font_multiply(ui);

        let y = ui.max_rect().min.y;
        let rect = ui.clip_rect();
        let len = self.input.len();
        if len > 0 {
            ui.painter().hline(
                rect.min.x..=(rect.max.x * (len as f32 / limit as f32)),
                y,
                ui.style().visuals.widgets.inactive.fg_stroke,
            );
        }

        ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .frame(false)
                .cursor_at_end(true)
                .desired_rows(if self.emoji_mode { 1 } else { 4 })
                .desired_width(ui.available_rect_before_wrap().width()),
        )
        .request_focus();
    }
}

pub struct Roomor {
    name: String,
    ip: Ipv4Addr,
    port: u16,
    chat_init: Option<UdpChat>,
    chats: Chats,
    _audio: Option<OutputStream>,
    audio_handler: Option<OutputStreamHandle>,
    play_audio: Arc<AtomicBool>,
    d_bus: Arc<AtomicBool>,
    back_rx: Receiver<BackEvent>,
    back_tx: Sender<ChatEvent>,
    error_message: Option<String>,
    last_time: SystemTime,
}

impl eframe::App for Roomor {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.back_tx.send(ChatEvent::Front(FrontEvent::Exit)).ok();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.stay_alive();
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
            chats: Chats::new(),
            _audio,
            audio_handler,
            play_audio,
            d_bus,
            back_tx,
            back_rx,
            error_message: None,
            last_time: SystemTime::now(),
        }
    }
}

impl Roomor {
    pub fn new(_cc: &CreationContext) -> Self {
        Roomor::default()
    }

    fn stay_alive(&mut self) {
        let now = SystemTime::now();
        if now
            .duration_since(self.last_time)
            .is_ok_and(|t| t > TIMEOUT)
        {
            self.chats.peers.check_alive();
            self.back_tx.send(ChatEvent::Front(FrontEvent::Alive)).ok();
            self.last_time = now;
        }
    }

    fn dispatch(&mut self) {
        self.last_time = SystemTime::now();
        if let Some(msg) = self.chats.compose_message() {
            self.back_tx
                .send(ChatEvent::Front(FrontEvent::Message(msg)))
                .ok();
        }
    }

    fn read_events(&mut self) {
        for event in self.back_rx.try_iter() {
            match event {
                BackEvent::PeerJoined((ip, name)) => {
                    self.chats.peer_joined(ip, name);
                }
                BackEvent::PeerLeft(ip) => {
                    self.chats.peer_left(ip);
                }
                BackEvent::Message(msg) => {
                    self.chats.take_message(msg);
                }
                BackEvent::MyIp(ip) => self.ip = ip,
            }
            self.chats.recalculate_order();
        }
    }

    fn setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        let size = ui.available_width() * 0.3;
                        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                            font_id.size = size;
                        }
                        TextMessage::logo().draw(ui, None);
                    });
                    ui.label("");
                    ui.label("");
                    ui.group(|ui| {
                        self.chats.get_mut_active().font_multiply(ui);
                        ui.heading("Port");
                        ui.add(egui::DragValue::new(&mut self.port));
                        ui.heading("Display Name");
                        limit_text(&mut self.name, MAX_NAME_SIZE);
                        ui.text_edit_singleline(&mut self.name).request_focus();
                    });
                    if let Some(err) = &self.error_message {
                        ui.heading(err);
                    }
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
            match init.prelude(&self.name, self.port) {
                Ok(_) => {
                    thread::spawn(move || init.run(&ctx));
                }
                Err(err) => {
                    self.error_message = Some(format!("{err}"));
                    self.chat_init = Some(init);
                }
            }
        }
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
                        self.dispatch();
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    // self.history.clear(); //FIXME
                }
                _ => (),
            })
        })
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
                            self.chats
                                .peers
                                .0
                                .values()
                                .filter(|p| p.is_online())
                                .count()
                        ))
                        .wrap(false),
                    )
                    .on_hover_ui(|h| {
                        for (ip, peer) in self.chats.peers.0.iter() {
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
                self.chats.get_mut_active().draw_input(ui);
            });
        egui::SidePanel::left("Chats List")
            .resizable(true)
            .show(ctx, |ui| {
                self.chats.draw_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.chats.draw_history(ui);
        });
    }
}

impl Repaintable for egui::Context {
    fn request_repaint(&self) {
        self.request_repaint()
    }
}

impl TextMessage {
    fn draw(&self, ui: &mut egui::Ui, incoming: Option<&Peer>) {
        let (direction, _fill_color) = if self.is_incoming() {
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
                if self.is_incoming() {
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
                                        Content::Enter(_) => {
                                            h.label("joined..");
                                        }
                                        Content::Exit => {
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
            Content::Text(content) => {
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
            Content::Icon(content) => {
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

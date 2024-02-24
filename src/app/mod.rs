mod rooms;

use self::rooms::Rooms;
use crate::chat::{
    limit_text,
    message::MAX_NAME_SIZE,
    networker::TIMEOUT,
    notifier::{Notifier, Repaintable},
    BackEvent, ChatEvent, FrontEvent, TextMessage, UdpChat,
};
use eframe::{
    egui::{self, *},
    CreationContext,
};
use flume::{Receiver, Sender};
use rodio::{OutputStream, OutputStreamHandle};
use std::{
    net::Ipv4Addr,
    sync::{atomic::AtomicBool, Arc},
    thread::{self, JoinHandle},
    time::SystemTime,
};

pub struct Roomor {
    name: String,
    ip: Ipv4Addr,
    port: u16,
    chat_init: Option<UdpChat>,
    chat_handle: Option<JoinHandle<()>>,
    chats: Rooms,
    _audio: Option<OutputStream>,
    audio_handler: Option<OutputStreamHandle>,
    notification_sound: Arc<AtomicBool>,
    notification_d_bus: Arc<AtomicBool>,
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
        let notification_sound = Arc::new(AtomicBool::new(true));
        let notification_d_bus = Arc::new(AtomicBool::new(true));
        let chat = UdpChat::new(String::new(), 4444, front_tx);
        let back_tx = chat.tx();

        Roomor {
            name: String::default(),
            ip: Ipv4Addr::UNSPECIFIED,
            port: 4444,
            chat_init: Some(chat),
            chat_handle: None,
            chats: Rooms::new(),
            _audio,
            audio_handler,
            notification_sound,
            notification_d_bus,
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
                        let size = ui.available_width() * 0.075;
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
                self.notification_sound.clone(),
                self.notification_d_bus.clone(),
            );
            match init.prelude(&self.name, self.port) {
                Ok(_) => {
                    self.chat_handle = Some(thread::spawn(move || init.run(&ctx)));
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
                    self.back_tx.send(ChatEvent::Front(FrontEvent::Exit)).ok();
                    if let Some(handle) = self.chat_handle.take() {
                        handle.join().unwrap();
                    }
                    *self = Roomor {
                        chats: std::mem::take(&mut self.chats),
                        notification_sound: std::mem::take(&mut self.notification_sound),
                        notification_d_bus: std::mem::take(&mut self.notification_d_bus),
                        ..Default::default()
                    };
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
                atomic_button(&self.notification_sound, '♪', h);
                atomic_button(&self.notification_d_bus, '⚑', h);

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
        let mut font_size = 10.0;
        egui::TopBottomPanel::bottom("text intput")
            .resizable(false)
            .show(ctx, |ui| {
                font_size = ui.text_style_height(&egui::TextStyle::Body);
                self.chats.get_mut_active().draw_input(ui);
            });
        egui::SidePanel::left("Chats List")
            .default_width(font_size * 10.0)
            .max_width(ctx.input(|i| i.screen_rect.width()) * 0.5)
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

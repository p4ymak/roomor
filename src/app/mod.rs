mod rooms;

use self::rooms::{Rooms, FONT_SCALE};
use crate::chat::{
    limit_text,
    message::MAX_NAME_SIZE,
    networker::{get_my_ipv4, parse_netmask, TIMEOUT_ALIVE, TIMEOUT_CHECK, TIMEOUT_SECOND},
    notifier::{Notifier, Repaintable},
    BackEvent, ChatEvent, FrontEvent, Recepients, TextMessage, UdpChat,
};
use eframe::{
    egui::{self, *},
    CreationContext,
};
use flume::{Receiver, Sender};
use ipnet::Ipv4Net;
use log::debug;
use rodio::{OutputStream, OutputStreamHandle};
use std::{
    net::Ipv4Addr,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    thread::{self, sleep, JoinHandle},
    time::SystemTime,
};

pub const ZOOM_STEP: f32 = 0.25;

pub struct UserSetup {
    pub init: bool,
    name: String,
    ip: Ipv4Addr,
    port: u16,
    mask: u8,
    pub error_message: Option<String>,
}
impl Default for UserSetup {
    fn default() -> Self {
        let (ip, error_message) = match get_my_ipv4() {
            Some(ip) => (ip, None),
            None => (
                Ipv4Addr::UNSPECIFIED,
                Some("Couldn't get local IP!".to_string()),
            ),
        };
        UserSetup {
            init: true,
            name: whoami::username(),
            ip,
            port: 4444,
            mask: 24,
            error_message,
        }
    }
}
impl UserSetup {
    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn mask(&self) -> u8 {
        self.mask
    }
    pub fn draw_setup(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                font_id.size *= FONT_SCALE;
            }
            ui.heading("Name");
            limit_text(&mut self.name, MAX_NAME_SIZE);
            let name =
                ui.add(egui::TextEdit::singleline(&mut self.name).horizontal_align(Align::Center));
            if self.name.is_empty() || self.init {
                name.request_focus();
                self.init = false;
            }
            ui.heading("IPv4");
            drag_ip(ui, &self.ip);
            ui.heading("Port");
            ui.add(egui::DragValue::new(&mut self.port));
            ui.heading("Mask");
            drag_mask(ui, &mut self.mask);
        });
        if let Some(err) = &self.error_message {
            ui.heading(err);
        }
    }
}
pub struct Roomor {
    user: UserSetup,
    chat_init: Option<UdpChat>,
    chat_handle: Option<JoinHandle<()>>,
    pulse_handle: Option<JoinHandle<()>>,
    rooms: Rooms,

    _audio: Option<OutputStream>,
    audio_handle: Option<OutputStreamHandle>,
    notification_sound: Arc<AtomicBool>,
    notification_d_bus: Arc<AtomicBool>,
    back_rx: Receiver<BackEvent>,
    back_tx: Sender<ChatEvent>,
    last_time: SystemTime,
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
            self.keep_alive();
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
        let user = UserSetup::default();

        let chat = UdpChat::new(user.ip(), front_tx);
        let back_tx = chat.tx();

        Roomor {
            user,
            chat_init: Some(chat),
            chat_handle: None,
            pulse_handle: None,
            rooms: Rooms::new(back_tx.clone()),
            _audio,
            audio_handle: audio_handler,
            notification_sound,
            notification_d_bus,
            back_tx,
            back_rx,
            last_time: SystemTime::now(),
        }
    }
}

impl Roomor {
    pub fn new(_cc: &CreationContext) -> Self {
        Roomor::default()
    }

    fn keep_alive(&mut self) {
        let now = SystemTime::now();
        if let Ok(delta) = now.duration_since(self.last_time) {
            if delta > TIMEOUT_CHECK {
                debug!("Front Check Peers");
                self.rooms.peers.check_alive(now);
                self.last_time = now;
            }
            if delta > TIMEOUT_ALIVE {
                debug!("Front Alive Ping");
                self.rooms.update_peers_online(&self.back_tx);
                self.last_time = now;
            }
        }
    }

    fn dispatch_text(&mut self) {
        if let Some(msg) = self.rooms.compose_message() {
            self.back_tx
                .send(ChatEvent::Front(FrontEvent::Message(msg)))
                .ok();
        }
    }

    fn dispatch_file(&mut self, path: &Path) {
        if let Some(link) = self.rooms.compose_file(path) {
            self.back_tx
                .send(ChatEvent::Front(FrontEvent::Message(link)))
                .ok();
        }
    }

    fn read_events(&mut self) {
        for event in self.back_rx.try_iter() {
            match event {
                BackEvent::PeerJoined((ip, name)) => {
                    self.rooms.peer_joined(ip, name);
                }
                BackEvent::PeerLeft(ip) => {
                    self.rooms.peer_left(ip);
                }
                BackEvent::Message(msg) => {
                    self.rooms.take_message(msg);
                }
            }
            self.rooms.recalculate_order();
        }
    }

    fn setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.style_mut().wrap = Some(false);
                        let size = ui.available_size_before_wrap().x * 0.075;
                        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                            font_id.size = size;
                        }

                        TextMessage::logo().draw(ui, None, &self.rooms.peers);
                    });
                    ui.label("");
                    self.user.draw_setup(ui);
                    ui.label("");
                    ui.heading(format!("Roomor v{}", env!("CARGO_PKG_VERSION")));
                    ui.visuals_mut().hyperlink_color = ui.visuals().text_color();
                    ui.hyperlink_to("by Roman Chumak", "http://www.p4ymak.su");
                    ui.hyperlink_to("Source Code", "https://www.github.com/p4ymak/roomor");
                });
            });
        });
    }

    fn init_chat(&mut self, ctx: &egui::Context) {
        if let Some(mut init) = self.chat_init.take() {
            let ctx = Notifier::new(
                ctx,
                self.audio_handle.clone(),
                self.notification_sound.clone(),
                self.notification_d_bus.clone(),
            );
            match init.prelude(&self.user) {
                Ok(_) => {
                    self.chat_handle = Some(thread::spawn(move || init.run(&ctx)));
                    self.pulse_handle = Some({
                        let tx = self.back_tx.clone();
                        thread::spawn(move || pulse(tx))
                    });
                }
                Err(err) => {
                    self.user.error_message = Some(format!("{err}"));
                    self.chat_init = Some(init);
                }
            }
        }
    }

    fn top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|h| {
                h.horizontal(|h| {
                    h.set_enabled(self.chat_init.is_none());
                    self.rooms.side_panel_toggle(h);
                });
                // Notifications
                atomic_button(&self.notification_sound, '🎵', h, "Sound Notifications");
                atomic_button(&self.notification_d_bus, '⚑', h, "Pop Notifications");
                self.settings_button(h);

                // Online Summary
                if self.chat_init.is_none() {
                    h.separator();
                    let summary = h.add(
                        egui::Label::new(format!(
                            "Online: {} / {}",
                            self.rooms
                                .peers
                                .0
                                .values()
                                .filter(|p| p.is_online())
                                .count(),
                            self.rooms.peers.0.len()
                        ))
                        .wrap(false),
                    );
                    if !self.rooms.peers.0.is_empty() {
                        summary.on_hover_ui(|h| {
                            for (ip, peer) in self.rooms.peers.0.iter() {
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
                    }
                    h.separator();
                    h.label(self.user.name()).on_hover_ui_at_pointer(|h| {
                        h.label(format!("{}:{}", self.user.ip(), self.user.port()));
                    });
                }
            });
        });
    }

    fn draw(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if !i.raw.hovered_files.is_empty() {
                debug!("HOVERED");
            }
            if !i.raw.dropped_files.is_empty() {
                debug!("DROPPED");
            }
            if let Some(path) = i.raw.dropped_files.first().and_then(|f| f.path.to_owned()) {
                debug!("Dropped file '{path:?}'");
                self.dispatch_file(&path);
            }
        });
        let mut font_size = 10.0;
        egui::TopBottomPanel::bottom("text intput")
            .resizable(false)
            .show(ctx, |ui| {
                font_size = ui.text_style_height(&egui::TextStyle::Body);
                self.rooms.draw_input(ui);
            });
        egui::SidePanel::left("Chats List")
            .min_width(font_size * 4.0)
            .max_width(ctx.input(|i| i.screen_rect.width()) * 0.5)
            .default_width(font_size * 8.0)
            .resizable(true)
            .show_animated(ctx, self.rooms.side_panel_opened, |ui| {
                self.rooms.draw_list(ui);
            });
        egui::SidePanel::left("Chats List Light")
            .exact_width(font_size)
            .resizable(false)
            .show_animated(ctx, !self.rooms.side_panel_opened, |ui| {
                self.rooms.draw_list(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            // FIXME
            // ui.interact(
            //     ui.available_rect_before_wrap(),
            //     egui::Id::new("context menu"),
            //     Sense::click(),
            // )
            // .context_menu(|ui| {
            //     if ui.button("Send File..").clicked() {
            //         if let Some(path) = rfd::FileDialog::new().pick_file() {
            //             self.dispatch_file(&path);
            //         }
            //     }
            // });

            self.rooms.draw_history(ui);
        });
    }

    fn settings_button(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("✱", |ui| {
            ui.horizontal(|h| {
                // GUI Settings
                h.label("UI Zoom");
                if h.button(egui::RichText::new("-").monospace()).clicked() {
                    let zoom = h.ctx().zoom_factor().max(0.5);
                    h.ctx().set_zoom_factor(zoom - ZOOM_STEP);
                    h.ctx().request_repaint();
                }
                if h.button(egui::RichText::new("=").monospace()).clicked() {
                    h.ctx().set_zoom_factor(1.0);
                    h.ctx().request_repaint();
                }
                if h.button(egui::RichText::new("+").monospace()).clicked() {
                    let zoom = h.ctx().zoom_factor().min(5.0);
                    h.ctx().set_zoom_factor(zoom + ZOOM_STEP);
                    h.ctx().request_repaint();
                }
                h.separator();
                egui::widgets::global_dark_light_mode_switch(h);
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
                    if self.chat_init.is_some() {
                        if !self.user.name().trim().is_empty() {
                            self.init_chat(ctx);
                        }
                    } else {
                        self.dispatch_text();
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    self.exit();
                }
                Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    ..
                } => {
                    self.rooms.side_panel_opened = !self.rooms.side_panel_opened;
                }

                Event::Key {
                    key: egui::Key::ArrowUp,
                    pressed: true,
                    ..
                } => self.rooms.list_go_up(),
                Event::Key {
                    key: egui::Key::ArrowDown,
                    pressed: true,
                    ..
                } => self.rooms.list_go_down(),

                Event::Key {
                    key: egui::Key::O,
                    modifiers: egui::Modifiers::CTRL,
                    pressed: true,
                    ..
                } => {
                    debug!("open file");

                    if self.chat_init.is_none() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.dispatch_file(&path);
                        }
                    }
                }
                _ => (),
            })
        })
    }
    fn exit(&mut self) {
        self.back_tx.send(ChatEvent::Front(FrontEvent::Exit)).ok();
        if let Some(handle) = self.chat_handle.take() {
            handle.join().unwrap();
        }
        let (front_tx, back_rx) = flume::unbounded();
        let chat = UdpChat::new(self.user.ip(), front_tx);
        let back_tx = chat.tx();
        self.user.init = true;
        self.chat_init = Some(chat);
        self.back_tx = back_tx.clone();
        self.back_rx = back_rx;
        self.rooms.back_tx = back_tx;
    }
}

impl Repaintable for egui::Context {
    fn request_repaint(&self) {
        self.request_repaint()
    }
}

fn atomic_button(value: &Arc<AtomicBool>, icon: char, ui: &mut egui::Ui, hover: &str) {
    let val = value.load(std::sync::atomic::Ordering::Relaxed);
    let mut icon = egui::RichText::new(icon).monospace();
    if !val {
        icon = icon.weak();
    }
    if ui.button(icon).on_hover_text_at_pointer(hover).clicked() {
        value.store(!val, std::sync::atomic::Ordering::Relaxed);
    }
}

fn drag_mask(ui: &mut egui::Ui, mask: &mut u8) {
    ui.add(
        egui::DragValue::new(mask)
            .speed(1)
            .custom_formatter(|m, _| {
                let net = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, m.min(32.0) as u8).expect("exists");
                let mask = net.netmask().octets();
                format!("{}.{}.{}.{}", mask[0], mask[1], mask[2], mask[3])
            })
            .custom_parser(|s| parse_netmask(s).map(|x| x as f64)),
    );
}
fn drag_ip(ui: &mut egui::Ui, ip: &Ipv4Addr) {
    ui.add_enabled(
        false,
        egui::DragValue::new(&mut 0)
            .speed(1)
            .custom_formatter(|_, _| {
                let mask = ip.octets();
                format!("{}.{}.{}.{}", mask[0], mask[1], mask[2], mask[3])
            }),
    );
}

fn pulse(tx: Sender<ChatEvent>) {
    let mut last_time = SystemTime::now();
    loop {
        sleep(TIMEOUT_CHECK);
        let now = SystemTime::now();
        if let Ok(delta) = now.duration_since(last_time) {
            if delta > TIMEOUT_CHECK {
                if delta > TIMEOUT_CHECK + TIMEOUT_SECOND {
                    debug!("Pulse Ping");
                    tx.send(ChatEvent::Front(FrontEvent::Ping(Recepients::All)))
                        .ok();
                }
                last_time = now;
            }
        }
    }
}

mod filetypes;
mod rooms;

use self::rooms::Rooms;
use crate::{
    chat::{
        limit_text,
        message::MAX_NAME_SIZE,
        networker::{get_my_ipv4, IP_MULTICAST_DEFAULT, TIMEOUT_ALIVE, TIMEOUT_CHECK},
        notifier::{Notifier, Repaintable},
        BackEvent, ChatEvent, FrontEvent, Recepients, TextMessage, UdpChat,
    },
    DONATION_LINK, HOMEPAGE_LINK, SOURCE_LINK,
};
use eframe::{
    egui::{self, *},
    CreationContext,
};
use flume::{Receiver, Sender};
use log::{debug, error};
use opener::{open, open_browser};
use rodio::{OutputStream, OutputStreamHandle};
use rooms::RoomAction;
use std::{
    net::Ipv4Addr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{atomic::AtomicBool, Arc},
    thread::{self, sleep, JoinHandle},
    time::SystemTime,
};

pub const ZOOM_STEP: f32 = 0.25;
pub const FONT_SCALE: f32 = 1.5;
pub const EMOJI_SCALE: f32 = 4.0;
pub const PUBLIC: &str = "Everyone";

pub struct UserSetup {
    pub init: bool,
    name: String,
    ip: Ipv4Addr,
    port: u16,
    multicast: Ipv4Addr,
    multicast_str: String,
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
            multicast: IP_MULTICAST_DEFAULT,
            multicast_str: format!("{}", IP_MULTICAST_DEFAULT),
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
    pub fn multicast(&self) -> Ipv4Addr {
        self.multicast
    }
    fn parse_multicast(&mut self) {
        if let Ok(ip) = Ipv4Addr::from_str(&self.multicast_str) {
            if ip.is_multicast() {
                self.multicast = ip;
            } else {
                self.multicast_str = self.multicast.to_string();
                self.error_message = Some("Non-multicast IP. Got Previous.".to_string());
            }
        } else {
            self.multicast_str = self.multicast.to_string();
            self.error_message = Some("Cant't parse IP. Got Previous.".to_string());
        }
    }
    pub fn draw_setup(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("Name");
            limit_text(&mut self.name, MAX_NAME_SIZE);
            ui.add(egui::TextEdit::singleline(&mut self.name).horizontal_align(Align::Center));
            ui.heading("IPv4");
            drag_ip(ui, &self.ip);
            ui.heading("Port");
            ui.add(egui::DragValue::new(&mut self.port));
            ui.heading("Multicast IPv4");
            let multicast = ui.add(
                egui::TextEdit::singleline(&mut self.multicast_str).horizontal_align(Align::Center),
            );
            if multicast.lost_focus() {
                self.parse_multicast();
            }
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
    downloads_path: PathBuf,
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
        let downloads_path = chat.downloads_path();
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
            downloads_path,
        }
    }
}

impl Roomor {
    pub fn new(cc: &CreationContext) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);

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
                        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                        let size = ui.available_size_before_wrap().x * 0.075;
                        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                            font_id.size = size;
                        }

                        TextMessage::logo().draw(ui, None, &self.rooms.peers);
                    });
                    ui.label("");
                    ui.vertical_centered_justified(|ui| {
                        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                            font_id.size *= FONT_SCALE;
                        }
                        self.user.draw_setup(ui);
                        let join_button = ui.add_enabled(
                            !self.user.name().trim().is_empty(),
                            egui::Button::new("Join"),
                        );
                        if self.user.init {
                            join_button.request_focus();
                            self.user.init = false;
                        }
                        if join_button.clicked() {
                            self.init_chat(ctx);
                        }
                    });
                    ui.label("");
                    ui.heading(format!("Roomor v{}", env!("CARGO_PKG_VERSION")));
                    ui.visuals_mut().hyperlink_color = ui.visuals().text_color();
                    ui.hyperlink_to("by Roman Chumak", HOMEPAGE_LINK);
                    ui.hyperlink_to("Source Code", SOURCE_LINK);
                    ui.hyperlink_to("Donate", DONATION_LINK);
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
                    self.chat_handle = Some(
                        thread::Builder::new()
                            .name("chat_back".to_string())
                            .spawn(move || init.run(&ctx))
                            .expect("can't build chat_back thread"),
                    );
                    self.pulse_handle = Some({
                        let tx = self.back_tx.clone();
                        thread::Builder::new()
                            .name("pulse".to_string())
                            .spawn(move || pulse(tx))
                            .expect("can't build pulse thread")
                    });
                }
                Err(err) => {
                    self.user.error_message = Some(format!("{err}"));
                    error!("{err}");
                    self.chat_init = Some(init);
                }
            }
        }
    }

    fn top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|h| {
                h.horizontal(|h| {
                    if self.chat_init.is_some() {
                        h.disable();
                    }
                    self.rooms.side_panel_toggle(h);
                });
                // Notifications
                atomic_button(
                    &self.notification_sound,
                    egui_phosphor::regular::MUSIC_NOTES,
                    h,
                    "Sound Notifications",
                );
                atomic_button(
                    &self.notification_d_bus,
                    egui_phosphor::regular::FLAG,
                    h,
                    "Pop Notifications",
                );
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
                        .wrap_mode(TextWrapMode::Extend),
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
        if !self.rooms.is_active_public() {
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
        }
        let mut font_size = 10.0;
        let max_height = ctx.input(|i| i.screen_rect.height()) * 0.5;
        egui::TopBottomPanel::bottom("text intput")
            .max_height(max_height)
            .resizable(false)
            .show(ctx, |ui| {
                font_size = ui.text_style_height(&egui::TextStyle::Body);
                egui::ScrollArea::vertical()
                    .min_scrolled_height(max_height)
                    .show(ui, |ui| {
                        self.rooms.draw_input(ui);
                    });
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
            .exact_width(font_size * 2.0)
            .resizable(false)
            .show_animated(ctx, !self.rooms.side_panel_opened, |ui| {
                self.rooms.draw_list(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| match self.rooms.draw_history(ui) {
            RoomAction::None => (),
            RoomAction::Clear => {
                self.rooms.get_mut_active().clear_history();
            }
            RoomAction::File => {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    if !self.rooms.is_active_public() {
                        self.dispatch_file(&path);
                    }
                }
            }
        });
    }

    fn settings_button(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(egui_phosphor::regular::GEAR_SIX, |ui| {
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
                egui::widgets::global_theme_preference_switch(h);
            });
            ui.separator();
            if ui
                .button(format!("{}  Clear History", egui_phosphor::regular::BROOM,))
                .clicked()
            {
                self.rooms.clear_history();
                ui.close_menu();
            }

            if ui
                .button(format!(
                    "{}  Open Downloads",
                    egui_phosphor::regular::FOLDER_OPEN,
                ))
                .clicked()
            {
                open(&self.downloads_path).ok();
                ui.close_menu();
            }
            if ui
                .button(format!("{}  Donate", egui_phosphor::regular::HEART,))
                .clicked()
            {
                open_browser(DONATION_LINK).ok();
                ui.close_menu();
            }
            if ui
                .button(format!("{}  Exit", egui_phosphor::regular::SIGN_OUT,))
                .clicked()
            {
                self.exit();
                ui.close_menu();
            }
        });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            i.raw.events.iter().for_each(|event| match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                    ..
                } => {
                    if self.chat_init.is_none() {
                        self.dispatch_text();
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    modifiers: Modifiers::SHIFT,
                    ..
                } => {
                    self.exit();
                }
                Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers: Modifiers::SHIFT,
                    ..
                } => {
                    self.rooms.side_panel_opened = !self.rooms.side_panel_opened;
                }

                Event::Key {
                    key: egui::Key::ArrowUp,
                    pressed: true,
                    modifiers: Modifiers::COMMAND,
                    ..
                } => self.rooms.list_go_up(),
                Event::Key {
                    key: egui::Key::ArrowDown,
                    pressed: true,
                    modifiers: Modifiers::COMMAND,
                    ..
                } => self.rooms.list_go_down(),

                Event::Key {
                    key: egui::Key::O,
                    modifiers: egui::Modifiers::COMMAND,
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
            handle.join().expect("can't join chat thread on exit");
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

fn atomic_button(value: &Arc<AtomicBool>, icon: &str, ui: &mut egui::Ui, hover: &str) {
    let val = value.load(std::sync::atomic::Ordering::Relaxed);
    let mut icon = egui::RichText::new(icon);
    if !val {
        icon = icon.weak();
    }
    if ui
        .add(egui::Button::new(icon).frame(false))
        .on_hover_text_at_pointer(hover)
        .clicked()
    {
        value.store(!val, std::sync::atomic::Ordering::Relaxed);
    }
}

// fn drag_mask(ui: &mut egui::Ui, mask: &mut u8) {
//     ui.add(
//         egui::DragValue::new(mask)
//             .speed(1)
//             .custom_formatter(|m, _| {
//                 let net = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, m.min(32.0) as u8).expect("exists");
//                 let mask = net.netmask().octets();
//                 format!("{}.{}.{}.{}", mask[0], mask[1], mask[2], mask[3])
//             })
//             .custom_parser(|s| parse_netmask(s).map(|x| x as f64)),
//     );
// }
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
                if delta > TIMEOUT_CHECK * 2 {
                    debug!("Pulse Ping");
                    tx.send(ChatEvent::Front(FrontEvent::Ping(Recepients::All)))
                        .ok();
                }
                last_time = now;
            }
        }
    }
}

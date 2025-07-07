#![allow(dead_code)]
mod filetypes;
mod rooms;
use self::rooms::Rooms;
use crate::chat::{
    limit_text,
    message::{new_id, DATA_LIMIT_BYTES, MAX_NAME_SIZE},
    networker::{get_my_ipv4, IP_MULTICAST_DEFAULT, PORT_DEFAULT, TIMEOUT_ALIVE, TIMEOUT_CHECK},
    notifier::{Notifier, Repaintable},
    peers::PeerId,
    BackEvent, ChatEvent, FrontEvent, TextMessage, UdpChat,
};
use directories::UserDirs;
use eframe::{
    egui::{self, *},
    CreationContext,
};
// use egui_keyboard::Keyboard;
#[cfg(target_os = "android")]
use egui_winit::winit::platform::android::activity::{
    input::{TextInputState, TextSpan},
    AndroidApp,
};
use flume::{Receiver, Sender};
use human_bytes::human_bytes;
use log::{debug, error};
use opener::{open, open_browser};
use rodio::{OutputStream, OutputStreamHandle};
use rooms::{text_height, RoomAction};
use std::{
    fs,
    net::Ipv4Addr,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU8},
        Arc,
    },
    thread::{self, sleep, JoinHandle},
    time::SystemTime,
};

const BUFFER_SIZE_DEFAULT: u8 = 13; // 2^X * Shard
const BUFFER_SIZE_MAX: u8 = 24;
pub const ZOOM_STEP: f32 = 0.25;
pub const FONT_SCALE: f32 = 1.5;
pub const EMOJI_SCALE: f32 = 4.0;
pub const PUBLIC: &str = "Everyone";
pub const HOMEPAGE_LINK: &str = "https://www.p4ymak.su";
pub const SOURCE_LINK: &str = "https://www.github.com/p4ymak/roomor";
pub const DONATION_LINK: &str = "https://www.donationalerts.com/r/p4ymak";

pub struct UserSetup {
    pub init: bool,
    name: String,
    ip: Ipv4Addr,
    id: PeerId,
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
        let id = PeerId::new(&whoami::username(), &whoami::devicename());
        UserSetup {
            init: true,
            name: whoami::username(),
            id,
            ip,
            port: PORT_DEFAULT,
            multicast: IP_MULTICAST_DEFAULT,
            multicast_str: IP_MULTICAST_DEFAULT.to_string(),
            error_message,
        }
    }
}
impl UserSetup {
    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn id(&self) -> PeerId {
        self.id
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

            // ui.heading("Interface");
            // egui::ComboBox::from_id_salt("interface")
            //     .selected_text(
            //         self.interface
            //             .as_ref()
            //             .map(|i| i.name.to_string())
            //             .unwrap_or("No Net Interface".to_string()),
            //     )
            //     .show_ui(ui, |ui| {
            //         for interface in self.interfaces.iter() {
            //             ui.selectable_value(
            //                 &mut self.interface,
            //                 Some(interface.clone()),
            //                 &interface.name,
            //             );
            //         }
            //     });
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
    buffer_size: Arc<AtomicU8>,
    back_rx: Receiver<BackEvent>,
    back_tx: Sender<ChatEvent>,
    last_time: SystemTime,
    downloads_path: PathBuf,
    #[cfg(target_os = "android")]
    android_app: Option<AndroidApp>,
    // keyboard: Keyboard,
}

impl eframe::App for Roomor {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.back_tx.send(ChatEvent::Front(FrontEvent::Exit)).ok();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.top_panel(ctx);

        #[cfg(target_os = "android")]
        if let Some(app) = &self.android_app {
            if ctx.wants_keyboard_input() {
                app.show_soft_input(true);
            }
            if self.rooms.get_active().mode == rooms::TextMode::Icon {
                app.hide_soft_input(true);
            }
        }

        if self.chat_init.is_some() {
            self.setup(ctx);
        } else {
            self.read_events();
            self.keep_alive();
            self.handle_dnd_files(ctx);
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

    // Handle Android Soft Input FIXME kinda hacky =/
    #[cfg(target_os = "android")]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(app) = &self.android_app {
            let text = app.text_input_state().text.to_string();
            let event = match text.as_str() {
                t if t.is_empty() => Event::Key {
                    key: Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
                t if t.contains("\n") => Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
                text => Event::Text(text[1..].to_string()),
            };

            raw_input.events.push(event);
            let state = TextInputState {
                text: String::from(" "),
                selection: TextSpan { start: 1, end: 1 },
                compose_region: None,
            };
            app.set_text_input_state(state);
        }
    }
}

impl Roomor {
    fn default(downloads_path: PathBuf) -> Self {
        let (_audio, audio_handler) = match OutputStream::try_default() {
            Ok((audio, audio_handler)) => (Some(audio), Some(audio_handler)),
            Err(_) => (None, None),
        };
        let (front_tx, back_rx) = flume::unbounded();
        let notification_sound = Arc::new(AtomicBool::new(true));
        let notification_d_bus = Arc::new(AtomicBool::new(true));
        let buffer_size = Arc::new(AtomicU8::new(BUFFER_SIZE_DEFAULT));
        let user = UserSetup::default();

        let chat = UdpChat::new(
            user.ip(),
            front_tx,
            downloads_path.clone(),
            buffer_size.clone(),
        );

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
            buffer_size,
            back_tx,
            back_rx,
            last_time: SystemTime::now(),
            downloads_path,
            #[cfg(target_os = "android")]
            android_app: None,
            // keyboard: Keyboard::default(),
        }
    }

    pub fn new(cc: &CreationContext) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        let downloads_path = UserDirs::new()
            .unwrap()
            .download_dir()
            .unwrap()
            .join("Roomor");
        fs::create_dir_all(&downloads_path).ok();

        Roomor::default(downloads_path)
    }
    #[cfg(target_os = "android")]
    pub fn new_android(cc: &CreationContext, app: AndroidApp) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.enable_accesskit();
        // if let Some(native_ppp) = cc.egui_ctx.native_pixels_per_point() {
        //     error!("NATIVE PPP: {native_ppp}");
        //     cc.egui_ctx.set_pixels_per_point(native_ppp.round());
        // }

        let downloads_path = PathBuf::from("/storage/emulated/0/Download").join("Roomor"); // FIXME hardcode
        fs::create_dir_all(&downloads_path).ok();
        Roomor {
            android_app: Some(app),
            ..Roomor::default(downloads_path)
        }
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
        self.rooms.dispatch_text();
    }

    fn dispatch_files(&mut self, paths: &[PathBuf]) {
        self.rooms.dispatch_files(paths);
    }

    fn read_events(&mut self) {
        for event in self.back_rx.try_iter() {
            match event {
                BackEvent::PeerJoined(ip, id, name) => {
                    self.rooms.peer_joined(ip, id, name);
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
                        let size = ui.available_size_before_wrap().x * 0.074;
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
                #[cfg(target_os = "android")]
                scale_text(h, 2.0);
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
                self.draw_settings_button(h);

                // Online Summary
                if self.chat_init.is_none() {
                    h.separator();
                    #[cfg(target_os = "android")]
                    scale_text(h, 0.5);
                    let summary = h.add(
                        egui::Label::new(format!(
                            "Online: {} / {}",
                            self.rooms
                                .peers
                                .ids
                                .values()
                                .filter(|p| p.is_online())
                                .count(),
                            self.rooms.peers.ids.len()
                        ))
                        .wrap_mode(TextWrapMode::Extend),
                    );
                    if !self.rooms.peers.ids.is_empty() {
                        summary.on_hover_ui(|h| {
                            for peer in self.rooms.peers.ids.values() {
                                let mut label = egui::RichText::new(peer.display_name());
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
        let mut font_size = 10.0;
        let max_height = ctx.input(|i| i.screen_rect.height()) * 0.5;
        egui::TopBottomPanel::bottom("text intput")
            .max_height(max_height)
            .resizable(false)
            .show_separator_line(true)
            .show(ctx, |ui| {
                font_size = text_height(ui);
                if self.rooms.get_active().mode == rooms::TextMode::Icon {
                    egui::ScrollArea::vertical()
                        .min_scrolled_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            self.rooms.draw_input(ui);
                        });
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_enabled_ui(self.rooms.is_able_to_send(), |ui| {
                            self.draw_input_buttons(ui);
                        });
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(max_height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                self.rooms.draw_input(ui);
                            });
                    });
                }
            });
        egui::SidePanel::left("Chats List")
            .min_width(font_size * 4.0)
            .max_width(ctx.input(|i| i.screen_rect.width()) * 0.5)
            .default_width(font_size * 8.0)
            .resizable(true)
            .show_separator_line(true)
            .show_animated(ctx, self.rooms.side_panel_opened, |ui| {
                self.rooms.draw_list(ui);
            });
        if !self.rooms.side_panel_opened {
            egui::SidePanel::left("Chats List Light")
                .exact_width(font_size * 2.0)
                .resizable(false)
                .show_separator_line(true)
                .show_animated(ctx, !self.rooms.side_panel_opened, |ui| {
                    self.rooms.draw_list(ui);
                });
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.rooms.draw_history(ui) {
            RoomAction::None => (),
            RoomAction::File =>
            {
                #[cfg(not(target_os = "android"))]
                if !self.rooms.is_active_public() {
                    self.pick_files();
                }
            }
        });
    }

    #[cfg(not(target_os = "android"))]
    fn pick_files(&self) {
        let tx = self.back_tx.clone();
        let peer_id = self.rooms.active_chat();
        thread::Builder::new()
            .name("file_picker".to_string())
            .spawn(move || {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    let mut id = new_id();
                    for path in paths {
                        if let Some(link) = Rooms::compose_file(peer_id, id, &path) {
                            tx.send(ChatEvent::Front(FrontEvent::Message(link))).ok();
                        }
                        id += 1;
                    }
                }
            })
            .expect("file picker thread failed");
    }

    fn draw_input_buttons(&mut self, ui: &mut egui::Ui) {
        let active_room = self.rooms.get_active();

        let is_text_empty = active_room.input.is_empty();
        ui.horizontal(|ui| {
            // FIXME hardcode scale for vertical center
            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                font_id.size *= 1.2 * FONT_SCALE;
            }
            #[cfg(not(target_os = "android"))]
            if is_text_empty
                && !self.rooms.is_active_public()
                && ui
                    .add(
                        egui::Button::new(RichText::new(egui_phosphor::regular::PAPERCLIP))
                            .frame(false),
                    )
                    .clicked()
            {
                self.pick_files();
            }
            if is_text_empty
                && ui
                    .add(
                        egui::Button::new(RichText::new(egui_phosphor::regular::SMILEY))
                            .frame(false),
                    )
                    .clicked()
            {
                self.rooms.get_mut_active().input = String::from("/");
            }

            if !is_text_empty
                && ui
                    .add(
                        egui::Button::new(RichText::new(egui_phosphor::regular::PAPER_PLANE_RIGHT))
                            .frame(false),
                    )
                    .clicked()
            {
                self.dispatch_text();
            }
        });
    }

    fn draw_settings_button(&mut self, ui: &mut egui::Ui) {
        ui.visuals_mut().button_frame = false;

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
            self.draw_buffer_settings(ui);
            ui.separator();
            if ui
                .button(format!("{}  Clear History", egui_phosphor::regular::BROOM))
                .clicked()
            {
                self.rooms.clear_history();
                ui.close_menu();
            }

            if ui
                .button(format!(
                    "{}  Open Downloads",
                    egui_phosphor::regular::FOLDER_OPEN
                ))
                .clicked()
            {
                open(&self.downloads_path).ok();
                ui.close_menu();
            }
            if ui
                .button(format!("{}  Donate", egui_phosphor::regular::HEART))
                .clicked()
            {
                open_browser(DONATION_LINK).ok();
                ui.close_menu();
            }
            if ui
                .button(format!("{}  Exit", egui_phosphor::regular::SIGN_OUT))
                .clicked()
            {
                self.exit();
                ui.close_menu();
            }
        });
    }
    fn draw_buffer_settings(&mut self, ui: &mut egui::Ui) {
        let formatter = |num, _| -> String {
            let size = 2_usize.pow(num as u32) * DATA_LIMIT_BYTES;
            human_bytes(size as f64)
        };
        let ordering = std::sync::atomic::Ordering::Relaxed;
        let mut buffer_size = self.buffer_size.load(ordering);
        ui.horizontal(|h| {
            h.label("Buffer size");
            h.add(
                DragValue::new(&mut buffer_size)
                    .range(1..=BUFFER_SIZE_MAX)
                    .custom_formatter(formatter),
            );
        });
        self.buffer_size.store(buffer_size, ordering);
    }

    fn handle_dnd_files(&mut self, ctx: &egui::Context) {
        if !self.rooms.is_active_public() {
            ctx.input(|i| {
                if !i.raw.hovered_files.is_empty() {
                    debug!("HOVERED");
                }
                if !i.raw.dropped_files.is_empty() {
                    let paths = i
                        .raw
                        .dropped_files
                        .iter()
                        .filter_map(|f| f.path.clone())
                        .collect::<Vec<PathBuf>>();
                    self.dispatch_files(&paths);
                }
            });
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        ctx.input_mut(|i| {
            #[cfg(not(target_os = "android"))]
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O)) {
                debug!("open file");

                if self.chat_init.is_none() && !self.rooms.is_active_public() {
                    self.pick_files();
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND,
                egui::Key::ArrowUp,
            )) {
                self.rooms.list_go_up();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND,
                egui::Key::ArrowDown,
            )) {
                self.rooms.list_go_down();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND,
                egui::Key::Escape,
            )) {
                self.exit();
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Tab)) {
                self.rooms.side_panel_opened = !self.rooms.side_panel_opened;
            }

            i.events.iter().for_each(|event| match event {
                #[cfg(not(target_os = "android"))]
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
                    modifiers: Modifiers::NONE,
                    ..
                } => {
                    let active_room = self.rooms.get_mut_active();
                    if active_room.mode == rooms::TextMode::Icon {
                        active_room.input.clear();
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
        let downloads_path = self.downloads_path.clone();

        *self = Roomor {
            #[cfg(target_os = "android")]
            android_app: self.android_app.clone(),
            ..Roomor::default(downloads_path)
        };
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
                    tx.send(ChatEvent::Front(FrontEvent::Ping(PeerId::PUBLIC)))
                        .ok();
                }
                last_time = now;
            }
        }
    }
}

fn scale_text(ui: &mut Ui, factor: f32) {
    ui.style_mut()
        .text_styles
        .values_mut()
        .for_each(|s| s.size *= factor);
}

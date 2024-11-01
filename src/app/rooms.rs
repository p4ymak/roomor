use super::{filetypes::file_ico, EMOJI_SCALE, FONT_SCALE, PUBLIC};
use crate::{
    chat::{
        file::FileLink,
        limit_text,
        message::{Id, MAX_EMOJI_SIZE},
        peers::{Peer, PeersMap, Presence},
        ChatEvent, Content, FrontEvent, Recepients, TextMessage,
    },
    emoji::EMOJI_LIST,
};
use eframe::{
    egui::{self, KeyboardShortcut, Modifiers, Rounding, Stroke},
    emath::Align2,
};
use flume::Sender;
use human_bytes::human_bytes;
use std::{collections::BTreeMap, net::Ipv4Addr, path::Path, sync::Arc, time::SystemTime};
use timediff::TimeDiff;

#[derive(PartialEq, Eq)]
pub enum TextMode {
    Normal,
    Big,
    Icon,
}

#[derive(PartialEq, Eq)]
pub enum RoomAction {
    None,
    File,
}

pub struct Rooms {
    active_chat: Recepients,
    pub peers: PeersMap,
    order: Vec<Recepients>,
    chats: BTreeMap<Recepients, ChatHistory>,
    pub side_panel_opened: bool,
    pub back_tx: Sender<ChatEvent>,
}

impl Rooms {
    pub fn new(back_tx: Sender<ChatEvent>) -> Self {
        let mut chats = BTreeMap::new();
        chats.insert(Recepients::All, ChatHistory::new(Recepients::All));
        Rooms {
            active_chat: Recepients::All,
            peers: PeersMap::new(),
            order: vec![],
            chats,
            side_panel_opened: true,
            back_tx,
        }
    }

    pub fn clear_history(&mut self) {
        self.chats.values_mut().for_each(|h| h.clear_history());
    }

    pub fn get_mut_public(&mut self) -> &mut ChatHistory {
        self.chats.get_mut(&Recepients::All).expect("Public Exists")
    }

    pub fn get_mut_private(&mut self, ip: Ipv4Addr) -> &mut ChatHistory {
        self.chats
            .entry(Recepients::One(ip))
            .or_insert(ChatHistory::new(Recepients::One(ip)))
    }

    pub fn get_mut_active(&mut self) -> &mut ChatHistory {
        self.chats
            .get_mut(&self.active_chat)
            .expect("Active Exists")
    }

    pub fn get_active(&self) -> &ChatHistory {
        self.chats.get(&self.active_chat).expect("Active Exists")
    }

    pub fn is_able_to_send(&self) -> bool {
        match self.active_chat {
            Recepients::One(_) => true,
            _ => self.peers.0.values().any(|p| p.is_online()),
        }
    }

    pub fn is_active_public(&self) -> bool {
        self.active_chat.is_public()
    }

    pub fn compose_message(&mut self) -> Option<TextMessage> {
        if !self.is_able_to_send() {
            return None;
        }
        let chat = self.get_mut_active();
        let mut trimmed = chat.input.trim().to_string();
        if chat.mode == TextMode::Big {
            trimmed = trimmed.replace('\n', "");
        }
        (!trimmed.is_empty()).then_some({
            chat.input.clear();
            let content = match chat.mode {
                TextMode::Normal => Content::Text(trimmed),
                TextMode::Big => Content::Big(trimmed),
                TextMode::Icon => Content::Icon(trimmed),
            };
            TextMessage::out_message(content, chat.recepients)
        })
    }
    pub fn compose_file(&mut self, id: Id, path: &Path) -> Option<TextMessage> {
        let link = Arc::new(FileLink::from_path(id, path)?);
        Some(TextMessage::out_message(
            Content::FileLink(link),
            self.active_chat,
        ))
    }

    pub fn peer_joined(&mut self, ip: Ipv4Addr, name: Option<String>) {
        if self.peers.peer_joined(ip, name.as_ref()) {
            let msg = TextMessage::in_enter(ip, name.unwrap_or(ip.to_string()));
            self.get_mut_public().history.push(msg.clone());
            self.get_mut_private(ip).history.push(msg);
        }
    }

    pub fn peer_left(&mut self, ip: Ipv4Addr) {
        self.get_mut_public().history.push(TextMessage::in_exit(ip));
        self.get_mut_private(ip)
            .history
            .push(TextMessage::in_exit(ip));
        self.peers.peer_exited(ip);
    }

    pub fn take_message(&mut self, msg: TextMessage) {
        let recepients = Recepients::from_ip(msg.ip(), msg.is_public());
        let target_chat = self
            .chats
            .entry(recepients)
            .or_insert(ChatHistory::new(recepients));
        if matches!(msg.content(), Content::Seen) {
            if let Some(found) = target_chat.history.iter_mut().rfind(|m| m.id() == msg.id()) {
                if let Content::FileLink(link) = found.content() {
                    link.set_ready();
                }
                match recepients {
                    Recepients::One(_) => found.seen_private(),
                    _ => found.seen_public_by(msg.ip()),
                }
            }
        } else {
            target_chat.history.push(msg);
            if recepients != self.active_chat {
                target_chat.unread += 1;
            }
        }
    }

    pub fn recalculate_order(&mut self) {
        let mut order = self
            .chats
            .values()
            .filter(|v| v.recepients != Recepients::All)
            .map(|c| (c.history.last().map(|m| m.time()), c.recepients))
            .collect::<Vec<_>>();
        order.sort_by(|a, b| b.0.cmp(&a.0));
        self.order = order.into_iter().map(|o| o.1).collect();
    }

    pub fn _has_unread(&self) -> bool {
        self.chats.values().any(|c| c.unread > 0)
    }

    pub fn draw_history(&self, ui: &mut egui::Ui) -> RoomAction {
        if !self.side_panel_opened {
            ui.vertical_centered(|ui| {
                let name = match self.active_chat {
                    Recepients::One(ip) => self.peers.0.get(&ip).expect("Peer exists").rich_name(),
                    _ => self.peers.rich_public(),
                };
                ui.label(name);
            });
            ui.separator();
        }
        self.get_active().draw_history(&self.peers, ui)
    }

    pub fn draw_list(&mut self, ui: &mut egui::Ui) {
        space(ui, 0.2);
        if self
            .chats
            .get_mut(&Recepients::All)
            .expect("Public exists")
            .draw_list_entry(
                ui,
                &mut self.active_chat,
                &self.peers,
                self.side_panel_opened,
            )
        {
            self.set_active(Recepients::All);
        }
        space(ui, 0.5);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut clicked = None;
            for recepient in self.order.iter() {
                if self
                    .chats
                    .get_mut(recepient)
                    .expect("Private Exists")
                    .draw_list_entry(
                        ui,
                        &mut self.active_chat,
                        &self.peers,
                        self.side_panel_opened,
                    )
                {
                    clicked = Some(recepient);
                }
            }
            if let Some(recepient) = clicked {
                self.set_active(*recepient);
            }
            ui.label("");
        });
    }

    pub fn draw_input(&mut self, ui: &mut egui::Ui) {
        let status = self.peers.online_status(self.active_chat);
        self.chats
            .get_mut(&self.active_chat)
            .expect("Active Exists")
            .draw_input(ui, status);
    }
    pub fn side_panel_toggle(&mut self, ui: &mut egui::Ui) {
        let side_ico = egui_phosphor::regular::SIDEBAR;
        let side_ico = egui::RichText::new(side_ico);
        if ui.add(egui::Button::new(side_ico).frame(false)).clicked() {
            self.side_panel_opened = !self.side_panel_opened;
        }
    }

    pub fn list_go_up(&mut self) {
        let active = if self.active_chat == Recepients::All {
            self.order.last().cloned().unwrap_or_default()
        } else {
            let active_id = self
                .order
                .iter()
                .position(|k| k == &self.active_chat)
                .unwrap_or_default();
            match active_id {
                0 => Recepients::All,
                _ => self
                    .order
                    .get(active_id.saturating_sub(1))
                    .unwrap_or(&Recepients::All)
                    .to_owned(),
            }
        };
        self.set_active(active);
    }

    pub fn list_go_down(&mut self) {
        let active = if self.active_chat == Recepients::All {
            self.order.first().cloned().unwrap_or_default()
        } else {
            let active_id = self
                .order
                .iter()
                .position(|k| k == &self.active_chat)
                .unwrap_or_default();
            self.order
                .get(active_id.saturating_add(1))
                .unwrap_or(&Recepients::All)
                .to_owned()
        };
        self.set_active(active);
    }

    fn set_active(&mut self, recepients: Recepients) {
        self.active_chat = recepients;
        if let Recepients::All = recepients {
            self.peers.0.values_mut().for_each(|p| {
                if !p.is_offline() {
                    p.set_presence(Presence::Unknown);
                }
            });
        }
        self.back_tx
            .send(ChatEvent::Front(FrontEvent::Ping(recepients)))
            .ok();

        self.get_mut_active().unread = 0;
    }

    pub fn update_peers_online(&mut self, tx: &Sender<ChatEvent>) {
        self.peers.0.values_mut().for_each(|p| {
            if !p.is_offline() {
                p.set_presence(Presence::Unknown);
            }
        });
        tx.send(ChatEvent::Front(FrontEvent::Ping(Recepients::All)))
            .ok();
    }
}

pub struct ChatHistory {
    recepients: Recepients,
    pub mode: TextMode,
    pub input: String,
    history: Vec<TextMessage>,
    unread: usize,
}

impl ChatHistory {
    pub fn new(recepients: Recepients) -> Self {
        ChatHistory {
            recepients,
            mode: TextMode::Normal,
            input: String::new(),
            history: vec![],
            unread: 0,
        }
    }

    pub fn font_multiply(&self, ui: &mut egui::Ui) {
        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
            let emoji_scale = match self.mode {
                TextMode::Normal => 1.0,
                TextMode::Big => 4.0,
                TextMode::Icon => 2.0,
            };

            font_id.size *= FONT_SCALE * emoji_scale;
        }
    }
    pub fn draw_history(&self, peers: &PeersMap, ui: &mut egui::Ui) -> RoomAction {
        let mut action = RoomAction::None;
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if !self.recepients.is_public() {
                    ui.interact(
                        ui.clip_rect(),
                        egui::Id::new("context menu"),
                        egui::Sense::click(),
                    )
                    .context_menu(|ui| {
                        if ui
                            .button(format!(
                                "{}  Send Files",
                                egui_phosphor::regular::FILE_ARROW_UP
                            ))
                            .clicked()
                        {
                            action = RoomAction::File;
                            ui.close_menu();
                        }
                        // if ui
                        //     .small_button(format!("{}  Clear History", egui_phosphor::regular::BROOM))
                        //     .clicked()
                        // {
                        //     action = RoomAction::Clear;
                        //     ui.close_menu();
                        // }
                    });
                }
                self.history.iter().for_each(|m| {
                    let peer = m.is_incoming().then_some(peers.0.get(&m.ip())).flatten();
                    m.draw(ui, peer, peers);
                });
            });
        action
    }
    pub fn draw_input(&mut self, ui: &mut egui::Ui, status: Presence) {
        ui.visuals_mut().clip_rect_margin = 0.0;
        let chat_interactive = !(self.recepients == Recepients::All && status != Presence::Online);

        self.mode = if self.input.starts_with(' ') {
            TextMode::Big
        } else if self.input.starts_with('/') {
            TextMode::Icon
        } else {
            TextMode::Normal
        };

        if self.mode == TextMode::Big {
            limit_text(&mut self.input, MAX_EMOJI_SIZE);
        }
        self.font_multiply(ui);

        let len = self.input.len();
        if len > 0 && self.mode == TextMode::Big {
            let y = ui.max_rect().min.y;
            let rect = ui.clip_rect();
            ui.painter().hline(
                rect.min.x..=(rect.max.x * (len as f32 / MAX_EMOJI_SIZE as f32)),
                y,
                ui.style().visuals.widgets.inactive.fg_stroke,
            );
        }
        if self.mode == TextMode::Icon {
            self.draw_input_emoji(ui);
        } else {
            let text_input = ui.add(
                egui::TextEdit::multiline(&mut self.input)
                    .frame(false)
                    .desired_rows(if self.mode == TextMode::Normal { 4 } else { 1 })
                    .desired_width(ui.available_rect_before_wrap().width())
                    .interactive(chat_interactive)
                    .cursor_at_end(true)
                    .return_key(Some(KeyboardShortcut::new(
                        Modifiers::SHIFT,
                        egui::Key::Enter,
                    ))),
            );
            text_input.request_focus();
            if text_input.secondary_clicked() && chat_interactive {
                self.input = "/".to_string();
            }
        }
    }

    fn draw_input_emoji(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for emoji in EMOJI_LIST {
                let icon = ui.add(egui::Button::new(emoji).frame(false));
                if icon.clicked() {
                    self.input = emoji.to_string();
                    emulate_enter(ui);
                }
                if icon.secondary_clicked() {
                    self.input.clear();
                }
            }
            if ui.response().secondary_clicked() {
                self.input.clear();
            }
        });
    }

    fn draw_list_entry(
        &mut self,
        ui: &mut egui::Ui,
        active_chat: &mut Recepients,
        peers: &PeersMap,
        side_panel_opened: bool,
    ) -> bool {
        let (name, color) = match self.recepients {
            Recepients::One(ip) => {
                if let Some(peer) = peers.0.get(&ip) {
                    (
                        peers.get_display_name(ip),
                        if peer.is_online() {
                            ui.visuals().strong_text_color()
                        } else if peer.is_offline() {
                            ui.visuals().weak_text_color()
                        } else {
                            ui.visuals().text_color()
                        },
                    )
                } else {
                    return false;
                }
            }
            _ => (PUBLIC.to_string(), {
                if peers.any_online() {
                    ui.visuals().strong_text_color()
                } else if peers.all_offline() {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().text_color()
                }
            }),
        };

        let max_rect = ui.max_rect();
        let font_size = text_height(ui);
        let font_id = egui::FontId::proportional(font_size);

        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(max_rect.width(), font_size * 1.5),
            egui::Sense::click(),
        );

        let is_active = *active_chat == self.recepients;
        let active_fg = ui.visuals().widgets.hovered.fg_stroke;
        let inactive_fg = ui.visuals().widgets.inactive.fg_stroke;
        let stroke_width = stroke_width(ui);
        let stroke = if response.hovered() || is_active {
            Stroke::new(stroke_width * 1.5, active_fg.color)
        } else {
            Stroke::new(stroke_width, inactive_fg.color.linear_multiply(0.5))
        };
        response.context_menu(|ui| {
            if ui
                .small_button(format!("{}  Clear History", egui_phosphor::regular::BROOM))
                .clicked()
            {
                self.clear_history();
                ui.close_menu();
            }
        });
        let clicked = response.clicked();

        let rounding = Rounding {
            nw: rounding(ui) * 2.0,
            ne: 0.0,
            sw: rounding(ui) * 2.0,
            se: 0.0,
        };

        painter.text(
            painter.clip_rect().left_center() + egui::Vec2::new(font_id.size, 0.0),
            Align2::LEFT_CENTER,
            name.clone(),
            font_id.clone(),
            color,
        );
        painter.rect_stroke(
            painter
                .clip_rect()
                .expand(-stroke_width)
                .shrink2(egui::Vec2::new(stroke_width, 0.0)),
            rounding,
            stroke,
        );

        if self.unread > 0 {
            painter.vline(
                painter.clip_rect().right() - stroke_width,
                painter.clip_rect().y_range(),
                Stroke::new(stroke_width * 4.0, active_fg.color),
            );
        }

        // Hover
        let mut hover_lines = vec![];
        if !side_panel_opened {
            hover_lines.push(name);
        }
        if self.unread > 0 {
            hover_lines.push(format!("Unread: {}", self.unread));
        }
        if let Some(msg) = self.history.last() {
            if let Some(ago) = pretty_ago(msg.time()) {
                hover_lines.push(format!("Last message {}", ago));
            }
        }
        if let Recepients::One(ip) = self.recepients {
            let peer = peers.0.get(&ip).expect("Peer exists");
            if let Some(ago) = pretty_ago(peer.last_time()) {
                hover_lines.push(format!("Last seen {}", ago));
            }
            hover_lines.push(format!("{ip}"));
        }
        if !hover_lines.is_empty() {
            response.on_hover_ui_at_pointer(|ui| {
                for line in hover_lines {
                    ui.label(line);
                }
            });
        }
        clicked
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.unread = 0;
    }
}

impl Peer {
    fn rich_name(&self) -> egui::RichText {
        let mut label = egui::RichText::new(self.display_name());
        if self.is_offline() {
            label = label.weak();
        } else if self.is_online() {
            label = label.strong();
        }
        label
    }
}

impl TextMessage {
    pub fn draw(&self, ui: &mut egui::Ui, incoming: Option<&Peer>, peers: &PeersMap) {
        let align = if self.is_incoming() {
            egui::Align::Min
        } else {
            egui::Align::Max
        };
        let ui_width = ui.available_width() - text_height(ui);
        ui.with_layout(egui::Layout::top_down(align), |line| {
            line.set_max_width(ui_width);
            let mut rounding =
                Rounding::same(rounding(line) * line.style().visuals.window_stroke.width);
            if self.is_seen() {
                if self.is_incoming() {
                    rounding.sw = 0.0;
                } else {
                    rounding.se = 0.0;
                }
            }
            let frame = egui::Frame::group(line.style())
                .outer_margin(line.style().visuals.window_stroke.width)
                .rounding(rounding)
                .stroke(Stroke::new(
                    stroke_width(line),
                    line.style().visuals.widgets.inactive.fg_stroke.color,
                ))
                .show(line, |ui| {
                    if let Some(peer) = incoming {
                        ui.vertical(|v| match self.content() {
                            Content::Ping(_) => {
                                v.horizontal(|h| {
                                    h.label(peer.rich_name())
                                        .on_hover_text_at_pointer(peer.ip().to_string());

                                    h.label("joined..");
                                });
                            }
                            Content::Exit => {
                                v.horizontal(|h| {
                                    h.label(peer.rich_name())
                                        .on_hover_text_at_pointer(peer.ip().to_string());

                                    h.label("left..");
                                });
                            }
                            _ => {
                                if self.is_public() {
                                    v.label(peer.rich_name())
                                        .on_hover_text_at_pointer(peer.ip().to_string());
                                }

                                self.draw_content(v);
                            }
                        });
                    } else {
                        self.draw_content(ui);
                    }
                });

            frame.response.on_hover_ui_at_pointer(|ui| {
                ui.label(pretty_ago(self.time()).unwrap_or_default());
                let seen_by = self.is_seen_by();
                // if self.content()
                if !seen_by.is_empty() {
                    ui.label("");
                    ui.label("Received by:");
                    for ip in seen_by.iter() {
                        if let Some(peer) = peers.0.get(ip) {
                            ui.label(peer.rich_name());
                        }
                    }
                }
                if let Content::FileLink(link) = self.content() {
                    if link.is_ready() {
                        let bandwidth = link.bandwidth();
                        ui.label(format!("{}/s", human_bytes(bandwidth as f32)));
                    }
                }
            });
        });
    }

    #[inline]
    pub fn draw_content(&self, ui: &mut eframe::egui::Ui) {
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
            Content::Big(content) | Content::Icon(content) => {
                for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                    font_id.size *= FONT_SCALE * EMOJI_SCALE;
                }
                ui.label(content);
            }
            Content::FileLink(link) => {
                let file_ico = file_ico(&link.path, ui);
                let font_size = text_height(ui);
                if link.is_aborted() && !link.is_ready() {
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::FILE_DASHED)
                            .size(font_size * 4.0),
                    );
                } else if link.is_ready() {
                    if ui.link(file_ico).clicked() {
                        opener::open(&link.path).ok();
                    }
                } else {
                    ui.label(file_ico);
                }
                ui.label(&link.name);
                ui.label(human_bytes(link.size as f64));
                let width = ui.min_rect().width();
                if !link.is_aborted() {
                    if link.is_ready() {
                        #[cfg(debug_assertions)]
                        {
                            let bandwidth = link.bandwidth();
                            ui.label(format!("{}/s", human_bytes(bandwidth as f32)));
                        }
                    } else {
                        let rounding =
                            Rounding::same(rounding(ui) * ui.style().visuals.window_stroke.width);
                        ui.add(
                            egui::ProgressBar::new(link.progress())
                                .rounding(rounding)
                                .desired_width(width)
                                .show_percentage(),
                        );
                        if ui.link("Cancel").clicked() {
                            link.abort();
                        }
                    }
                }
            }
            _ => (),
        }
    }
}
pub fn text_height(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body)
}
fn rounding(ui: &mut egui::Ui) -> f32 {
    text_height(ui) * 0.75
}
fn stroke_width(ui: &mut egui::Ui) -> f32 {
    text_height(ui) * 0.1
}

pub fn pretty_ago(ts: SystemTime) -> Option<String> {
    let delta = SystemTime::now().duration_since(ts).ok()?.as_secs();
    TimeDiff::to_diff(format!("{delta}s"))
        .locale(String::from("en-US"))
        .ok()?
        .parse()
        .ok()
}

pub fn emulate_enter(ui: &egui::Ui) {
    let event = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    ui.ctx().input_mut(|w| w.raw.events.push(event));
}

pub fn space(ui: &mut egui::Ui, value: f32) {
    let height = text_height(ui) * value;
    ui.allocate_exact_size(
        egui::Vec2::new(0., height),
        egui::Sense::focusable_noninteractive(),
    );
}

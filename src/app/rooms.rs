use crate::chat::{
    file::{FileLink, FileStatus},
    limit_text,
    message::{MAX_EMOJI_SIZE, MAX_TEXT_SIZE},
    peers::{Peer, PeersMap, Presence},
    ChatEvent, Content, FrontEvent, Recepients, TextMessage,
};
use eframe::{
    egui,
    egui::{Rounding, Stroke},
    emath::Align2,
};
use flume::Sender;
use std::{collections::BTreeMap, net::Ipv4Addr, path::Path, time::SystemTime};
use timediff::TimeDiff;

pub const FONT_SCALE: f32 = 1.5;
pub const EMOJI_SCALE: f32 = 4.0;
pub const PUBLIC: &str = "Everyone";

#[derive(Default)]
pub struct Rooms {
    active_chat: Recepients,
    pub peers: PeersMap,
    order: Vec<Recepients>,
    chats: BTreeMap<Recepients, ChatHistory>,
    pub side_panel_opened: bool,
}
impl Rooms {
    pub fn new() -> Self {
        let mut chats = BTreeMap::new();
        chats.insert(Recepients::Peers, ChatHistory::new(Recepients::Peers));
        Rooms {
            active_chat: Recepients::Peers,
            peers: PeersMap::new(),
            order: vec![],
            chats,
            side_panel_opened: true,
        }
    }

    pub fn get_mut_public(&mut self) -> &mut ChatHistory {
        self.chats
            .get_mut(&Recepients::Peers)
            .expect("Public Exists")
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
            Recepients::One(ip) => self.peers.0.get(&ip).is_some_and(|p| p.is_online()),
            _ => self.peers.0.values().any(|p| p.is_online()),
        }
    }

    pub fn compose_message(&mut self) -> Option<TextMessage> {
        if !self.is_able_to_send() {
            return None;
        }
        let chat = self.get_mut_active();
        let trimmed = chat.input.trim().replace('\n', "").to_string();
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
    pub fn compose_file(&mut self, path: &Path) -> Option<TextMessage> {
        // FIXME
        // if !self.is_able_to_send() {
        //     return None;
        // }
        Some(TextMessage::out_message(
            Content::FileLink(FileLink::new(path)?),
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
        target_chat.history.push(msg);
        if recepients != self.active_chat {
            target_chat.unread += 1;
        }
    }

    pub fn recalculate_order(&mut self) {
        let mut order = self
            .chats
            .values()
            .filter(|v| v.recepients != Recepients::Peers)
            .filter_map(|c| c.history.last().map(|m| (m.time(), c.recepients)))
            .collect::<Vec<_>>();
        order.sort_by(|a, b| b.0.cmp(&a.0));
        self.order = order.into_iter().map(|o| o.1).collect();
    }

    pub fn _has_unread(&self) -> bool {
        self.chats.values().any(|c| c.unread > 0)
    }

    pub fn draw_history(&self, ui: &mut egui::Ui) {
        if !self.side_panel_opened {
            ui.vertical_centered(|ui| {
                let name = match self.active_chat {
                    Recepients::One(ip) => self.peers.0.get(&ip).expect("Peer exists").rich_name(),
                    _ => egui::RichText::new(PUBLIC).strong(),
                };
                ui.label(name);
            });
            ui.separator();
        }
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

    pub fn draw_list(&mut self, ui: &mut egui::Ui, tx: &Sender<ChatEvent>) {
        if self
            .chats
            .get_mut(&Recepients::Peers)
            .expect("Public exists")
            .draw_list_entry(
                ui,
                &mut self.active_chat,
                &self.peers,
                self.side_panel_opened,
            )
        {
            self.set_active(Recepients::Peers, tx);
        }
        ui.label("");
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
                self.set_active(*recepient, tx);
            }
            ui.label("");
        });
    }

    pub fn draw_input(&mut self, ui: &mut egui::Ui, tx: &Sender<ChatEvent>) {
        let status = self.peers.online_status(self.active_chat);
        self.get_mut_active().draw_input(ui, status, tx);
    }
    pub fn side_panel_toggle(&mut self, ui: &mut egui::Ui) {
        let side_ico = if self.side_panel_opened { "" } else { "" };
        let side_ico = egui::RichText::new(side_ico).monospace();
        // if self.has_unread() {
        //     side_ico = side_ico.strong();
        // }
        if ui.button(side_ico).clicked() {
            self.side_panel_opened = !self.side_panel_opened;
        }
    }

    pub fn list_go_up(&mut self, tx: &Sender<ChatEvent>) {
        let active = if self.active_chat == Recepients::Peers {
            self.order.last().cloned().unwrap_or_default()
        } else {
            let active_id = self
                .order
                .iter()
                .position(|k| k == &self.active_chat)
                .unwrap_or_default();
            match active_id {
                0 => Recepients::Peers,
                _ => self
                    .order
                    .get(active_id.saturating_sub(1))
                    .unwrap_or(&Recepients::Peers)
                    .to_owned(),
            }
        };
        self.set_active(active, tx);
    }

    pub fn list_go_down(&mut self, tx: &Sender<ChatEvent>) {
        let active = if self.active_chat == Recepients::Peers {
            self.order.first().cloned().unwrap_or_default()
        } else {
            let active_id = self
                .order
                .iter()
                .position(|k| k == &self.active_chat)
                .unwrap_or_default();
            self.order
                .get(active_id.saturating_add(1))
                .unwrap_or(&Recepients::Peers)
                .to_owned()
        };
        self.set_active(active, tx);
    }

    fn set_active(&mut self, recepient: Recepients, tx: &Sender<ChatEvent>) {
        self.active_chat = recepient;
        match recepient {
            Recepients::One(ip) => {
                if let Some(peer) = self.peers.0.get_mut(&ip) {
                    if !peer.is_offline() {
                        peer.set_presence(Presence::Unknown);
                    }
                }
            }
            _ => self.peers.0.values_mut().for_each(|p| {
                if !p.is_offline() {
                    p.set_presence(Presence::Unknown);
                }
            }),
        };
        tx.send(ChatEvent::Front(FrontEvent::Ping(recepient))).ok();
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
    emoji_mode: bool,
    input: String,
    history: Vec<TextMessage>,
    unread: usize,
}

impl ChatHistory {
    pub fn new(recepients: Recepients) -> Self {
        ChatHistory {
            recepients,
            emoji_mode: false,
            input: String::with_capacity(MAX_TEXT_SIZE),
            history: vec![],
            unread: 0,
        }
    }

    pub fn font_multiply(&self, ui: &mut egui::Ui) {
        for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
            let emoji_scale = if self.emoji_mode { 4.0 } else { 1.0 };
            font_id.size *= FONT_SCALE * emoji_scale;
        }
    }

    pub fn draw_input(&mut self, ui: &mut egui::Ui, status: Presence, tx: &Sender<ChatEvent>) {
        ui.visuals_mut().clip_rect_margin = 0.0;
        if status != Presence::Online {
            ui.visuals_mut().override_text_color =
                Some(ui.visuals().widgets.noninteractive.text_color());
        }
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
        let text_input = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .frame(false)
                .cursor_at_end(true)
                .desired_rows(if self.emoji_mode { 1 } else { 4 })
                .desired_width(ui.available_rect_before_wrap().width())
                .cursor_at_end(true),
        );
        text_input.request_focus();
        if text_input.changed() {
            self.input = self.input.replace('\n', "");
            if status == Presence::Unknown {
                tx.send(ChatEvent::Front(FrontEvent::Ping(self.recepients)))
                    .ok();
            }
        }
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
                        if peer.is_offline() {
                            ui.visuals().weak_text_color()
                        } else if peer.is_online() {
                            ui.visuals().strong_text_color()
                        } else {
                            ui.visuals().text_color()
                        },
                    )
                } else {
                    return false;
                }
            }
            _ => (PUBLIC.to_string(), ui.visuals().strong_text_color()),
        };

        let max_rect = ui.max_rect();
        let font_size = ui.text_style_height(&egui::TextStyle::Body);
        let font_id = egui::FontId::proportional(font_size);
        // let response = ui.interact(
        //     egui::Rect {
        //         min: max_rect.min,
        //         max: egui::Pos2::new(
        //             max_rect.width(),
        //             ui.text_style_height(&egui::TextStyle::Body) * 1.5,
        //         ),
        //     },
        //     egui::Id::new(name.to_string()),
        //     egui::Sense::click(),
        // );

        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(
                max_rect.width(),
                ui.text_style_height(&egui::TextStyle::Body) * 1.5,
            ),
            egui::Sense::click(),
        );

        let is_active = *active_chat == self.recepients;
        let active_fg = ui.visuals().widgets.hovered.fg_stroke;
        let inactive_fg = ui.visuals().widgets.inactive.fg_stroke;
        let stroke_width = stroke_width(ui);
        let stroke = if response.hovered() {
            Stroke::new(stroke_width * 2.0, active_fg.color)
        } else if is_active {
            Stroke::new(stroke_width * 2.0, inactive_fg.color)
        } else {
            Stroke::new(stroke_width, inactive_fg.color.linear_multiply(0.5))
        };
        let clicked = response.clicked();

        let rounding = Rounding {
            nw: rounding(ui) * 2.0,
            ne: 0.0,
            sw: rounding(ui) * 2.0,
            se: 0.0,
        };
        // egui::Frame::default()
        //     .stroke(stroke)
        //     .rounding(rounding)
        //     .show(ui, |ui| {
        //         ui.heading(egui::RichText::new(name.clone()).color(color));
        //     });

        painter.rect_stroke(painter.clip_rect(), rounding, stroke);

        painter.text(
            painter.clip_rect().left_center() + egui::Vec2::new(font_id.size, 0.0),
            Align2::LEFT_CENTER,
            name.clone(),
            font_id.clone(),
            color,
        );
        if self.unread > 0 {
            // painter.text(
            //     painter.clip_rect().right_center() - egui::Vec2::new(font_id.size, 0.0),
            //     Align2::RIGHT_CENTER,
            //     format!("[{}]", unread),
            //     font_id,
            //     ui.style().visuals.widgets.inactive.text_color(),
            // );
            painter.vline(
                painter.clip_rect().right(),
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
    pub fn draw(&self, ui: &mut egui::Ui, incoming: Option<&Peer>) {
        let direction = if self.is_incoming() {
            egui::Direction::LeftToRight
        } else {
            egui::Direction::RightToLeft
        };
        ui.with_layout(
            egui::Layout::from_main_dir_and_cross_align(direction, egui::Align::Min)
                .with_main_wrap(true),
            |line| {
                let mut rounding = Rounding::same(rounding(line) * FONT_SCALE);
                if self.is_incoming() {
                    rounding.sw = 0.0;
                } else {
                    rounding.se = 0.0;
                }
                egui::Frame::group(line.style())
                    .rounding(rounding)
                    .stroke(Stroke::new(
                        stroke_width(line),
                        line.style().visuals.widgets.inactive.fg_stroke.color,
                    ))
                    .show(line, |g| {
                        if let Some(peer) = incoming {
                            g.vertical(|v| {
                                v.horizontal(|h| {
                                    h.label(peer.rich_name())
                                        .on_hover_text_at_pointer(peer.ip().to_string());
                                    match self.content() {
                                        Content::Ping(_) => {
                                            h.label("joined..");
                                        }
                                        Content::Exit => {
                                            h.label("left..");
                                        }
                                        _ => (),
                                    }
                                });
                                self.draw_text(v);
                            });
                        } else {
                            self.draw_text(g);
                        }
                    })
                    .response
                    .on_hover_text_at_pointer(pretty_ago(self.time()).unwrap_or_default());
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
            Content::FileLink(link) => {
                ui.horizontal(|h| {
                    h.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                                font_id.size *= FONT_SCALE * EMOJI_SCALE;
                            }
                            ui.label("🖹");
                        });
                        // ui.heading(&link.name);
                        // ui.label(human_bytes(link.size as f64));

                        match link.status {
                            FileStatus::Link => if ui.link("Download").clicked() {},
                            FileStatus::InProgress => {
                                ui.add(egui::ProgressBar::new(
                                    link.progress.load(std::sync::atomic::Ordering::Relaxed) as f32
                                        / 100.0,
                                ));
                            }
                            FileStatus::Ready => {
                                // if ui.link("Open").clicked() {
                                // opener::open(&link.path).ok();
                                // }
                            }
                        };
                    });
                });
            }
            _ => (),
        }
    }
}

fn rounding(ui: &mut egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body) * 0.5
}
fn stroke_width(ui: &mut egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body) * 0.1
}

pub fn pretty_ago(ts: SystemTime) -> Option<String> {
    let delta = SystemTime::now().duration_since(ts).ok()?.as_secs();
    TimeDiff::to_diff(format!("{delta}s"))
        .locale(String::from("en-US"))
        .ok()?
        .parse()
        .ok()
}

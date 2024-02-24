use crate::chat::{
    limit_text,
    message::{MAX_EMOJI_SIZE, MAX_TEXT_SIZE},
    peers::{Peer, PeersMap},
    Content, Recepients, TextMessage,
};
use eframe::{
    egui,
    egui::{Rounding, Stroke},
    emath::Align2,
};
use std::{collections::BTreeMap, net::Ipv4Addr, time::SystemTime};
use timediff::TimeDiff;

pub const FONT_SCALE: f32 = 1.5;
pub const EMOJI_SCALE: f32 = 4.0;

#[derive(Default)]
pub struct Rooms {
    active_chat: Recepients,
    pub peers: PeersMap,
    order: Vec<Recepients>,
    chats: BTreeMap<Recepients, ChatHistory>,
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
        }
    }

    pub fn get_mut_public(&mut self) -> &mut ChatHistory {
        self.chats
            .get_mut(&Recepients::Peers)
            .expect("Public Exists")
    }

    pub fn get_mut_peer(&mut self, ip: Ipv4Addr) -> &mut ChatHistory {
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
        draw_list_entry(
            ui,
            &mut self.active_chat,
            &mut self.chats,
            &self.peers,
            Recepients::Peers,
        );

        egui::ScrollArea::vertical().show(ui, |ui| {
            let order = &self.order;
            for recepient in order.iter().filter(|r| r != &&Recepients::Peers) {
                draw_list_entry(
                    ui,
                    &mut self.active_chat,
                    &mut self.chats,
                    &self.peers,
                    *recepient,
                );
            }
            ui.label("");
        });
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

    pub fn draw_input(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().visuals.clip_rect_margin = 0.0;

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

impl Peer {
    fn rich_name(&self) -> egui::RichText {
        let mut label = egui::RichText::new(self.display_name());
        if self.is_exited() {
            label = label.weak();
        } else if self.is_online() {
            label = label.strong();
        }
        label
    }
}

impl TextMessage {
    pub fn draw(&self, ui: &mut egui::Ui, incoming: Option<&Peer>) {
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
                            g.vertical(|g| {
                                g.horizontal(|h| {
                                    h.label(peer.rich_name())
                                        .on_hover_text_at_pointer(peer.ip().to_string());
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

fn draw_list_entry(
    ui: &mut egui::Ui,
    active_chat: &mut Recepients,
    chats: &mut BTreeMap<Recepients, ChatHistory>,
    peers: &PeersMap,
    recepient: Recepients,
) {
    let max_rect = ui.max_rect();
    let font_size = ui.text_style_height(&egui::TextStyle::Body);
    let font_id = egui::FontId::proportional(font_size);

    let (response, painter) = ui.allocate_painter(
        egui::Vec2::new(
            max_rect.width(),
            ui.text_style_height(&egui::TextStyle::Body) * 1.5,
        ),
        egui::Sense::click(),
    );

    let is_active = *active_chat == recepient;
    let active_fg = ui.visuals().widgets.hovered.fg_stroke;
    let inactive_fg = ui.visuals().widgets.inactive.fg_stroke;
    let stroke = if is_active {
        Stroke::new(stroke_width(ui) * 2.0, inactive_fg.color)
    } else if response.hovered() {
        Stroke::new(stroke_width(ui) * 2.0, active_fg.color)
    } else {
        egui::Stroke::NONE
    };

    let unread = &mut chats.get_mut(&recepient).expect("Chat exists").unread;

    if response.clicked() {
        *active_chat = recepient;
        *unread = 0;
    }

    let rounding = Rounding {
        nw: rounding(ui),
        ne: 0.0,
        sw: rounding(ui),
        se: 0.0,
    };

    let unread = *unread;

    painter.rect_stroke(painter.clip_rect(), rounding, stroke);
    let (name, color) = match recepient {
        Recepients::One(ip) => {
            let peer = peers.0.get(&ip).expect("Peer exists");
            (
                peers.get_display_name(ip),
                if peer.is_exited() {
                    ui.visuals().weak_text_color()
                } else if peer.is_online() {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                },
            )
        }
        _ => ("Everyone".to_string(), ui.visuals().strong_text_color()),
    };

    painter.text(
        painter.clip_rect().left_center() + egui::Vec2::new(font_id.size, 0.0),
        Align2::LEFT_CENTER,
        name,
        font_id.clone(),
        color,
    );
    if unread > 0 {
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
            Stroke::new(stroke_width(ui) * 2.0, active_fg.color),
        );
    }
    response.on_hover_ui_at_pointer(|ui| {
        if unread > 0 {
            ui.label(format!("Unread: {}", unread));
        }
        if let Some(msg) = chats.get(&recepient).and_then(|c| c.history.last()) {
            if let Some(ago) = pretty_ago(msg.time()) {
                ui.label(format!("Last message {}", ago));
            }
        }
        if let Recepients::One(ip) = recepient {
            let peer = peers.0.get(&ip).expect("Peer exists");
            if let Some(ago) = pretty_ago(peer.last_time()) {
                ui.label(format!("Last seen {}", ago));
            }
            ui.label(format!("{ip}"));
        }
    });
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

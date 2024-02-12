pub mod message;

use self::message::Id;
use message::{Command, Message};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub enum Recepients {
    One(Ipv4Addr),
    Peers,
    All,
}

pub trait Repaintable
where
    Self: Clone + Sync + Send + 'static,
{
    fn request_repaint(&self);
}
#[derive(Clone)]
pub struct RepaintDummy;
impl Repaintable for RepaintDummy {
    fn request_repaint(&self) {}
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

pub struct UdpChat {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: usize,
    pub name: String,
    sync_sender: mpsc::SyncSender<(Ipv4Addr, Message)>,
    sync_receiver: mpsc::Receiver<(Ipv4Addr, Message)>,
    pub message: Message,
    pub history: Vec<TextMessage>,
    pub peers: BTreeMap<Ipv4Addr, Peer>,
    all_recepients: Vec<String>,
}

pub enum MessageContent {
    Joined,
    Left,
    Text(String),
}
#[allow(dead_code)]
pub struct TextMessage {
    ip: Ipv4Addr,
    id: Id,
    content: MessageContent,
}
impl TextMessage {
    pub fn from_text_message(ip: Ipv4Addr, msg: &Message) -> Self {
        TextMessage {
            ip,
            id: msg.id,
            content: MessageContent::Text(msg.read_text()),
        }
    }
    pub fn enter(ip: Ipv4Addr, id: Id) -> Self {
        TextMessage {
            ip,
            id,
            content: MessageContent::Joined,
        }
    }
    pub fn exit(ip: Ipv4Addr, id: Id) -> Self {
        TextMessage {
            ip,
            id,
            content: MessageContent::Left,
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn _id(&self) -> Id {
        self.id
    }
    pub fn content(&self) -> &MessageContent {
        &self.content
    }
    pub fn text(&self) -> &str {
        if let MessageContent::Text(text) = &self.content {
            text
        } else {
            ""
        }
    }
    pub fn draw_text(&self, ui: &mut eframe::egui::Ui) {
        if let MessageContent::Text(content) = &self.content {
            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                font_id.size *= 1.5;
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

impl UdpChat {
    pub fn new(name: String, port: usize) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Message)>(0);

        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            sync_sender: tx,
            sync_receiver: rx,
            message: Message::empty(),
            history: Vec::<TextMessage>::new(),
            peers: BTreeMap::<Ipv4Addr, Peer>::new(),
            all_recepients: vec![],
        }
    }

    pub fn prelude(&mut self, ctx: &impl Repaintable) {
        self.connect().ok();
        self.listen(ctx);
        self.message = Message::enter(&self.name);
        self.send(Recepients::All);
    }

    fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        let my_ip = local_ipaddress::get()
            .ok_or("no local")?
            .parse::<Ipv4Addr>()?;
        self.ip = my_ip;
        self.all_recepients = (0..=254)
            .map(|i| {
                format!(
                    "{}.{}.{}.{}:{}",
                    self.ip.octets()[0],
                    self.ip.octets()[1],
                    self.ip.octets()[2],
                    i,
                    self.port
                )
            })
            .collect();
        self.socket = match UdpSocket::bind(format!("{}:{}", self.ip, self.port)) {
            Ok(socket) => {
                socket.set_broadcast(true).unwrap();
                socket.set_multicast_loop_v4(false).unwrap();
                Some(Arc::new(socket))
            }
            _ => None,
        };

        Ok(())
    }

    fn listen(&self, ctx: &impl Repaintable) {
        if let Some(socket) = &self.socket {
            let reader = Arc::clone(socket);
            let receiver = self.sync_sender.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        reader.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if let Some(message) =
                            Message::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            ctx.request_repaint();
                            receiver.send((ip, message)).ok();
                        }
                    }
                }
            });
        }
    }

    pub fn send(&mut self, mut addrs: Recepients) {
        if self.message.command == Command::Empty {
            return;
        }

        let bytes = self.message.to_be_bytes();
        if let Some(socket) = &self.socket {
            if !self.peers.values().any(|p| p.is_online()) {
                addrs = Recepients::All;
            }
            match addrs {
                Recepients::All => self
                    .all_recepients
                    .iter()
                    .map(|r| socket.send_to(&bytes, r).is_ok())
                    .all(|r| r),
                Recepients::Peers => self
                    .peers
                    .keys()
                    .map(|ip| {
                        socket
                            .send_to(&bytes, format!("{}:{}", ip, self.port))
                            .is_ok()
                    })
                    .all(|r| r),
                Recepients::One(ip) => socket
                    .send_to(&bytes, format!("{}:{}", ip, self.port))
                    .is_ok(),
            };
        }
        if self.message.command == Command::Text {
            self.history
                .push(TextMessage::from_text_message(self.ip, &self.message));
        }
        self.message = Message::empty();
    }

    pub fn receive(&mut self) {
        if let Ok((r_ip, r_msg)) = self.sync_receiver.try_recv() {
            if r_ip == self.ip {
                return;
            }
            let txt_msg = TextMessage::from_text_message(r_ip, &r_msg);
            match r_msg.command {
                Command::Enter => {
                    let name = String::from_utf8_lossy(&r_msg.data);

                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        ip.insert(Peer::new(name));
                        self.history.push(TextMessage::enter(r_ip, r_msg.id));
                        self.message = Message::enter(&self.name);
                        self.send(Recepients::One(r_ip));
                    } else if let Some(peer) = self.peers.get_mut(&r_ip) {
                        if !peer.online {
                            peer.online = true;
                            peer.name = txt_msg.text().to_string();
                            self.history.push(TextMessage::enter(r_ip, r_msg.id));
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(r_ip));
                        }
                    }
                }
                Command::Text | Command::Repeat => {
                    self.history.push(txt_msg);
                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        ip.insert(Peer::new(r_ip.to_string()));
                        if r_ip != self.ip {
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(r_ip));
                        }
                    }
                }
                Command::Damaged => {
                    self.message =
                        Message::new(Command::AskToRepeat, r_msg.id.to_be_bytes().to_vec());
                    self.send(Recepients::One(r_ip));
                }
                // Command::AskToRepeat => {
                //     let id: u32 = u32::from_be_bytes(
                //         (0..4)
                //             .map(|i| *r_msg.data.get(i).unwrap_or(&0))
                //             .collect::<Vec<u8>>()
                //             .try_into()
                //             .unwrap(),
                //     );
                //     self.message = Message::retry_text(
                //         id,
                //         self.history
                //             .iter()
                //             .find(|m| m.id() == id)
                //             .unwrap_or(&TextMessage::new(r_ip, id, "NO SUCH MESSAGE! = ("))
                //             .content
                //             .as_str(),
                //     );
                //     self.send(Recepients::One(r_ip));
                // }
                Command::Exit => {
                    self.history.push(TextMessage::exit(r_ip, r_msg.id));
                    self.peers.entry(r_ip).and_modify(|p| p.online = false);
                }
                _ => (),
            }
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

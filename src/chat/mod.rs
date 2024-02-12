pub mod message;

use message::{Command, Id, Message};
use rodio::source::SineWave;
use rodio::{OutputStream, OutputStreamHandle, Source};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

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
    pub port: u16,
    pub name: String,
    sync_sender: mpsc::SyncSender<(Ipv4Addr, Message)>,
    sync_receiver: mpsc::Receiver<(Ipv4Addr, Message)>,
    pub play_audio: Arc<AtomicBool>,
    pub message: Message,
    pub history: Vec<TextMessage>,
    pub peers: BTreeMap<Ipv4Addr, Peer>,
    all_recepients: Vec<Ipv4Addr>,
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
    pub fn new(name: String, port: u16) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Message)>(0);

        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            sync_sender: tx,
            sync_receiver: rx,
            play_audio: Arc::new(AtomicBool::new(true)),
            message: Message::empty(),
            history: Vec::<TextMessage>::new(),
            peers: BTreeMap::<Ipv4Addr, Peer>::new(),
            all_recepients: vec![],
        }
    }

    pub fn prelude(&mut self, ctx: &impl Repaintable) {
        self.connect().ok(); // FIXME Handle Error
        self.listen(ctx);
        self.message = Message::enter(&self.name);
        self.send(Recepients::All);
    }

    fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        self.ip = get_my_ipv4().ok_or("No local IpV4")?;
        let octets = self.ip.octets();

        self.all_recepients = (0..=254)
            .map(|i| Ipv4Addr::new(octets[0], octets[1], octets[2], i))
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
            let socket = Arc::clone(socket);
            let receiver = self.sync_sender.clone();
            let ctx = ctx.clone();
            let play_audio = Arc::clone(&self.play_audio);
            let port = self.port;
            let name = self.name.clone();
            thread::spawn(move || {
                let mut sound_stream = OutputStream::try_default().ok();
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        socket.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if let Some(message) =
                            Message::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            if matches!(
                                &message.command,
                                Command::Enter | Command::Exit | Command::Text | Command::Greating
                            ) && play_audio.load(std::sync::atomic::Ordering::Relaxed)
                            {
                                play_sound(&mut sound_stream);
                            }
                            if message.command == Command::Enter {
                                let greating = Message::greating(&name);
                                socket
                                    .send_to(&greating.to_be_bytes(), SocketAddrV4::new(ip, port))
                                    .ok();
                            }
                            receiver.send((ip, message)).ok();
                            ctx.request_repaint();
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
                    .map(|r| {
                        socket
                            .send_to(&bytes, SocketAddrV4::new(*r, self.port))
                            .is_ok()
                    })
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

    pub fn receive(&mut self) -> bool {
        let mut new_message = false;
        if let Ok((r_ip, r_msg)) = self.sync_receiver.try_recv() {
            if r_ip == self.ip {
                return new_message;
            }
            let txt_msg = TextMessage::from_text_message(r_ip, &r_msg);
            match r_msg.command {
                Command::Enter | Command::Greating => {
                    new_message = true;
                    let name = String::from_utf8_lossy(&r_msg.data);
                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        ip.insert(Peer::new(name));
                        self.history.push(TextMessage::enter(r_ip, r_msg.id));

                        // self.message = Message::greating(&self.name);
                        // self.send(Recepients::One(r_ip));
                    } else if let Some(peer) = self.peers.get_mut(&r_ip) {
                        if !peer.online {
                            peer.online = true;
                            peer.name = txt_msg.text().to_string();
                            self.history.push(TextMessage::enter(r_ip, r_msg.id));
                            // self.message = Message::greting(&self.name);
                            // self.send(Recepients::One(r_ip));
                        }
                    }
                }
                Command::Text | Command::Repeat => {
                    new_message = true;
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
                    new_message = true;
                    self.history.push(TextMessage::exit(r_ip, r_msg.id));
                    self.peers.entry(r_ip).and_modify(|p| p.online = false);
                }
                _ => (),
            }
        }
        new_message
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

pub fn get_my_ipv4() -> Option<Ipv4Addr> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return None,
    };

    match socket.connect("8.8.8.8:80") {
        Ok(()) => (),
        Err(_) => return None,
    };

    if let Ok(SocketAddr::V4(addr)) = socket.local_addr() {
        return Some(addr.ip().to_owned());
    }
    None
}
fn play_sound(stream: &mut Option<(OutputStream, OutputStreamHandle)>) {
    if let Some((_stream, handle)) = stream {
        let mix = SineWave::new(432.0)
            .take_duration(Duration::from_secs_f32(0.2))
            .amplify(0.20)
            .fade_in(Duration::from_secs_f32(0.2))
            .buffered()
            .reverb(Duration::from_secs_f32(0.5), 0.2);
        let mix = SineWave::new(564.0)
            .take_duration(Duration::from_secs_f32(0.2))
            .amplify(0.10)
            .fade_in(Duration::from_secs_f32(0.2))
            .buffered()
            .reverb(Duration::from_secs_f32(0.3), 0.2)
            .mix(mix);
        handle.play_raw(mix).ok();
    }
}

pub mod message;

use eframe::egui::Context;
use flume::{Receiver, Sender};
use message::{Command, Id, Message};
use rodio::source::SineWave;
use rodio::{OutputStreamHandle, Source};
use std::collections::BTreeSet;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
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

#[derive(Clone)]
pub struct Notifier {
    ctx: Context,
    audio: Option<OutputStreamHandle>,
}
impl Notifier {
    pub fn new(ctx: &Context, audio: Option<OutputStreamHandle>) -> Self {
        Notifier {
            ctx: ctx.clone(),
            audio,
        }
    }
    fn play_sound(&self) {
        if let Some(audio) = &self.audio {
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
            audio.play_raw(mix).ok();
        }
    }
}

impl Repaintable for Notifier {
    fn request_repaint(&self) {
        self.ctx.request_repaint();
        self.play_sound();
    }
}

pub enum FrontEvent {
    Enter(String),
    Send(String),
    Exit,
}
pub enum BackEvent {
    PeerJoined((Ipv4Addr, String)),
    PeerLeft(Ipv4Addr),
    Message(TextMessage),
    MyIp(Ipv4Addr),
}

pub struct UdpChat {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub name: String,
    front_rx: Receiver<FrontEvent>,
    front_tx: Sender<BackEvent>,
    listen_tx: Sender<(Ipv4Addr, Message)>,
    listen_rx: Receiver<(Ipv4Addr, Message)>,
    pub play_audio: Arc<AtomicBool>,
    pub message: Message,
    // pub history: Vec<TextMessage>,
    pub peers: BTreeSet<Ipv4Addr>,
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
    pub fn enter(ip: Ipv4Addr) -> Self {
        TextMessage {
            ip,
            id: 0,
            content: MessageContent::Joined,
        }
    }
    pub fn exit(ip: Ipv4Addr) -> Self {
        TextMessage {
            ip,
            id: 0,
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
    // pub fn text(&self) -> &str {
    //     if let MessageContent::Text(text) = &self.content {
    //         text
    //     } else {
    //         ""
    //     }
    // }
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
    pub fn new(
        name: String,
        port: u16,
        front_tx: Sender<BackEvent>,
        front_rx: Receiver<FrontEvent>,
        play_audio: Arc<AtomicBool>,
    ) -> Self {
        let (listen_tx, listen_rx) = flume::unbounded::<(Ipv4Addr, Message)>();
        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            front_tx,
            front_rx,
            listen_tx,
            listen_rx,
            play_audio,
            message: Message::empty(),
            // history: Vec::<TextMessage>::new(),
            peers: BTreeSet::<Ipv4Addr>::new(),
            all_recepients: vec![],
        }
    }

    pub fn prelude(&mut self) {
        self.connect().ok(); // FIXME Handle Error
        self.listen();
        self.message = Message::enter(&self.name);
        self.send(Recepients::All);
    }

    fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        self.ip = get_my_ipv4().ok_or("No local IpV4")?;
        let octets = self.ip.octets();
        self.front_tx.send(BackEvent::MyIp(self.ip)).ok();

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

    pub fn run(&mut self, ctx: &impl Repaintable) {
        loop {
            self.read_front_events();
            self.receive(ctx);
        }
    }

    fn read_front_events(&mut self) {
        let mut recepients = None;
        for event in self.front_rx.try_iter() {
            match event {
                FrontEvent::Enter(name) => {
                    self.message = Message::enter(&name);
                    recepients = Some(Recepients::All);
                }
                FrontEvent::Send(text) => {
                    self.message = Message::text(&text);
                    recepients = Some(Recepients::Peers);
                }
                FrontEvent::Exit => {
                    self.message = Message::exit();
                    recepients = Some(Recepients::Peers);
                }
            }
        }
        if let Some(recepients) = recepients {
            self.send(recepients);
        }
    }

    fn listen(&self) {
        if let Some(socket) = &self.socket {
            let socket = Arc::clone(socket);
            let receiver = self.listen_tx.clone();
            // let ctx = ctx.clone();
            // let play_audio = Arc::clone(&self.play_audio);
            // let port = self.port;
            // let name = self.name.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        socket.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if let Some(message) =
                            Message::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            // if matches!(
                            //     &message.command,
                            //     Command::Enter | Command::Exit | Command::Text | Command::Greating
                            // ) && play_audio.load(std::sync::atomic::Ordering::Relaxed)
                            // {
                            //     ctx.request_repaint();
                            // }
                            // if message.command == Command::Enter {
                            //     let greating = Message::greating(&name);
                            //     socket
                            //         .send_to(&greating.to_be_bytes(), SocketAddrV4::new(ip, port))
                            //         .ok();
                            // }
                            // println!("messsage: {message}");
                            receiver.send((ip, message)).ok();
                            // ctx.request_repaint();
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
            if self.peers.is_empty() {
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
                    .iter()
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
            self.front_tx
                .send(BackEvent::Message(TextMessage::from_text_message(
                    self.ip,
                    &self.message,
                )))
                .ok();
        }
        self.message = Message::empty();
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        let mut recepients = None;

        for (r_ip, r_msg) in self.listen_rx.try_iter() {
            if r_ip == self.ip {
                continue;
            }
            let txt_msg = TextMessage::from_text_message(r_ip, &r_msg);

            match r_msg.command {
                Command::Enter | Command::Greating => {
                    let name = String::from_utf8_lossy(&r_msg.data);
                    self.front_tx
                        .send(BackEvent::PeerJoined((r_ip, name.to_string())))
                        .ok();

                    ctx.request_repaint();
                }
                Command::Text | Command::Repeat => {
                    if !self.peers.contains(&r_ip) {
                        self.front_tx
                            .send(BackEvent::PeerJoined((r_ip, r_ip.to_string())))
                            .ok();
                        self.peers.insert(r_ip);
                        if r_ip != self.ip {
                            self.message = Message::enter(&self.name);
                            recepients = Some(Recepients::One(r_ip));
                        }
                    }
                    self.front_tx.send(BackEvent::Message(txt_msg)).ok();
                    ctx.request_repaint()
                }
                // Command::Damaged => {
                //     self.message =
                //         Message::new(Command::AskToRepeat, r_msg.id.to_be_bytes().to_vec());
                //     recepients = Some(Recepients::One(r_ip));
                // }
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
                    self.peers.remove(&r_ip);
                    self.front_tx.send(BackEvent::PeerLeft(r_ip)).ok();
                    ctx.request_repaint();
                }
                _ => (),
            }
        }
        if let Some(recepients) = recepients {
            self.send(recepients);
        }
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

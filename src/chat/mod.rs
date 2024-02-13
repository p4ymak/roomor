pub mod message;

use eframe::egui::Context;
use flume::{Receiver, Sender};
use message::{Command, Id, Message};
use rodio::source::SineWave;
use rodio::{OutputStreamHandle, Source};
use std::collections::{BTreeMap, BTreeSet};
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
#[derive(Debug)]
pub enum FrontEvent {
    Enter(String),
    Send(String),
    Exit,
}
#[derive(Debug)]
pub enum BackEvent {
    PeerJoined((Ipv4Addr, String)),
    PeerLeft(Ipv4Addr),
    Message(TextMessage),
    MyIp(Ipv4Addr),
}

#[derive(Debug)]
pub enum ChatEvent {
    Front(FrontEvent),
    Incoming((Ipv4Addr, Message)),
}

struct MessageSender {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub message: Message,
    pub peers: BTreeSet<Ipv4Addr>,
    all_recepients: Vec<Ipv4Addr>,
    front_tx: Sender<BackEvent>,
}
impl MessageSender {
    pub fn new(port: u16, front_tx: Sender<BackEvent>) -> Self {
        MessageSender {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            message: Message::empty(),
            peers: BTreeSet::<Ipv4Addr>::new(),
            all_recepients: vec![],
            front_tx,
        }
    }
    fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        self.ip = get_my_ipv4().ok_or("No local IpV4")?;
        let octets = self.ip.octets();
        self.front_tx.send(BackEvent::MyIp(self.ip)).ok();

        self.all_recepients = (0..=254)
            .map(|i| Ipv4Addr::new(octets[0], octets[1], octets[2], i))
            .collect();
        self.socket = match UdpSocket::bind(SocketAddrV4::new(self.ip, self.port)) {
            Ok(socket) => {
                socket.set_broadcast(true).ok();
                socket.set_multicast_loop_v4(false).ok();
                socket.set_nonblocking(false).ok();
                Some(Arc::new(socket))
            }
            _ => None,
        };

        Ok(())
    }

    pub fn send(&mut self, message: Message, mut addrs: Recepients) {
        if message.command == Command::Empty {
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
        if message.command == Command::Text {
            self.front_tx
                .send(BackEvent::Message(TextMessage::from_text_message(
                    self.ip,
                    &self.message,
                )))
                .ok();
        }
    }
}
pub struct UdpChat {
    pub name: String,
    tx: Sender<ChatEvent>,
    rx: Receiver<ChatEvent>,
    pub play_audio: Arc<AtomicBool>,
    sender: MessageSender, // pub history: Vec<TextMessage>,
    history: BTreeMap<Id, Message>,
}

#[derive(Debug)]
pub enum MessageContent {
    Joined,
    Left,
    Text(String),
}

#[allow(dead_code)]
#[derive(Debug)]
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
        // front_rx: Receiver<Event>,
        play_audio: Arc<AtomicBool>,
    ) -> Self {
        let (tx, rx) = flume::unbounded::<ChatEvent>();
        UdpChat {
            sender: MessageSender::new(port, front_tx),
            name,
            tx,
            rx,
            play_audio,
            history: BTreeMap::<Id, Message>::new(),
        }
    }
    pub fn tx(&self) -> Sender<ChatEvent> {
        self.tx.clone()
    }
    pub fn prelude(&mut self, name: &str, port: u16) {
        self.name = name.to_string();
        self.sender.port = port;
        self.sender.connect().ok(); // FIXME Handle Error
        self.listen();
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.sender
            .send(Message::enter(&self.name), Recepients::All);
        // self.read_front_events();
        self.receive(ctx);
    }

    fn listen(&self) {
        if let Some(socket) = &self.sender.socket {
            let socket = Arc::clone(socket);
            let receiver = self.tx.clone();
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
                            receiver.send(ChatEvent::Incoming((ip, message))).ok();
                            // ctx.request_repaint();
                        }
                    }
                }
            });
        }
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        println!("READ");

        for event in self.rx.iter() {
            println!("{:?}", event);
            match event {
                ChatEvent::Front(front) => match front {
                    FrontEvent::Enter(name) => {
                        self.sender.send(Message::enter(&name), Recepients::All);
                    }
                    FrontEvent::Send(text) => {
                        self.sender.send(Message::text(&text), Recepients::Peers);
                    }
                    FrontEvent::Exit => {
                        self.sender.send(Message::exit(), Recepients::Peers);
                    }
                },

                ChatEvent::Incoming((r_ip, r_msg)) => {
                    if r_ip == self.sender.ip {
                        continue;
                    }
                    let txt_msg = TextMessage::from_text_message(r_ip, &r_msg);
                    // FIXME
                    if !self.sender.peers.contains(&r_ip) {
                        self.sender
                            .front_tx
                            .send(BackEvent::PeerJoined((r_ip, r_ip.to_string())))
                            .ok();
                        self.sender.peers.insert(r_ip);
                        if r_ip != self.sender.ip {
                            self.sender
                                .send(Message::enter(&self.name), Recepients::One(r_ip));
                        }
                    }
                    match r_msg.command {
                        Command::Enter | Command::Greating => {
                            let name = String::from_utf8_lossy(&r_msg.data);
                            self.sender
                                .front_tx
                                .send(BackEvent::PeerJoined((r_ip, name.to_string())))
                                .ok();

                            ctx.request_repaint();
                        }
                        Command::Text | Command::Repeat => {
                            self.sender.front_tx.send(BackEvent::Message(txt_msg)).ok();
                            ctx.request_repaint()
                        }
                        Command::Damaged => {
                            self.sender.send(
                                Message::new(Command::AskToRepeat, r_msg.id.to_be_bytes().to_vec()),
                                Recepients::One(r_ip),
                            );
                        }
                        Command::AskToRepeat => {
                            let id: u32 = u32::from_be_bytes(
                                (0..4)
                                    .map(|i| *r_msg.data.get(i).unwrap_or(&0))
                                    .collect::<Vec<u8>>()
                                    .try_into()
                                    .unwrap(),
                            );
                            if let Some(message) = self.history.get(&id) {
                                let mut message = message.clone();
                                message.command = Command::Repeat;
                                self.sender.send(message, Recepients::One(r_ip));
                            }
                        }
                        Command::Exit => {
                            self.sender.peers.remove(&r_ip);
                            self.sender.front_tx.send(BackEvent::PeerLeft(r_ip)).ok();
                            ctx.request_repaint();
                        }
                        _ => (),
                    }
                }
            }
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

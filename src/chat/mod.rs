pub mod message;
pub mod networker;
pub mod notifier;
pub mod peers;

use self::{networker::NetWorker, notifier::Repaintable};
use flume::{Receiver, Sender};
use log::debug;
use message::{Command, Id, UdpMessage};
use std::{
    collections::BTreeMap,
    error::Error,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    thread::{self, JoinHandle},
    time::SystemTime,
};

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Recepients {
    One(Ipv4Addr),
    #[default]
    Peers,
    All,
    Myself,
}
impl Recepients {
    pub fn from_ip(ip: Ipv4Addr, public: bool) -> Self {
        if !public {
            Recepients::One(ip)
        } else {
            Recepients::Peers
        }
    }
}

#[derive(Debug, Clone)]
pub enum Content {
    Enter(String),
    Text(String),
    Icon(String),
    Exit,
    Empty,
}
#[derive(Debug)]
pub enum BackEvent {
    PeerJoined((Ipv4Addr, Option<String>)),
    PeerLeft(Ipv4Addr),
    Message(TextMessage),
}

#[derive(Debug)]
pub enum FrontEvent {
    Ping(Recepients),
    Greating(Recepients),
    Exit,
    Message(TextMessage),
}

#[derive(Debug)]
pub enum ChatEvent {
    Front(FrontEvent),
    Incoming((Ipv4Addr, UdpMessage)),
}

pub struct UdpChat {
    pub name: String,
    tx: Sender<ChatEvent>,
    rx: Receiver<ChatEvent>,
    sender: NetWorker,
    history: BTreeMap<Id, UdpMessage>,
    thread_handle: Option<JoinHandle<()>>,
}
impl Drop for UdpChat {
    fn drop(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TextMessage {
    timestamp: SystemTime,
    incoming: bool,
    public: bool,
    ip: Ipv4Addr,
    id: Id,
    content: Content,
}
impl TextMessage {
    pub fn logo() -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip: Ipv4Addr::UNSPECIFIED,
            id: 0,
            content: Content::Icon(String::from("RMЯ")),
        }
    }

    pub fn from_message(ip: Ipv4Addr, msg: &UdpMessage, incoming: bool) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming,
            public: msg.public,
            ip,
            id: msg.id,
            content: match msg.command {
                Command::Enter => Content::Enter(msg.read_text()),
                Command::Text => Content::Text(msg.read_text()),
                Command::Icon => Content::Icon(msg.read_text()),
                Command::Exit => Content::Exit,
                _ => Content::Empty,
            },
        }
    }
    pub fn is_public(&self) -> bool {
        self.public
    }
    pub fn is_incoming(&self) -> bool {
        self.incoming
    }

    pub fn in_enter(ip: Ipv4Addr, name: String) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip,
            id: 0,
            content: Content::Enter(name),
        }
    }

    pub fn out_message(content: Content, recipients: Recepients) -> Self {
        let (public, ip) = match recipients {
            Recepients::One(ip) => (false, ip),
            _ => (true, Ipv4Addr::BROADCAST),
        };

        TextMessage {
            timestamp: SystemTime::now(),
            incoming: false,
            public,
            ip,
            id: 0,
            content,
        }
    }
    pub fn in_exit(ip: Ipv4Addr) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip,
            id: 0,
            content: Content::Exit,
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    // pub fn id(&self) -> Id {
    //     self.id
    // }
    pub fn content(&self) -> &Content {
        &self.content
    }
    pub fn get_text(&self) -> &str {
        match &self.content {
            Content::Text(text) | Content::Icon(text) => text,
            _ => "",
        }
    }
    pub fn time(&self) -> SystemTime {
        self.timestamp
    }
}

impl UdpChat {
    pub fn new(ip: Ipv4Addr, front_tx: Sender<BackEvent>) -> Self {
        let (tx, rx) = flume::unbounded::<ChatEvent>();
        let sender = NetWorker::new(ip, front_tx);

        UdpChat {
            sender,
            name: String::new(),
            tx,
            rx,
            history: BTreeMap::<Id, UdpMessage>::new(),
            thread_handle: None,
        }
    }
    pub fn tx(&self) -> Sender<ChatEvent> {
        self.tx.clone()
    }
    pub fn prelude(
        &mut self,
        name: &str,
        port: u16,
        mask: u8,
    ) -> Result<(), Box<dyn Error + 'static>> {
        self.name = name.to_string();
        self.sender.port = port;
        self.sender.name = name.to_string();
        self.sender.connect(mask)?;
        self.listen();
        Ok(())
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.sender
            .send(UdpMessage::enter(&self.name), Recepients::All);
        self.receive(ctx);
    }

    fn listen(&mut self) {
        self.thread_handle = self.sender.socket.as_ref().map(|socket| {
            let local_ip = self.sender.ip;
            let socket = Arc::clone(socket);
            let receiver = self.tx.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        socket.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();

                        if let Some(message) =
                            UdpMessage::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            if ip != local_ip {
                                receiver.send(ChatEvent::Incoming((ip, message))).ok();
                            } else if message.command == Command::Exit {
                                break;
                            }
                        }
                    }
                }
            })
        });
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        for event in self.rx.iter() {
            match event {
                ChatEvent::Front(front) => match front {
                    FrontEvent::Message(msg) => {
                        debug!("Sending: {}", msg.get_text());

                        let message = UdpMessage::from_message(&msg);
                        self.history.insert(message.id, message.clone());
                        self.sender
                            .send(message, Recepients::from_ip(msg.ip, msg.is_public()));
                        self.sender.front_tx.send(BackEvent::Message(msg)).ok();
                        ctx.request_repaint();
                    }
                    FrontEvent::Ping(recepients) => {
                        debug!("Ping {recepients:?}");
                        self.sender.send(UdpMessage::enter(&self.name), recepients);
                    }
                    FrontEvent::Greating(recepients) => {
                        debug!("Greating {recepients:?}");
                        self.sender
                            .send(UdpMessage::greating(&self.name), recepients);
                    }
                    FrontEvent::Exit => {
                        debug!("I'm Exit");
                        self.sender.send(UdpMessage::exit(), Recepients::Peers);
                        self.sender.send(UdpMessage::exit(), Recepients::Myself);
                        break;
                    }
                },

                ChatEvent::Incoming((r_ip, r_msg)) => {
                    if r_ip == self.sender.ip {
                        continue;
                    }
                    let txt_msg = TextMessage::from_message(r_ip, &r_msg, true);
                    debug!("Received {:?} from {r_ip}", r_msg.command);

                    match r_msg.command {
                        Command::Enter | Command::Greating => {
                            let user_name = String::from_utf8_lossy(&r_msg.data);

                            self.sender.handle_event(
                                BackEvent::PeerJoined((r_ip, Some(user_name.to_string()))),
                                ctx,
                            );
                            if r_msg.command == Command::Enter {
                                self.sender
                                    .send(UdpMessage::greating(&self.name), Recepients::One(r_ip));
                            }
                        }

                        Command::Exit => {
                            self.sender.handle_event(BackEvent::PeerLeft(r_ip), ctx);
                        }
                        Command::Text | Command::Icon | Command::Repeat => {
                            self.sender.incoming(r_ip, &self.name);
                            self.sender.handle_event(BackEvent::Message(txt_msg), ctx);
                        }
                        Command::Error => {
                            self.sender.incoming(r_ip, &self.name);
                            self.sender.send(
                                UdpMessage::new(
                                    Command::AskToRepeat,
                                    r_msg.id.to_be_bytes().to_vec(),
                                    r_msg.public,
                                ),
                                Recepients::One(r_ip),
                            );
                        }
                        Command::AskToRepeat => {
                            self.sender.incoming(r_ip, &self.name);
                            let id: u32 = u32::from_be_bytes(
                                (0..4)
                                    .map(|i| *r_msg.data.get(i).unwrap_or(&0))
                                    .collect::<Vec<u8>>()
                                    .try_into()
                                    .unwrap_or_default(),
                            );
                            // Resend my Name
                            if id == 0 {
                                self.sender
                                    .send(UdpMessage::greating(&self.name), Recepients::One(r_ip));
                            } else if let Some(message) = self.history.get(&id) {
                                let mut message = message.clone();
                                message.command = Command::Repeat;
                                self.sender.send(message, Recepients::One(r_ip));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn utf8_truncate(input: &mut String, maxsize: usize) {
    let mut utf8_maxsize = input.len();
    if utf8_maxsize >= maxsize {
        {
            let mut char_iter = input.char_indices();
            while utf8_maxsize >= maxsize {
                utf8_maxsize = match char_iter.next_back() {
                    Some((index, _)) => index,
                    _ => 0,
                };
            }
        } // Extra {} wrap to limit the immutable borrow of char_indices()
        input.truncate(utf8_maxsize);
    }
}
pub fn limit_text(text: &mut String, limit: usize) {
    *text = text.trim_end_matches('\n').to_string();
    utf8_truncate(text, limit);
}

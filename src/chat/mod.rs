pub mod file;
pub mod inbox;
pub mod message;
pub mod networker;
pub mod notifier;
pub mod outbox;
pub mod peers;

use self::{
    file::{FileEnding, FileLink},
    inbox::InMessage,
    message::{new_id, DATA_LIMIT_BYTES, MAX_PREVIEW_CHARS},
    networker::{NetWorker, TIMEOUT_SECOND},
    notifier::Repaintable,
    outbox::Outbox,
};
use crate::{app::UserSetup, chat::peers::Presence};
use directories::UserDirs;
use eframe::Result;
use flume::{Receiver, Sender};
use inbox::Inbox;
use log::error;
use message::{Command, Id, UdpMessage};
use std::{
    error::Error,
    fs,
    net::{Ipv4Addr, SocketAddr},
    ops::ControlFlow,
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::SystemTime,
};

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Recepients {
    One(Ipv4Addr),
    #[default]
    All,
}
impl Recepients {
    pub fn from_ip(ip: Ipv4Addr, public: bool) -> Self {
        if !public {
            Recepients::One(ip)
        } else {
            Recepients::All
        }
    }
    pub fn is_public(&self) -> bool {
        matches!(self, Recepients::All)
    }
}

// FIXME
#[allow(dead_code)]
#[derive(Clone)]
pub enum Content {
    Ping(String),
    Text(String),
    Icon(String),
    FileLink(Arc<FileLink>),
    FileData(PathBuf),
    FileEnding(FileEnding),
    Exit,
    Seen,
    Empty,
}
impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "content")
    }
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
    networker: NetWorker,
    outbox: Outbox,
    inbox: Inbox,
    downloads_path: PathBuf,
    thread_handle: Option<JoinHandle<()>>,
}
impl Drop for UdpChat {
    fn drop(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }
    }
}
#[derive(Debug, Clone)]
pub enum Seen {
    One,
    Many(Vec<Ipv4Addr>),
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
    seen: Option<Seen>,
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
            seen: Some(Seen::One),
        }
    }

    pub fn from_udp(ip: Ipv4Addr, msg: &UdpMessage, incoming: bool) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming,
            public: msg.public,
            ip,
            id: msg.id,
            content: match msg.command {
                Command::Enter => Content::Ping(msg.read_text()),
                Command::Text => {
                    let mut text = msg.read_text();
                    let is_icon = text.starts_with(' ');
                    text = text.trim().to_string();
                    if is_icon {
                        Content::Icon(text)
                    } else {
                        Content::Text(text)
                    }
                }
                // Command::File => Content::FileLink(FileLink::from_text(&msg.read_text()).unwrap()), // FIXME
                Command::Exit => Content::Exit,
                Command::Seen => Content::Seen,
                _ => Content::Empty,
            },
            seen: incoming.then_some(Seen::One),
        }
    }

    pub fn from_inmsg(inmsg: &InMessage) -> Self {
        TextMessage {
            timestamp: inmsg.ts,
            incoming: true,
            public: inmsg.public,
            ip: inmsg.sender,
            id: inmsg.id,
            content: Content::FileLink(inmsg.link.clone()),
            seen: Some(Seen::One),
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
            content: Content::Ping(name),
            seen: Some(Seen::One),
        }
    }

    pub fn out_message(content: Content, recipients: Recepients) -> Self {
        let (public, ip) = match recipients {
            Recepients::One(ip) => (false, ip),
            _ => (true, Ipv4Addr::BROADCAST), //FIXME ??
        };

        TextMessage {
            timestamp: SystemTime::now(),
            incoming: false,
            public,
            ip,
            id: new_id(),
            content,
            seen: None,
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
            seen: Some(Seen::One),
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn seen_private(&mut self) {
        self.seen = Some(Seen::One);
    }
    pub fn seen_public_by(&mut self, ip: Ipv4Addr) {
        if let Some(Seen::Many(peers)) = &mut self.seen {
            peers.push(ip);
        } else {
            self.seen = Some(Seen::Many(vec![ip]));
        }
    }
    pub fn is_seen(&self) -> bool {
        self.seen.is_some()
    }
    pub fn _is_seen_by(&self) -> &[Ipv4Addr] {
        if let Some(Seen::Many(peers)) = &self.seen {
            peers
        } else {
            &[]
        }
    }
    pub fn content(&self) -> &Content {
        &self.content
    }
    pub fn get_text(&self) -> String {
        match &self.content {
            Content::Text(text) => {
                if text.chars().count() <= MAX_PREVIEW_CHARS {
                    text.to_owned()
                } else {
                    let mut preview = text
                        .as_str()
                        .chars()
                        .take(MAX_PREVIEW_CHARS)
                        .collect::<String>();
                    preview.push_str("..");
                    preview
                }
            }
            Content::Icon(icon) => icon.to_owned(),
            Content::FileLink(link) => link.name.to_owned(),
            _ => String::new(),
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
        let downloads_path = UserDirs::new()
            .unwrap()
            .download_dir()
            .unwrap()
            .join("Roomor");
        fs::create_dir_all(&downloads_path).ok(); // FIXME

        UdpChat {
            networker: sender,
            name: String::new(),
            tx,
            rx,
            outbox: Outbox::default(),
            inbox: Inbox::default(),
            thread_handle: None,
            downloads_path,
        }
    }
    pub fn tx(&self) -> Sender<ChatEvent> {
        self.tx.clone()
    }
    pub fn prelude(&mut self, user: &UserSetup) -> Result<(), Box<dyn Error + 'static>> {
        self.name = user.name().to_string();
        self.networker.port = user.port();
        self.networker.name = user.name().to_string();
        self.networker.connect(user.multicast())?;
        self.listen();
        Ok(())
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.networker
            .send(UdpMessage::enter(&self.name), Recepients::All)
            .inspect_err(|e| error!("{e}"))
            .ok();
        self.receive(ctx);
    }

    fn listen(&mut self) {
        self.thread_handle = self.networker.socket.as_ref().map(|socket| {
            let local_ip = self.networker.ip;
            let socket = Arc::clone(socket);
            let receiver = self.tx.clone();
            thread::Builder::new()
                .name("listener".to_string())
                .spawn(move || {
                    let mut buf = [0; DATA_LIMIT_BYTES * 2];
                    loop {
                        if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                            socket.recv_from(&mut buf)
                        {
                            let ip = *src_addr_v4.ip();
                            if let Some(message) =
                                UdpMessage::from_be_bytes(&buf[..number_of_bytes])
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
                .expect("can't build thread")
        });
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        for event in self.rx.iter() {
            // FIXME file request timer
            self.inbox.retain(&mut self.networker, ctx, TIMEOUT_SECOND);

            match event {
                ChatEvent::Front(front) => {
                    match self
                        .networker
                        .handle_front_event(&mut self.outbox, ctx, front)
                    {
                        ControlFlow::Continue(_) => continue,
                        ControlFlow::Break(_) => break,
                    }
                }
                ChatEvent::Incoming((r_ip, r_msg)) => {
                    self.networker.handle_message(
                        &mut self.inbox,
                        &mut self.outbox,
                        ctx,
                        r_ip,
                        r_msg,
                        &self.downloads_path,
                    );
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
                utf8_maxsize = char_iter.next_back().unwrap_or_default().0;
            }
        } // Extra {} wrap to limit the immutable borrow of char_indices()
        input.truncate(utf8_maxsize);
    }
}

pub fn limit_text(text: &mut String, limit: usize) {
    *text = text.trim_end_matches('\n').to_string();
    utf8_truncate(text, limit);
}

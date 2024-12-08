pub mod file;
pub mod inbox;
pub mod message;
pub mod networker;
pub mod notifier;
pub mod outbox;
pub mod peers;

use self::{
    file::FileLink,
    inbox::InMessage,
    message::{new_id, DATA_LIMIT_BYTES, MAX_PREVIEW_CHARS},
    networker::NetWorker,
    notifier::Repaintable,
    outbox::Outbox,
};
use crate::{app::UserSetup, chat::peers::Presence};
use eframe::Result;
use flume::{Receiver, Sender};
use inbox::Inbox;
use log::error;
use message::{Command, Id, UdpMessage};
use peers::PeerId;
use std::{
    error::Error,
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
    Big(String),
    Icon(String),
    FileLink(Arc<FileLink>),
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
    PeerJoined(Ipv4Addr, PeerId, Option<String>),
    PeerLeft(PeerId),
    Message(TextMessage),
}

#[derive(Debug)]
pub enum FrontEvent {
    Ping(PeerId),
    Exit,
    Message(TextMessage),
}

#[derive(Debug)]
pub enum ChatEvent {
    Front(FrontEvent),
    Incoming(Ipv4Addr, UdpMessage),
}

pub struct UdpChat {
    pub name: String,
    pub id: PeerId,
    tx: Sender<ChatEvent>,
    rx: Receiver<ChatEvent>,
    networker: NetWorker,
    outbox: Outbox,
    inbox: Inbox,
    pub downloads_path: PathBuf,
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
    Many(Vec<PeerId>),
}

#[derive(Debug, Copy, Clone)]
pub enum Destination {
    From(PeerId),
    To(PeerId),
}
impl Destination {
    pub fn is_incoming(&self) -> bool {
        matches!(self, Destination::From(_))
    }
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TextMessage {
    timestamp: SystemTime,
    public: bool,
    dest: Destination,
    id: Id,
    content: Content,
    seen: Option<Seen>,
}
impl TextMessage {
    pub fn logo() -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            public: true,
            dest: Destination::From(PeerId::PUBLIC),
            id: 0,
            content: Content::Big(String::from("RMЯ")),
            seen: Some(Seen::One),
        }
    }

    pub fn from_udp(msg: &UdpMessage) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            public: msg.public,
            dest: Destination::From(msg.from_peer_id),
            id: msg.id,
            content: match msg.command {
                Command::Enter => Content::Ping(msg.read_text()),
                Command::Text => {
                    let mut text = msg.read_text();
                    let is_big = text.starts_with(' ');
                    let is_icon = text.starts_with('/');
                    text = text.trim().to_string();
                    if is_big {
                        Content::Big(text)
                    } else if is_icon {
                        Content::Icon(text.trim_start_matches('/').to_string())
                    } else {
                        Content::Text(text)
                    }
                }
                // Command::File => Content::FileLink(FileLink::from_text(&msg.read_text()).unwrap()), // FIXME
                Command::Exit => Content::Exit,
                Command::Seen => Content::Seen,
                _ => Content::Empty,
            },
            seen: Some(Seen::One),
        }
    }

    pub fn from_inmsg(inmsg: &InMessage) -> Self {
        TextMessage {
            timestamp: inmsg.ts,
            public: inmsg.public,
            dest: Destination::From(inmsg.from_peer_id),
            id: inmsg.id,
            content: Content::FileLink(inmsg.link.clone()),
            seen: Some(Seen::One),
        }
    }

    pub fn is_public(&self) -> bool {
        self.public
    }
    pub fn is_incoming(&self) -> bool {
        self.dest.is_incoming()
    }

    pub fn in_enter(peer_id: PeerId, name: String) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            public: true,
            dest: Destination::From(peer_id),
            id: 0,
            content: Content::Ping(name),
            seen: Some(Seen::One),
        }
    }

    pub fn out_message(content: Content, peer_id: PeerId) -> Self {
        let id = match content {
            Content::FileLink(ref link) => link.id(),
            _ => new_id(),
        };
        TextMessage {
            timestamp: SystemTime::now(),
            public: peer_id.is_public(),
            dest: Destination::To(peer_id),
            id,
            content,
            seen: None,
        }
    }

    pub fn in_exit(peer_id: PeerId) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            public: true,
            dest: Destination::From(peer_id),
            id: 0,
            content: Content::Exit,
            seen: Some(Seen::One),
        }
    }

    pub fn peer_id(&self) -> PeerId {
        match self.dest {
            Destination::From(id) => id,
            Destination::To(id) => id,
        }
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn seen_private(&mut self) {
        self.seen = Some(Seen::One);
    }
    pub fn seen_public_by(&mut self, id: PeerId) {
        if let Some(Seen::Many(peers)) = &mut self.seen {
            peers.push(id);
        } else {
            self.seen = Some(Seen::Many(vec![id]));
        }
    }
    pub fn is_seen(&self) -> bool {
        self.seen.is_some()
    }
    pub fn is_seen_by(&self) -> &[PeerId] {
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
            Content::Big(big) => big.to_owned(),
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
    pub fn new(ip: Ipv4Addr, front_tx: Sender<BackEvent>, downloads_path: PathBuf) -> Self {
        let (tx, rx) = flume::unbounded::<ChatEvent>();
        let sender = NetWorker::new(ip, front_tx);

        UdpChat {
            networker: sender,
            id: PeerId::default(),
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
        self.id = user.id();
        self.networker.multicast.set_port(user.port());
        self.networker.set_id(user.id());
        self.networker.name = user.name().to_string();
        self.networker.connect(user.multicast())?;
        self.listen();
        Ok(())
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.networker
            .send(UdpMessage::enter(self.id, &self.name), PeerId::PUBLIC)
            .inspect_err(|e| error!("{e}"))
            .ok();
        self.receive(ctx);
    }

    fn listen(&mut self) {
        self.thread_handle = self.networker.socket.as_ref().map(|socket| {
            let local_id = self.networker.id(); // FIXME maybe need update
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
                            if let Ok(message) = UdpMessage::from_be_bytes(&buf[..number_of_bytes])
                            {
                                log::debug!(
                                    "{:?} From PeerId {}",
                                    message.command,
                                    message.from_peer_id.0
                                );
                                if message.from_peer_id != local_id {
                                    receiver.send(ChatEvent::Incoming(ip, message)).ok();
                                } else if message.command == Command::Exit {
                                    #[cfg(not(target_os = "android"))] // FIXME
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
                ChatEvent::Incoming(r_ip, r_msg) => {
                    let peer_id = r_msg.from_peer_id;
                    self.networker.handle_message(
                        &mut self.inbox,
                        &mut self.outbox,
                        ctx,
                        r_ip,
                        r_msg,
                        &self.downloads_path,
                    );
                    self.inbox
                        .wake_for_missed_one(&mut self.networker, ctx, peer_id);
                }
            }
        }
    }
    #[allow(dead_code)]
    pub fn downloads_path(&self) -> PathBuf {
        self.downloads_path.clone()
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

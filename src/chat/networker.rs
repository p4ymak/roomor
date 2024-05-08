use crate::chat::{
    file::FileLink,
    message::{self, send_shards, Command, ShardCount, DATA_LIMIT_BYTES},
    InMessage, Seen, TextMessage,
};

use super::{
    message::UdpMessage, notifier::Repaintable, peers::PeersMap, BackEvent, Content, FrontEvent,
    Inbox, Outbox, Recepients,
};
use flume::Sender;
use log::{debug, error};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    ops::ControlFlow,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub const TIMEOUT_ALIVE: Duration = Duration::from_secs(60);
pub const TIMEOUT_CHECK: Duration = Duration::from_secs(10);
pub const IP_MULTICAST_DEFAULT: Ipv4Addr = Ipv4Addr::new(225, 225, 225, 225);
pub const IP_UNSPECIFIED: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
// pub const TIMEOUT_SECOND: Duration = Duration::from_secs(1);

pub struct NetWorker {
    pub name: String,
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub peers: PeersMap,
    pub front_tx: Sender<BackEvent>,
}

impl NetWorker {
    pub fn new(ip: Ipv4Addr, front_tx: Sender<BackEvent>) -> Self {
        NetWorker {
            name: String::new(),
            socket: None,
            ip,
            port: 4444,
            peers: PeersMap::new(),
            front_tx,
        }
    }
    pub fn connect(&mut self, multicast: Ipv4Addr) -> Result<(), Box<dyn Error + 'static>> {
        let socket = UdpSocket::bind(SocketAddrV4::new(IP_UNSPECIFIED, self.port))?;
        socket.set_broadcast(true)?;
        socket.set_multicast_loop_v4(true)?;
        socket.join_multicast_v4(&multicast, &IP_UNSPECIFIED)?;
        socket.set_nonblocking(false)?;
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    pub fn send(&mut self, message: UdpMessage, addrs: Recepients) -> std::io::Result<usize> {
        let bytes = message.to_be_bytes();
        if let Some(socket) = &self.socket {
            let result = match addrs {
                Recepients::All => {
                    socket.send_to(&bytes, SocketAddrV4::new(IP_MULTICAST_DEFAULT, self.port))
                }
                Recepients::One(ip) => socket.send_to(&bytes, SocketAddrV4::new(ip, self.port)),
            };
            match &result {
                Ok(_num) => (), // debug!("Sent {num} bytes of '{:?}' to {addrs:?}", message.command),
                Err(err) => error!("Could't send '{:?}' to {addrs:?}: {err}", message.command),
            };
            result
        } else {
            Ok(0)
        }
    }

    pub fn handle_back_event(&mut self, event: BackEvent, ctx: &impl Repaintable) {
        match event {
            BackEvent::PeerJoined((ip, ref user_name)) => {
                let new_comer = self.peers.peer_joined(ip, user_name.clone());
                self.front_tx.send(event).ok();
                if new_comer {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    // let notification_text = format!("{} joined..", self.peers.get_display_name(ip));
                    // ctx.notify(&notification_text);
                    // } else {
                    // ctx.request_repaint();
                }
                ctx.request_repaint();
            }
            BackEvent::PeerLeft(ip) => {
                // let notification_text = format!("{} left..", self.peers.get_display_name(ip));
                self.peers.remove(&ip);
                self.front_tx.send(BackEvent::PeerLeft(ip)).ok();
                // ctx.notify(&notification_text);
                ctx.request_repaint();
            }
            BackEvent::Message(msg) => {
                if matches!(msg.content, Content::Seen) {
                    self.front_tx.send(BackEvent::Message(msg)).ok();
                    ctx.request_repaint();
                } else {
                    let text = msg.get_text();
                    let name = self.peers.get_display_name(msg.ip());
                    let notification_text = format!("{name}: {text}");
                    self.front_tx.send(BackEvent::Message(msg)).ok();
                    ctx.notify(&notification_text);
                }
            }
        }
    }

    pub fn handle_front_event(
        &mut self,
        outbox: &mut Outbox,
        ctx: &impl Repaintable,
        event: FrontEvent,
    ) -> ControlFlow<()> {
        match event {
            FrontEvent::Message(msg) => {
                // debug!("Sending: {}", msg.get_text());

                UdpMessage::send_message(&msg, self, outbox)
                    .inspect_err(|e| error!("{e}"))
                    .ok();

                self.front_tx.send(BackEvent::Message(msg)).ok();
                ctx.request_repaint();
            }
            FrontEvent::Ping(recepients) => {
                debug!("Ping {recepients:?}");
                self.send(UdpMessage::enter(&self.name), recepients)
                    .inspect_err(|e| error!("{e}"))
                    .ok();
            }
            FrontEvent::Exit => {
                debug!("I'm Exit");
                self.send(UdpMessage::exit(), Recepients::All)
                    .inspect_err(|e| error!("{e}"))
                    .ok();
                // self.sender.send(UdpMessage::exit(), Recepients::Myself);
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    pub fn handle_message(
        &mut self,
        inbox: &mut Inbox,
        outbox: &mut Outbox,
        ctx: &impl Repaintable,
        r_ip: Ipv4Addr,
        r_msg: UdpMessage,
        downloads_path: &Path,
    ) {
        if r_ip == self.ip {
            return;
        }
        // debug!("Received {:?} from {r_ip}", r_msg.command);
        let r_id = r_msg.id;

        match r_msg.command {
            Command::Enter | Command::Greating => {
                let user_name = String::from_utf8_lossy(&r_msg.data);
                self.handle_back_event(
                    BackEvent::PeerJoined((r_ip, Some(user_name.to_string()))),
                    ctx,
                );
                if r_msg.command == Command::Enter {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                }
                for undelivered in outbox.undelivered(r_ip) {
                    self.send(undelivered.clone(), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                }
            }

            Command::Exit => {
                self.handle_back_event(BackEvent::PeerLeft(r_ip), ctx);
            }
            Command::Text | Command::File | Command::Repeat => match r_msg.part {
                message::Part::Single => {
                    let txt_msg = TextMessage::from_message(r_ip, &r_msg, true);
                    self.incoming(r_ip); // FIXME
                    self.send(UdpMessage::seen(&txt_msg), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                }
                message::Part::Init(_) => {
                    debug!("incomint PartInit");

                    if let Some(inmsg) = InMessage::new(r_ip, r_msg) {
                        // TODO move to fn
                        let link = FileLink::new(
                            &inmsg.file_name,
                            downloads_path,
                            inmsg.count * DATA_LIMIT_BYTES as ShardCount,
                            inmsg.progress.clone(),
                        );
                        debug!("Creating link");

                        let txt_msg = TextMessage {
                            timestamp: inmsg.ts,
                            incoming: true,
                            public: inmsg.public,
                            ip: inmsg.sender,
                            id: inmsg.id,
                            content: Content::FileLink(link),
                            seen: Some(Seen::One),
                        };
                        self.send(UdpMessage::seen(&txt_msg), Recepients::One(inmsg.sender))
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                        self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                        inbox.0.insert(r_id, inmsg);
                    }
                }
                message::Part::Shard(count) => {
                    let mut completed = false;
                    if let Some(inmsg) = inbox.0.get_mut(&r_msg.id) {
                        completed = inmsg.insert(count, r_msg, self, ctx, downloads_path);
                    }
                    if completed {
                        inbox.0.remove(&r_id);
                    }
                }
                _ => (),
            },
            Command::Seen => {
                let txt_msg = TextMessage::from_message(r_ip, &r_msg, true);
                self.incoming(r_ip);
                // outbox.remove(r_ip, txt_msg.id());
                self.handle_back_event(BackEvent::Message(txt_msg), ctx);
            }
            Command::Error => {
                self.incoming(r_ip);
                self.send(
                    UdpMessage::new_single(
                        Command::AskToRepeat,
                        r_msg.id.to_be_bytes().to_vec(),
                        r_msg.public,
                    ),
                    Recepients::One(r_ip),
                )
                .inspect_err(|e| error!("{e}"))
                .ok();
            }

            Command::AskToRepeat => {
                self.incoming(r_ip);
                let id = r_msg.id;
                debug!("Asked to repeat {id}, part: {:?}", r_msg.part);
                // Resend my Name
                if id == 0 {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                } else if let message::Part::AskRange(range) = &r_msg.part {
                    let msg_text = r_msg.read_text();
                    debug!("{msg_text}");
                    if let Some(link) = outbox.files.get(&id) {
                        debug!("sending shards {range:?}");
                        match send_shards(
                            link,
                            range.to_owned(),
                            r_msg.id,
                            Recepients::One(r_ip),
                            self,
                        ) {
                            Ok(_) => (),
                            Err(err) => error!("{err}"),
                        }
                    }
                } else if let Some(message) = outbox.get(r_ip, id) {
                    debug!("Message found..");

                    let mut message = message.clone();
                    message.command = Command::Repeat;
                    self.send(message, Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                } else {
                    error!("Message not found!");
                }
            } // Command::File => todo!(),
        }
    }

    pub fn incoming(&mut self, ip: Ipv4Addr) {
        self.front_tx.send(BackEvent::PeerJoined((ip, None))).ok();
        match self.peers.0.get_mut(&ip) {
            None => {
                let noname: Option<&str> = None;
                self.peers.peer_joined(ip, noname);
                self.send(UdpMessage::enter(&self.name), Recepients::One(ip))
                    .inspect_err(|e| error!("{e}"))
                    .ok();
            }
            Some(peer) => {
                peer.set_last_time(SystemTime::now());
                if !peer.has_name() {
                    self.send(UdpMessage::enter(&self.name), Recepients::One(ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                }
            }
        };
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

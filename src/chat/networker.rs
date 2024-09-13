use crate::chat::{
    inbox::InMessage,
    message::{self, send_shards, Command, Part, ShardCount},
    TextMessage,
};

use super::{
    file::FileLink,
    message::{Id, UdpMessage},
    notifier::Repaintable,
    peers::PeersMap,
    BackEvent, Content, FrontEvent, Inbox, Outbox, Recepients,
};
use flume::Sender;
use log::{debug, error};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    ops::{ControlFlow, RangeInclusive},
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, SystemTime},
};

pub const TIMEOUT_ALIVE: Duration = Duration::from_secs(60);
pub const TIMEOUT_CHECK: Duration = Duration::from_secs(10);
pub const TIMEOUT_SECOND: Duration = Duration::from_secs(1);
pub const IP_MULTICAST_DEFAULT: Ipv4Addr = Ipv4Addr::new(225, 225, 225, 225);
pub const IP_UNSPECIFIED: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
pub type Port = u16;

pub struct NetWorker {
    pub name: String,
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: Port,
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

    pub fn send(&self, message: UdpMessage, addrs: Recepients) -> std::io::Result<usize> {
        if let Some(socket) = &self.socket {
            send(socket, self.port, message, addrs)
        } else {
            Ok(0)
        }
    }
    pub fn send_shards(
        &self,
        link: Arc<FileLink>,
        range: RangeInclusive<ShardCount>,
        id: Id,
        recepients: Recepients,
        ctx: &impl Repaintable,
    ) {
        if let Some(socket) = &self.socket {
            let socket = socket.clone();
            let port = self.port;
            let ctx = ctx.clone();

            thread::Builder::new()
                .name("shard_sender".to_string())
                .spawn(
                    move || match send_shards(link, range, id, recepients, socket, port, ctx) {
                        Ok(_) => (),
                        Err(err) => error!("{err}"),
                    },
                )
                .ok();
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
                }
                ctx.request_repaint();
            }
            BackEvent::PeerLeft(ip) => {
                self.peers.remove(&ip);
                self.front_tx.send(BackEvent::PeerLeft(ip)).ok();
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
                    let txt_msg = TextMessage::from_udp(r_ip, &r_msg, true);
                    self.incoming(r_ip); // FIXME
                    self.send(UdpMessage::seen_msg(&txt_msg), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                }
                message::Part::Init(_) => {
                    debug!("incomint PartInit");

                    if let Some(inmsg) = InMessage::new(r_ip, r_msg, downloads_path) {
                        let txt_msg = TextMessage::from_inmsg(&inmsg);
                        self.send(
                            UdpMessage::ask_to_repeat(inmsg.id, Part::AskRange(0..=inmsg.terminal)),
                            Recepients::One(r_ip),
                        )
                        .ok();
                        if inmsg.command == Command::File {
                            self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                        }
                        inbox.insert(r_id, inmsg);
                    }
                }
                message::Part::Shard(count) => {
                    let mut completed = false;
                    if let Some(inmsg) = inbox.get_mut(&r_id) {
                        completed = inmsg.insert(count, r_msg, self, ctx);
                    } else {
                        // FIXME
                        // self.send(UdpMessage::abort(r_id), Recepients::One(r_ip))
                        //     .inspect_err(|e| error!("{e}"))
                        //     .ok();
                    }

                    if completed {
                        inbox.remove(&r_id);
                        self.send(UdpMessage::seen_id(r_id, false), Recepients::One(r_ip))
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                    }
                }

                _ => (),
            },

            Command::Seen => {
                let txt_msg = TextMessage::from_udp(r_ip, &r_msg, true);
                self.incoming(r_ip);
                outbox.remove(r_ip, txt_msg.id());
                self.handle_back_event(BackEvent::Message(txt_msg), ctx);
            }
            Command::Abort => {
                debug!("ABORTING! {}", r_id);
                inbox.remove(&r_id);
                outbox.files.remove(&r_id);
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
                debug!("Asked to repeat {r_id}, part: {:?}", r_msg.part);
                // Resend my Name
                if r_id == 0 {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                } else if let message::Part::AskRange(range) = &r_msg.part {
                    if let Some(link) = outbox.files.get(&r_id) {
                        let completed = link.completed.load(std::sync::atomic::Ordering::Relaxed);
                        link.completed.store(
                            completed.saturating_sub(range.clone().count() as ShardCount),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        debug!("sending shards {range:?}");
                        self.send_shards(
                            link.clone(),
                            range.to_owned(),
                            r_id,
                            Recepients::One(r_ip),
                            ctx,
                        );
                    }
                } else if let Some(message) = outbox.get(r_ip, r_id) {
                    debug!("Message found..");

                    let mut message = message.clone();
                    message.command = Command::Repeat;
                    self.send(message, Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                } else {
                    self.send(UdpMessage::abort(r_id), Recepients::One(r_ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    error!("Message not found. Aborting transmission!");
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

pub fn send(
    socket: &Arc<UdpSocket>,
    port: Port,
    message: UdpMessage,
    addrs: Recepients,
) -> std::io::Result<usize> {
    let bytes = message.to_be_bytes();
    let result = match addrs {
        Recepients::All => socket.send_to(&bytes, SocketAddrV4::new(IP_MULTICAST_DEFAULT, port)),
        Recepients::One(ip) => socket.send_to(&bytes, SocketAddrV4::new(ip, port)),
    };
    match &result {
        Ok(_num) => (), // debug!("Sent {num} bytes of '{:?}' to {addrs:?}", message.command),
        Err(err) => error!("Could't send '{:?}' to {addrs:?}: {err}", message.command),
    };
    result
}

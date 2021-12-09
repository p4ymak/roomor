use eframe::{egui, epi};
use egui::*;
use epi::Storage;

// use local_ipaddress;
use std::io::{self, BufRead};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub struct ChatApp {
    socket: Option<Arc<UdpSocket>>,
    ip: String,
    port: usize,
    name: String,
    sync_sender: mpsc::SyncSender<[String; 2]>,
    sync_receiver: mpsc::Receiver<[String; 2]>,
    message: String,
    chat: Vec<[String; 2]>,
}

impl epi::App for ChatApp {
    fn name(&self) -> &str {
        "Chat"
    }
    fn warm_up_enabled(&self) -> bool {
        true
    }
    // fn persist_native_window(&self) -> bool {
    //     false
    // }
    // fn persist_egui_memory(&self) -> bool {
    //     false
    // }
    // fn auto_save_interval(&self) -> Duration {
    //     Duration::MAX
    // }
    fn setup(
        &mut self,
        _ctx: &egui::CtxRef,
        _frame: &mut epi::Frame<'_>,
        _storage: Option<&dyn Storage>,
    ) {
        self.port = 4444;
        if let Some(my_ip) = local_ipaddress::get() {
            self.ip = my_ip.to_owned();
            self.socket = match UdpSocket::bind(format!("{}:{}", my_ip, self.port)) {
                Ok(socket) => Some(Arc::new(socket)),
                _ => None,
            };
        }

        let reader = self.socket.clone().unwrap();
        let sender = self.sync_sender.clone();
        thread::spawn(move || {
            let mut buf = [0; 32];
            loop {
                if let Ok((number_of_bytes, src_addr)) = reader.recv_from(&mut buf) {
                    let filled_buf = std::str::from_utf8(&buf[..number_of_bytes]).unwrap();
                    println!("{:?}:  {:?}", src_addr, filled_buf);
                    sender
                        .send([src_addr.ip().to_string(), filled_buf.to_string()])
                        .unwrap();
                }
            }
        });

        self.message = "---- ENTERED ----".to_string();
        self.send();
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        self.read();
        egui::TopBottomPanel::top("socket").show(ctx, |ui| {
            ui.label(format!("{}:{}", self.ip, self.port));
        });
        egui::TopBottomPanel::bottom("my_panel").show(ctx, |ui| {
            let message_box = ui.add(
                egui::TextEdit::multiline(&mut self.message)
                    .desired_width(f32::INFINITY)
                    .code_editor()
                    .id(egui::Id::new("text_input")),
            );
            message_box.request_focus();
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.chat.is_empty() {
                egui::ScrollArea::vertical()
                    .max_width(f32::INFINITY)
                    .always_show_scroll(true)
                    .stick_to_bottom()
                    .show(ui, |ui| {
                        self.chat.iter().for_each(|m| {
                            if m[0] == self.ip {
                                ui.with_layout(egui::Layout::right_to_left(), |line| {
                                    line.add(egui::Label::new(&m[0]).wrap(true).strong());
                                    line.add(
                                        egui::Button::new(&m[1])
                                            .wrap(true)
                                            .text_style(egui::TextStyle::Heading)
                                            .fill(egui::Color32::from_rgb(44, 44, 44)),
                                    );
                                });
                            } else {
                                ui.with_layout(egui::Layout::left_to_right(), |line| {
                                    line.add(egui::Label::new(&m[0]).wrap(true).strong());
                                    line.add(
                                        egui::Button::new(&m[1])
                                            .wrap(true)
                                            .text_style(egui::TextStyle::Heading)
                                            .fill(egui::Color32::from_rgb(44, 44, 44)),
                                    );
                                });
                            }
                        });
                    });
            }
        });

        self.handle_keys(ctx);
    }
}
impl Default for ChatApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel::<[String; 2]>(0);
        ChatApp {
            socket: None,
            ip: "0.0.0.0".to_string(),
            port: 4444,
            name: "Unknown".to_string(),
            sync_sender: tx,
            sync_receiver: rx,
            message: String::new(),
            chat: Vec::<[String; 2]>::new(),
        }
    }
}
impl ChatApp {
    fn send(&mut self) {
        if !self.message.trim().is_empty() {
            if let Some(socket) = &self.socket {
                for i in 0..=255 {
                    let destination = format!("192.168.0.{}:{}", i, self.port);
                    socket
                        .send_to(self.message.trim().as_bytes(), &destination)
                        .ok();
                }
            }
        }
        self.message = String::new();
        // self.read();
    }
    // fn read(&mut self) {
    //     let reader = self.socket.clone().unwrap();
    //     let read = thread::spawn(move || {
    //         let mut buf = [0; 32];
    //         if let Ok((number_of_bytes, src_addr)) = reader.recv_from(&mut buf) {
    //             let filled_buf = std::str::from_utf8(&buf[..number_of_bytes]).unwrap();
    //             println!("{:?}:  {:?}", src_addr, filled_buf);
    //             return [src_addr.ip().to_string(), filled_buf.to_string()];
    //         }
    //         ["0.0.0.0".to_string(), "".to_string()]
    //     });
    //     if let Ok(message) = read.join() {
    //         if !message.is_empty() {
    //             self.chat.push(message);
    //         }
    //     }
    // }
    fn read(&mut self) {
        println!("read");
        if let Ok(message) = self.sync_receiver.try_recv() {
            println!("read message");
            if !message.is_empty() {
                println!("read message ok");
                self.chat.push(message);
            }
        }
        println!("read_ok");
    }
    fn handle_keys(&mut self, ctx: &egui::CtxRef) {
        for event in &ctx.input().raw.events {
            match event {
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => self.send(),
                _ => (),
            }
        }
    }
}

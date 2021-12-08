use eframe::{egui, epi};
use egui::*;
use epi::Storage;

use local_ipaddress;
use std::io::{self, BufRead};
use std::net::UdpSocket;
use std::sync::Arc;
use std::thread;

#[derive(Default)]
pub struct ChatApp {
    socket: Option<Arc<UdpSocket>>,
    ip: String,
    port: usize,
    name: String,
    message: String,
    chat: Vec<String>,
}

impl epi::App for ChatApp {
    fn name(&self) -> &str {
        "Chat"
    }
    // fn warm_up_enabled(&self) -> bool {
    //     true
    // }
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
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        egui::TopBottomPanel::top("socket").show(ctx, |ui| {
            ui.label(format!("{}:{}", self.ip, self.port));
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(self.chat.join("\n"));
        });

        egui::TopBottomPanel::bottom("my_panel").show(ctx, |ui| {
            let message_box = ui.add(egui::TextEdit::singleline(&mut self.message));
            let send = ui.add(egui::Button::new("Send"));
            if send.clicked() {
                self.send();
                self.message = String::new();
            }
        });
    }
}
impl ChatApp {
    fn send(&mut self) {
        if let Some(socket) = &self.socket {
            for i in 0..=255 {
                let destination = format!("192.168.0.{}:{}", i, self.port);
                socket.send_to(self.message.as_bytes(), &destination).ok();
            }
        }
        self.read();
    }
    fn _read(&mut self) {
        let mut buf = [0; 32];
        if let Some(socket) = &self.socket {
            if let Ok((number_of_bytes, src_addr)) = socket.recv_from(&mut buf) {
                let filled_buf = std::str::from_utf8(&buf[..number_of_bytes]).unwrap();
                self.chat.push(filled_buf.to_string());
            }
        }
    }
    fn read(&mut self) {
        let reader = self.socket.clone().unwrap();
        let read = thread::spawn(move || {
            let mut buf = [0; 32];
            if let Ok((number_of_bytes, src_addr)) = reader.recv_from(&mut buf) {
                let filled_buf = std::str::from_utf8(&buf[..number_of_bytes]).unwrap();
                println!("{:?}:  {:?}", src_addr, filled_buf);
                return filled_buf.to_string();
            }
            "".to_string()
        });
        if let Ok(message) = read.join() {
            if !message.is_empty() {
                self.chat.push(message);
            }
        }
    }
}

use local_ip_address::local_ip;
use std::io::{self, BufRead};
use std::net::UdpSocket;
use std::sync::Arc;
use std::thread;

fn main() {
    let mut username = String::new();
    let stdin = io::stdin();
    stdin.lock().read_line(&mut username).unwrap();
    let my_ip = local_ip().expect("Couldn't retrieve local IP");
    let port = 4400;
    let socket =
        Arc::new(UdpSocket::bind(format!("{}:{}", my_ip, port)).expect("Couldn't bind to address"));

    let reader = socket.clone();
    thread::spawn(move || {
        let mut buf = [0; 32];
        if let Ok((number_of_bytes, src_addr)) = reader.recv_from(&mut buf) {
            let filled_buf = std::str::from_utf8(&buf[..number_of_bytes]).unwrap();
            println!("{:?}:  {:?}", src_addr, filled_buf);
        }
    });

    for i in 0..=255 {
        let destination = format!("192.168.0.{}:{}", i, port);
        socket.send_to(username.as_bytes(), &destination).ok();
    }
}

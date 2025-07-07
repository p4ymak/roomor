use crate::chat::{
    message::{Command, Part, UdpMessage},
    peers::PeerId,
};

#[test]
pub fn protocol() {
    for cmd_id in 0..=10 {
        let cmd = Command::from_code(cmd_id);
        let peer_id = PeerId::new("name", "device");
        let msg = match cmd {
            Command::Enter => UdpMessage::enter(peer_id, "name"),
            Command::Greating => UdpMessage::greating(peer_id, "name"),
            Command::Text => UdpMessage::new_single(peer_id, Command::Text, vec![], false),
            Command::File => UdpMessage::new_single(peer_id, Command::File, vec![], false),
            Command::AskToRepeat => UdpMessage::ask_to_repeat(peer_id, 4, Part::Single, true),
            Command::Repeat => UdpMessage::new_single(peer_id, Command::Text, vec![], false),
            Command::Exit => UdpMessage::exit(peer_id),
            Command::Seen => UdpMessage::seen_id(peer_id, 0, true),
            Command::Error => UdpMessage::new_single(peer_id, Command::Error, vec![], true),
            Command::Abort => UdpMessage::abort(peer_id, 5),
        };
        let bytes = msg.to_be_bytes();
        let converted = UdpMessage::from_be_bytes(&bytes);
        println!("{converted:?}");
        assert_eq!(converted.ok(), Some(msg));
    }
}

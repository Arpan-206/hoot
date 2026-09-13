//! Captive-portal DNS: every name resolves to the Sprig, so the phone's
//! connectivity check lands on the setup page.

use embassy_net::Stack;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_time::Timer;
use sprig_proto::dns;

use super::PORTAL_IP;

pub async fn serve(stack: Stack<'_>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1024];
    let mut sock = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if sock.bind(53).is_err() {
        warn!("dns: bind failed");
        loop {
            Timer::after_secs(60).await;
        }
    }

    let mut pkt = [0u8; 512];
    let mut reply = [0u8; 512];
    loop {
        let Ok((n, meta)) = sock.recv_from(&mut pkt).await else { continue };
        if let Some(len) = dns::build_reply(&pkt[..n], PORTAL_IP, &mut reply) {
            let mut name = [0u8; 80];
            let shown = dns::question_name(&pkt[..n], &mut name).unwrap_or(0);
            info!("dns: {}", core::str::from_utf8(&name[..shown]).unwrap_or("?"));
            let _ = sock.send_to(&reply[..len], meta.endpoint).await;
        }
    }
}

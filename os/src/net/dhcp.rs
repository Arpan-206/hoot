//! DHCP server for the setup hotspot: one /24, addresses handed out in
//! order of appearance, the Sprig as router and DNS.

use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Stack};
use embassy_time::Timer;
use sprig_proto::dhcp::{self, REPLY_MAX};

use super::PORTAL_IP;

const LEASE_SECS: u32 = 3600;
const CLIENTS: usize = 4;

pub async fn serve(stack: Stack<'_>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1024];
    let mut sock = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if sock.bind(67).is_err() {
        warn!("dhcp: bind failed");
        loop {
            Timer::after_secs(60).await;
        }
    }

    let mut macs = [[0u8; 6]; CLIENTS];
    let mut used = 0usize;
    let mut pkt = [0u8; 600];
    let mut reply = [0u8; REPLY_MAX];
    loop {
        let Ok((n, _)) = sock.recv_from(&mut pkt).await else { continue };
        let Some(req) = dhcp::parse(&pkt[..n]) else { continue };
        let index = match macs[..used].iter().position(|m| *m == req.mac) {
            Some(i) => i,
            None if used < CLIENTS => {
                macs[used] = req.mac;
                used += 1;
                used - 1
            }
            None => 0,
        };
        let client_ip = [PORTAL_IP[0], PORTAL_IP[1], PORTAL_IP[2], 2 + index as u8];
        if let Some(len) = dhcp::build_reply(&req, PORTAL_IP, client_ip, LEASE_SECS, &mut reply) {
            // The client has no address yet, so the answer goes to everyone.
            let to = IpEndpoint::new(IpAddress::v4(255, 255, 255, 255), 68);
            match sock.send_to(&reply[..len], to).await {
                Ok(()) => info!("dhcp: {:?} -> .{}", req.msg_type, client_ip[3]),
                Err(_) => warn!("dhcp: send failed"),
            }
        }
    }
}

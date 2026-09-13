//! Just enough DHCPv4 (RFC 2131) for a setup hotspot: answer DISCOVER with
//! an OFFER and REQUEST with an ACK, one subnet, fixed lease.

const MAGIC: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
const OPT_SUBNET: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;

/// Smallest reply we produce: fixed header plus options.
pub const REPLY_MAX: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgType {
    Discover,
    Request,
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub xid: [u8; 4],
    pub mac: [u8; 6],
    pub msg_type: MsgType,
    pub requested_ip: Option<[u8; 4]>,
}

/// Parse a client message. Returns `None` for anything that is not a
/// well-formed BOOTREQUEST with the DHCP magic cookie.
pub fn parse(pkt: &[u8]) -> Option<Request> {
    if pkt.len() < 240 || pkt[0] != 1 || pkt[1] != 1 || pkt[2] != 6 || pkt[236..240] != MAGIC {
        return None;
    }
    let mut req = Request {
        xid: pkt[4..8].try_into().ok()?,
        mac: pkt[28..34].try_into().ok()?,
        msg_type: MsgType::Other(0),
        requested_ip: None,
    };
    let mut i = 240;
    while i < pkt.len() {
        let code = pkt[i];
        if code == OPT_END {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *pkt.get(i + 1)? as usize;
        let data = pkt.get(i + 2..i + 2 + len)?;
        match code {
            OPT_MSG_TYPE if len == 1 => {
                req.msg_type = match data[0] {
                    1 => MsgType::Discover,
                    3 => MsgType::Request,
                    other => MsgType::Other(other),
                }
            }
            OPT_REQUESTED_IP if len == 4 => req.requested_ip = data.try_into().ok(),
            _ => {}
        }
        i += 2 + len;
    }
    Some(req)
}

/// Build an OFFER (for a DISCOVER) or ACK (for a REQUEST) giving `client_ip`
/// on a /24 where `server_ip` is router and DNS. Returns the length, or
/// `None` when the request needs no answer or `out` is too small.
pub fn build_reply(req: &Request, server_ip: [u8; 4], client_ip: [u8; 4], lease_secs: u32, out: &mut [u8]) -> Option<usize> {
    let reply_type: u8 = match req.msg_type {
        MsgType::Discover => 2, // OFFER
        MsgType::Request => 5,  // ACK
        MsgType::Other(_) => return None,
    };
    if out.len() < REPLY_MAX {
        return None;
    }
    out[..240].fill(0);
    out[0] = 2; // BOOTREPLY
    out[1] = 1; // ethernet
    out[2] = 6;
    out[4..8].copy_from_slice(&req.xid);
    out[10] = 0x80; // flags: broadcast, the client has no address yet
    out[16..20].copy_from_slice(&client_ip); // yiaddr
    out[20..24].copy_from_slice(&server_ip); // siaddr
    out[28..34].copy_from_slice(&req.mac);
    out[236..240].copy_from_slice(&MAGIC);
    let mut i = 240;
    let mut put = |code: u8, data: &[u8]| {
        out[i] = code;
        out[i + 1] = data.len() as u8;
        out[i + 2..i + 2 + data.len()].copy_from_slice(data);
        i += 2 + data.len();
    };
    put(OPT_MSG_TYPE, &[reply_type]);
    put(OPT_SERVER_ID, &server_ip);
    put(OPT_LEASE, &lease_secs.to_be_bytes());
    put(OPT_SUBNET, &[255, 255, 255, 0]);
    put(OPT_ROUTER, &server_ip);
    put(OPT_DNS, &server_ip);
    out[i] = OPT_END;
    Some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discover(mac: [u8; 6]) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        p[0] = 1;
        p[1] = 1;
        p[2] = 6;
        p[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        p[28..34].copy_from_slice(&mac);
        p[236..240].copy_from_slice(&MAGIC);
        p.extend_from_slice(&[OPT_MSG_TYPE, 1, 1, OPT_END]);
        p
    }

    #[test]
    fn parses_discover() {
        let mac = [1, 2, 3, 4, 5, 6];
        let r = parse(&discover(mac)).unwrap();
        assert_eq!(r.msg_type, MsgType::Discover);
        assert_eq!(r.mac, mac);
        assert_eq!(r.xid, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(r.requested_ip, None);
    }

    #[test]
    fn parses_request_with_requested_ip() {
        let mut p = discover([9; 6]);
        p.truncate(240);
        p.extend_from_slice(&[OPT_MSG_TYPE, 1, 3, OPT_REQUESTED_IP, 4, 192, 168, 4, 2, OPT_END]);
        let r = parse(&p).unwrap();
        assert_eq!(r.msg_type, MsgType::Request);
        assert_eq!(r.requested_ip, Some([192, 168, 4, 2]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[0u8; 100]).is_none());
        let mut p = discover([1; 6]);
        p[236] = 0; // bad magic
        assert!(parse(&p).is_none());
    }

    #[test]
    fn builds_offer_and_ack() {
        let r = parse(&discover([1, 2, 3, 4, 5, 6])).unwrap();
        let mut out = [0u8; REPLY_MAX];
        let n = build_reply(&r, [192, 168, 4, 1], [192, 168, 4, 2], 3600, &mut out).unwrap();
        assert_eq!(out[0], 2);
        assert_eq!(&out[4..8], &r.xid);
        assert_eq!(&out[16..20], &[192, 168, 4, 2]);
        assert_eq!(&out[28..34], &r.mac);
        assert_eq!(&out[236..240], &MAGIC);
        assert_eq!(&out[240..243], &[OPT_MSG_TYPE, 1, 2]);
        assert_eq!(out[n - 1], OPT_END);
        let ack = Request { msg_type: MsgType::Request, ..r };
        let n = build_reply(&ack, [192, 168, 4, 1], [192, 168, 4, 2], 3600, &mut out).unwrap();
        assert_eq!(&out[240..243], &[OPT_MSG_TYPE, 1, 5]);
        assert!(n < REPLY_MAX);
        let other = Request { msg_type: MsgType::Other(7), ..r };
        assert_eq!(build_reply(&other, [0; 4], [0; 4], 1, &mut out), None);
    }
}

//! The setup hotspot and captive portal.
//!
//! Flow: scan for networks while still a station, start an open access
//! point called `Sprig-Setup`, give the Sprig a fixed address, and run
//! DHCP, DNS and a small web server until the user saves settings or the
//! portal times out. Phones open the page by themselves because every DNS
//! name resolves to the Sprig and every unknown path redirects to `/`.

use cyw43::{Control, ScanOptions};
use embassy_futures::select::{Either3, select3};
use embassy_net::tcp::{State, TcpSocket};
use embassy_net::{ConfigV4, DhcpConfig, Ipv4Address, Ipv4Cidr, Stack, StaticConfigV4};
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::Write;
use sprig_proto::record::FixedStr;
use sprig_proto::{form, http};

use super::{PORTAL_IP, PORTAL_SSID, PortalResult, dhcp, dns, with};
use crate::ui::text::{StrBuf, format};

const CHANNEL: u8 = 6;
const SCAN_TIMEOUT: Duration = Duration::from_secs(8);
/// Give up and retry the saved network after this long, like the ESP32 frame.
pub const PORTAL_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_NETWORKS: usize = 10;
const HEAD_MAX: usize = 1536;
const BODY_MAX: usize = 512;

#[derive(Clone, Copy, Default)]
struct Network {
    ssid: FixedStr<32>,
    rssi: i16,
}

/// Run the portal until the user saves settings, or the timeout passes.
pub async fn run(control: &mut Control<'_>, stack: Stack<'_>) -> Option<PortalResult> {
    let mut networks = [Network::default(); MAX_NETWORKS];
    let count = scan(control, &mut networks).await;
    info!("portal: {} networks found", count);

    control.start_ap_open(PORTAL_SSID, CHANNEL).await;
    stack.set_config_v4(ConfigV4::Static(StaticConfigV4 {
        address: Ipv4Cidr::new(
            Ipv4Address::new(PORTAL_IP[0], PORTAL_IP[1], PORTAL_IP[2], PORTAL_IP[3]),
            24,
        ),
        gateway: None,
        dns_servers: Default::default(),
    }));
    info!("portal: hotspot '{}' up at 192.168.4.1", PORTAL_SSID);

    let (server, name) = with(|s| (s.server, s.name));
    // Phones probe with several connections at once and open the page right
    // after the probe. Three web server sockets keep none of them waiting.
    let found = &networks[..count];
    let web = select3(
        serve_http(stack, found, server, name),
        serve_http(stack, found, server, name),
        serve_http(stack, found, server, name),
    );
    let servers = select3(dhcp::serve(stack), dns::serve(stack), web);
    let result = match with_timeout(PORTAL_TIMEOUT, servers).await {
        Ok(Either3::Third(Either3::First(r) | Either3::Second(r) | Either3::Third(r))) => Some(r),
        Ok(_) => None,
        Err(_) => {
            info!("portal: timed out");
            None
        }
    };

    control.close_ap().await;
    stack.set_config_v4(ConfigV4::Dhcp(DhcpConfig::default()));
    result
}

/// Collect nearby network names, strongest first.
async fn scan(control: &mut Control<'_>, out: &mut [Network]) -> usize {
    let mut count = 0;
    let mut scanner = control.scan(ScanOptions::default()).await;
    let _ = with_timeout(SCAN_TIMEOUT, async {
        while let Some(bss) = scanner.next().await {
            let len = (bss.ssid_len as usize).min(32);
            let Ok(ssid) = core::str::from_utf8(&bss.ssid[..len]) else { continue };
            if ssid.is_empty() {
                continue;
            }
            if let Some(seen) = out[..count].iter_mut().find(|n| n.ssid.as_str() == ssid) {
                seen.rssi = seen.rssi.max(bss.rssi);
                continue;
            }
            if count < out.len() {
                out[count] = Network { ssid: FixedStr::truncated(ssid), rssi: bss.rssi };
                count += 1;
            }
        }
    })
    .await;
    out[..count].sort_unstable_by_key(|n| core::cmp::Reverse(n.rssi));
    count
}

async fn serve_http(
    stack: Stack<'_>,
    networks: &[Network],
    server: FixedStr<96>,
    name: FixedStr<24>,
) -> PortalResult {
    let mut rx = [0u8; 2048];
    let mut tx = [0u8; 2048];
    let mut sock = TcpSocket::new(stack, &mut rx, &mut tx);
    sock.set_timeout(Some(Duration::from_secs(10)));
    let mut head = [0u8; HEAD_MAX];
    loop {
        if sock.accept(80).await.is_err() {
            sock.abort();
            Timer::after_millis(100).await;
            continue;
        }
        let outcome = handle(&mut sock, &mut head, networks, &server, &name).await;
        finish(&mut sock).await;
        if let Some(result) = outcome {
            return result;
        }
    }
}

/// Serve one request. Returns the settings when the user saved them.
async fn handle(
    sock: &mut TcpSocket<'_>,
    head: &mut [u8; HEAD_MAX],
    networks: &[Network],
    server: &FixedStr<96>,
    name: &FixedStr<24>,
) -> Option<PortalResult> {
    let mut filled = 0;
    let head_end = loop {
        let n = sock.read(&mut head[filled..]).await.ok()?;
        if n == 0 {
            return None;
        }
        filled += n;
        if let Some(end) = http::head_len(&head[..filled]) {
            break end;
        }
        if filled == HEAD_MAX {
            return None;
        }
    };
    let (method, path) = http::parse_request_line(&head[..head_end])?;
    let is_post = method == "POST";
    let path: FixedStr<64> = FixedStr::truncated(path);
    info!("portal: {} {}", method, path.as_str());

    match (is_post, path.as_str()) {
        (false, "/") => {
            send_page(sock, networks, server, name).await;
            None
        }
        (true, "/save") => {
            let length: usize = http::header_value(&head[..head_end], "Content-Length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(BODY_MAX);
            let mut body = [0u8; BODY_MAX];
            let mut got = (filled - head_end).min(length);
            body[..got].copy_from_slice(&head[head_end..head_end + got]);
            while got < length {
                let n = sock.read(&mut body[got..length]).await.ok()?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            let text = core::str::from_utf8(&body[..got]).unwrap_or("");
            let mut result = PortalResult {
                ssid: FixedStr::new(),
                password: FixedStr::new(),
                server: *server,
                name: *name,
            };
            form::fields(text, |key, value| match key {
                "ssid" => {
                    result.ssid.set(value.trim());
                }
                "password" => {
                    result.password.set(value);
                }
                "server" => {
                    let v = value.trim().trim_end_matches('/');
                    result.server.clear();
                    if !v.starts_with("http://") && !v.is_empty() {
                        result.server.push_str("http://");
                    }
                    result.server.push_str(v);
                }
                "name" => {
                    result.name.set(value.trim());
                }
                _ => {}
            });
            if result.ssid.is_empty() {
                send_page(sock, networks, server, name).await;
                return None;
            }
            send_saved(sock, result.ssid.as_str()).await;
            Some(result)
        }
        // Connectivity checks from phones and laptops land here.
        _ => {
            send_redirect(sock).await;
            None
        }
    }
}

const PAGE_HEAD: &str = "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Sprig setup</title><style>body{font-family:system-ui,sans-serif;margin:0;background:#0e1216;color:#e8eef4}.w{max-width:420px;margin:0 auto;padding:20px}h1{color:#3ddc84;font-size:22px;margin:0 0 4px}p{color:#8b949e;margin:0 0 12px}label{display:block;margin:14px 0 6px;color:#8b949e}input{width:100%;padding:12px;font-size:16px;border-radius:8px;border:1px solid #2a333d;background:#161c24;color:#e8eef4;box-sizing:border-box}button{width:100%;padding:14px;margin-top:18px;font-size:16px;border:0;border-radius:8px;background:#3ddc84;color:#0e1216;font-weight:600}.n{display:block;width:100%;text-align:left;margin:6px 0;padding:10px 12px;background:#161c24;color:#e8eef4;border:1px solid #2a333d;border-radius:8px;font-weight:400}</style></head><body><div class=w><h1>Sprig setup</h1><p>Pick your Wi-Fi and save. The Sprig connects by itself.</p><form method=post action=/save><label>Wi-Fi network</label><input name=ssid id=ssid required autocomplete=off><div>";
const PAGE_MID_A: &str = "</div><label>Password</label><input name=password type=password autocomplete=off><label>Photo server</label><input name=server value=\"";
const PAGE_MID_B: &str = "\"><label>Frame name</label><input name=name value=\"";
const PAGE_TAIL: &str = "\"><button>Save and connect</button></form></div><script>for(const b of document.querySelectorAll('.n'))b.onclick=e=>{e.preventDefault();ssid.value=b.textContent}</script></body></html>";

async fn send_page(sock: &mut TcpSocket<'_>, networks: &[Network], server: &FixedStr<96>, name: &FixedStr<24>) {
    // Network buttons, HTML-escaped, as many as fit.
    let mut list = [0u8; 1024];
    let mut list_len = 0;
    for n in networks {
        let mut escaped = [0u8; 200];
        let Some(e) = form::html_escape(n.ssid.as_str(), &mut escaped) else { continue };
        let open = b"<button class=n type=button>";
        let close = b"</button>";
        let need = open.len() + e + close.len();
        if list_len + need > list.len() {
            break;
        }
        list[list_len..list_len + open.len()].copy_from_slice(open);
        list_len += open.len();
        list[list_len..list_len + e].copy_from_slice(&escaped[..e]);
        list_len += e;
        list[list_len..list_len + close.len()].copy_from_slice(close);
        list_len += close.len();
    }
    let mut server_esc = [0u8; 200];
    let server_len = form::html_escape(server.as_str(), &mut server_esc).unwrap_or(0);
    let mut name_esc = [0u8; 64];
    let name_len = form::html_escape(name.as_str(), &mut name_esc).unwrap_or(0);

    let total = PAGE_HEAD.len() + list_len + PAGE_MID_A.len() + server_len + PAGE_MID_B.len() + name_len + PAGE_TAIL.len();
    let header: StrBuf<160> = format(format_args!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {total}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    ));
    let parts: [&[u8]; 7] = [
        header.as_str().as_bytes(),
        PAGE_HEAD.as_bytes(),
        &list[..list_len],
        PAGE_MID_A.as_bytes(),
        &server_esc[..server_len],
        PAGE_MID_B.as_bytes(),
        &name_esc[..name_len],
    ];
    for part in parts {
        if sock.write_all(part).await.is_err() {
            return;
        }
    }
    let _ = sock.write_all(PAGE_TAIL.as_bytes()).await;
}

async fn send_saved(sock: &mut TcpSocket<'_>, ssid: &str) {
    let mut esc = [0u8; 200];
    let n = form::html_escape(ssid, &mut esc).unwrap_or(0);
    const A: &str = "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Saved</title><style>body{font-family:system-ui,sans-serif;background:#0e1216;color:#e8eef4;margin:0}.w{max-width:420px;margin:0 auto;padding:24px}h1{color:#3ddc84}p{color:#8b949e}</style></head><body><div class=w><h1>Saved</h1><p>The Sprig is now connecting to <b>";
    const B: &str = "</b>. You can close this page and rejoin your own Wi-Fi.</p></div></body></html>";
    let total = A.len() + n + B.len();
    let header: StrBuf<160> = format(format_args!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
    ));
    for part in [header.as_str().as_bytes(), A.as_bytes(), &esc[..n], B.as_bytes()] {
        if sock.write_all(part).await.is_err() {
            return;
        }
    }
}

async fn send_redirect(sock: &mut TcpSocket<'_>) {
    // A body with a refresh helps clients that render the 302 instead of
    // following it.
    const BODY: &str = "<html><head><meta http-equiv=\"refresh\" content=\"0;url=http://192.168.4.1/\"></head><body><a href=\"http://192.168.4.1/\">Sprig setup</a></body></html>";
    let header: StrBuf<200> = format(format_args!(
        "HTTP/1.1 302 Found\r\nLocation: http://192.168.4.1/\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        BODY.len()
    ));
    if sock.write_all(header.as_str().as_bytes()).await.is_ok() {
        let _ = sock.write_all(BODY.as_bytes()).await;
    }
}

/// Flush, close politely, then reset so the socket can accept again.
/// Kept short: a phone opens its next connection right after the reply.
async fn finish(sock: &mut TcpSocket<'_>) {
    let _ = with_timeout(Duration::from_millis(300), sock.flush()).await;
    sock.close();
    let _ = with_timeout(Duration::from_millis(200), async {
        while !matches!(sock.state(), State::Closed | State::TimeWait) {
            Timer::after_millis(10).await;
        }
    })
    .await;
    sock.abort();
}

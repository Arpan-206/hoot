//! A small HTTP/1.1 GET client over `embassy-net`.
//!
//! Plain HTTP only. Bodies stream into a blob slot or a small RAM buffer.

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Stack};
use embassy_time::{Duration, with_timeout};
use embedded_io_async::Write;
use hoot_proto::record::FixedStr;
use hoot_proto::{http, url};

use super::{Body, FetchError, FetchRequest, FetchResult, SMALL_BODY_MAX, Sink};
use crate::ota;
use crate::storage::{BLOB_DATA_MAX, BlobWriter, FlashMutex};

const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// Reads fail after this long without data, like the ESP32 frame.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const HEAD_MAX: usize = 1024;

/// Fetch `req`. `small` receives the body for `Sink::Small`.
pub async fn fetch(
    stack: Stack<'_>,
    req: &FetchRequest,
    flash: &'static FlashMutex,
    small: &mut [u8; SMALL_BODY_MAX],
) -> Result<FetchResult, FetchError> {
    let u = url::parse(req.url.as_str()).map_err(|_| FetchError::BadUrl)?;

    let ip = match url::parse_ipv4(u.host) {
        Some(o) => Ipv4Address::new(o[0], o[1], o[2], o[3]),
        None => {
            let answers = with_timeout(DNS_TIMEOUT, stack.dns_query(u.host, DnsQueryType::A))
                .await
                .map_err(|_| FetchError::Timeout)?
                .map_err(|_| FetchError::Dns)?;
            match answers.first() {
                Some(IpAddress::Ipv4(a)) => *a,
                _ => return Err(FetchError::Dns),
            }
        }
    };

    let mut rx = [0u8; 2048];
    let mut tx = [0u8; 512];
    let mut sock = TcpSocket::new(stack, &mut rx, &mut tx);
    sock.set_timeout(Some(IO_TIMEOUT));
    with_timeout(
        CONNECT_TIMEOUT,
        sock.connect(IpEndpoint::new(IpAddress::Ipv4(ip), u.port)),
    )
    .await
    .map_err(|_| FetchError::Timeout)?
    .map_err(|_| FetchError::Connect)?;

    // Host header: name plus port when it is not the default.
    let mut host = FixedStr::<96>::truncated(u.host);
    if u.port != 80 {
        let mut port = [0u8; 6];
        host.push_str(itoa(u.port as u32, &mut port));
    }
    let ims = (!req.if_modified_since.is_empty()).then(|| req.if_modified_since.as_str());
    let mut reqbuf = [0u8; 640];
    match req.body {
        Body::None => {
            let n = http::write_get(&mut reqbuf, host.as_str(), u.path, ims, req.headers.as_str())
                .ok_or(FetchError::BadUrl)?;
            sock.write_all(&reqbuf[..n]).await.map_err(|_| FetchError::Connect)?;
        }
        Body::RecentWarnings => {
            let mut lines = [0u8; 1024];
            let len = crate::logging::drain_into(&mut lines);
            let n = http::write_post(&mut reqbuf, host.as_str(), u.path, "text/plain", len, req.headers.as_str())
                .ok_or(FetchError::BadUrl)?;
            sock.write_all(&reqbuf[..n]).await.map_err(|_| FetchError::Connect)?;
            sock.write_all(&lines[..len]).await.map_err(|_| FetchError::Connect)?;
        }
        Body::Form(form) => {
            let body = form.as_str().as_bytes();
            let n = http::write_post(
                &mut reqbuf,
                host.as_str(),
                u.path,
                "application/x-www-form-urlencoded",
                body.len(),
                req.headers.as_str(),
            )
            .ok_or(FetchError::BadUrl)?;
            sock.write_all(&reqbuf[..n]).await.map_err(|_| FetchError::Connect)?;
            sock.write_all(body).await.map_err(|_| FetchError::Connect)?;
        }
    }

    // Read until the blank line that ends the head.
    let mut buf = [0u8; HEAD_MAX];
    let mut filled = 0;
    let head_end = loop {
        let n = sock.read(&mut buf[filled..]).await.map_err(|_| FetchError::Protocol)?;
        if n == 0 {
            return Err(FetchError::Protocol);
        }
        filled += n;
        if let Some(end) = http::head_len(&buf[..filled]) {
            break end;
        }
        if filled == HEAD_MAX {
            return Err(FetchError::Protocol);
        }
    };
    let head = http::parse_head(&buf[..head_end]).map_err(|_| FetchError::Protocol)?;
    let mut result = FetchResult {
        status: head.status,
        len: 0,
        last_modified: FixedStr::truncated(head.last_modified.unwrap_or("")),
        crc32: 0,
        command: FixedStr::truncated(head.command.unwrap_or("")),
        unread: head.unread.unwrap_or(0),
        time: head.time,
        tz_min: head.tz_min.unwrap_or(0),
        live: head.live.unwrap_or(0),
        goals_stamp: head.goals_stamp.unwrap_or(0),
    };
    if head.status != 200 {
        sock.close();
        return Ok(result);
    }
    if head.chunked {
        return Err(FetchError::Chunked);
    }
    let content_length = head.content_length;

    // Body: whatever came with the head first, then the rest of the stream.
    let mut total: usize = 0;
    match req.sink {
        Sink::Blob { slot, kind, seq } => {
            if content_length.is_some_and(|l| l as u32 > BLOB_DATA_MAX) {
                return Err(FetchError::TooLarge);
            }
            let mut w = BlobWriter::begin(flash, slot, kind, seq).map_err(|_| FetchError::Storage)?;
            let leftover = filled - head_end;
            if leftover > 0 {
                w.write(&buf[head_end..filled]).map_err(|_| FetchError::Storage)?;
                total += leftover;
            }
            loop {
                if content_length.is_some_and(|l| total >= l) {
                    break;
                }
                let n = sock.read(&mut buf).await.map_err(|_| FetchError::Protocol)?;
                if n == 0 {
                    break;
                }
                if (total + n) as u32 > BLOB_DATA_MAX {
                    return Err(FetchError::TooLarge);
                }
                w.write(&buf[..n]).map_err(|_| FetchError::Storage)?;
                total += n;
            }
            if content_length.is_some_and(|l| l != total) {
                return Err(FetchError::Protocol);
            }
            let header = w.finish().map_err(|_| FetchError::Storage)?;
            result.len = header.len;
            result.crc32 = header.crc32;
        }
        Sink::Firmware => {
            let mut scratch = ota::scratch();
            let mut updater = ota::updater(flash, &mut scratch);
            let mut w = ota::ImageWriter::new(&mut updater);
            let leftover = filled - head_end;
            if leftover > 0 {
                w.write(&buf[head_end..filled]).map_err(|_| FetchError::Update)?;
                total += leftover;
            }
            loop {
                if content_length.is_some_and(|l| total >= l) {
                    break;
                }
                let n = sock.read(&mut buf).await.map_err(|_| FetchError::Protocol)?;
                if n == 0 {
                    break;
                }
                w.write(&buf[..n]).map_err(|_| FetchError::Update)?;
                total += n;
            }
            if content_length.is_some_and(|l| l != total) {
                return Err(FetchError::Protocol);
            }
            result.len = w.finish().map_err(|_| FetchError::Update)? as u32;
        }
        Sink::Frame => {
            const FRAME: usize = hoot_gfx::BYTES;
            if content_length.is_some_and(|l| l != FRAME) {
                return Err(FetchError::TooLarge);
            }
            let leftover = (filled - head_end).min(FRAME);
            super::LIVE_FRAME.lock(|c| c.borrow_mut()[..leftover].copy_from_slice(&buf[head_end..head_end + leftover]));
            total = leftover;
            while total < FRAME {
                let n = sock.read(&mut buf).await.map_err(|_| FetchError::Protocol)?;
                if n == 0 {
                    break;
                }
                let n = n.min(FRAME - total);
                super::LIVE_FRAME.lock(|c| c.borrow_mut()[total..total + n].copy_from_slice(&buf[..n]));
                total += n;
            }
            if total != FRAME {
                return Err(FetchError::Protocol);
            }
            result.len = total as u32;
        }
        Sink::Small => {
            let leftover = (filled - head_end).min(SMALL_BODY_MAX);
            small[..leftover].copy_from_slice(&buf[head_end..head_end + leftover]);
            total = leftover;
            while total < SMALL_BODY_MAX && content_length.is_none_or(|l| total < l) {
                let n = sock.read(&mut small[total..]).await.map_err(|_| FetchError::Protocol)?;
                if n == 0 {
                    break;
                }
                total += n;
            }
            result.len = total as u32;
        }
    }
    sock.close();
    Ok(result)
}

/// Decimal formatting without `core::fmt`.
fn itoa(mut v: u32, buf: &mut [u8; 6]) -> &str {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    buf[i - 1] = b':';
    core::str::from_utf8(&buf[i - 1..]).unwrap_or(":0")
}

//! HTTP/1.1 response head parsing for a tiny client.
//!
//! The client reads bytes until it sees `\r\n\r\n`, then hands the head to
//! [`parse_head`]. Bodies are streamed by the caller using `content_length`.

/// The parts of a response head that the OS cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Head<'a> {
    pub status: u16,
    pub content_length: Option<usize>,
    pub last_modified: Option<&'a str>,
    pub chunked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadError {
    NotUtf8,
    BadStatusLine,
}

/// Method and path of a request line such as `GET /save HTTP/1.1`.
pub fn parse_request_line(head: &[u8]) -> Option<(&str, &str)> {
    let end = head.iter().position(|&b| b == b'\r' || b == b'\n').unwrap_or(head.len());
    let line = core::str::from_utf8(&head[..end]).ok()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let path = parts.next()?;
    if method.is_empty() || !path.starts_with('/') {
        return None;
    }
    Some((method, path))
}

/// Value of the first header called `name` (case-insensitive) in a head.
pub fn header_value<'a>(head: &'a [u8], name: &str) -> Option<&'a str> {
    let text = core::str::from_utf8(head).ok()?;
    text.split("\r\n").skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Length of the head including the blank line, if the buffer holds a full one.
pub fn head_len(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Parse a complete response head (status line plus headers).
pub fn parse_head(head: &[u8]) -> Result<Head<'_>, HeadError> {
    let text = core::str::from_utf8(head).map_err(|_| HeadError::NotUtf8)?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(HeadError::BadStatusLine)?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(HeadError::BadStatusLine);
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or(HeadError::BadStatusLine)?;

    let mut out = Head { status, ..Head::default() };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            out.content_length = value.parse().ok();
        } else if name.eq_ignore_ascii_case("last-modified") {
            out.last_modified = Some(value);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            out.chunked = value.eq_ignore_ascii_case("chunked");
        }
    }
    Ok(out)
}

/// Write a GET request head into `buf`. Returns the number of bytes written,
/// or `None` if `buf` is too small.
pub fn write_get(
    buf: &mut [u8],
    host: &str,
    path: &str,
    if_modified_since: Option<&str>,
) -> Option<usize> {
    let mut w = Writer { buf, len: 0 };
    w.put(b"GET ")?;
    w.put(path.as_bytes())?;
    w.put(b" HTTP/1.1\r\nHost: ")?;
    w.put(host.as_bytes())?;
    w.put(b"\r\nUser-Agent: SprigOS\r\nConnection: close\r\n")?;
    if let Some(ims) = if_modified_since {
        w.put(b"If-Modified-Since: ")?;
        w.put(ims.as_bytes())?;
        w.put(b"\r\n")?;
    }
    w.put(b"\r\n")?;
    Some(w.len)
}

struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.len.checked_add(bytes.len())?;
        if end > self.buf.len() {
            return None;
        }
        self.buf[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-length: 40960\r\nLast-Modified: Sat, 13 Sep 2026 10:00:00 GMT\r\nContent-Type: application/octet-stream\r\n\r\nBODY";

    #[test]
    fn finds_end_of_head() {
        let n = head_len(HEAD).unwrap();
        assert_eq!(&HEAD[n..], b"BODY");
        assert_eq!(head_len(b"HTTP/1.1 200 OK\r\nX: y\r\n"), None);
    }

    #[test]
    fn parses_status_and_headers_case_insensitively() {
        let n = head_len(HEAD).unwrap();
        let h = parse_head(&HEAD[..n]).unwrap();
        assert_eq!(h.status, 200);
        assert_eq!(h.content_length, Some(40960));
        assert_eq!(h.last_modified, Some("Sat, 13 Sep 2026 10:00:00 GMT"));
        assert!(!h.chunked);
    }

    #[test]
    fn handles_304_without_body() {
        let h = parse_head(b"HTTP/1.1 304 Not Modified\r\n\r\n").unwrap();
        assert_eq!(h.status, 304);
        assert_eq!(h.content_length, None);
    }

    #[test]
    fn detects_chunked() {
        let h = parse_head(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
        assert!(h.chunked);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_head(b"hello\r\n\r\n").unwrap_err(), HeadError::BadStatusLine);
        assert_eq!(parse_head(b"HTTP/1.1 abc\r\n\r\n").unwrap_err(), HeadError::BadStatusLine);
    }

    #[test]
    fn parses_request_lines_and_headers() {
        let head = b"POST /save HTTP/1.1\r\nHost: 192.168.4.1\r\ncontent-length: 42\r\n\r\n";
        assert_eq!(parse_request_line(head), Some(("POST", "/save")));
        assert_eq!(header_value(head, "Content-Length"), Some("42"));
        assert_eq!(header_value(head, "host"), Some("192.168.4.1"));
        assert_eq!(header_value(head, "Cookie"), None);
        assert_eq!(parse_request_line(b"nonsense"), None);
        assert_eq!(parse_request_line(b"GET nopath HTTP/1.1"), None);
    }

    #[test]
    fn writes_a_get_request() {
        let mut buf = [0u8; 256];
        let n = write_get(&mut buf, "192.168.1.9:8000", "/frame/arpan.rgb565", Some("Sat, 13 Sep 2026 10:00:00 GMT")).unwrap();
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.starts_with("GET /frame/arpan.rgb565 HTTP/1.1\r\nHost: 192.168.1.9:8000\r\n"));
        assert!(s.contains("If-Modified-Since: Sat, 13 Sep 2026 10:00:00 GMT\r\n"));
        assert!(s.ends_with("\r\n\r\n"));
        assert_eq!(write_get(&mut [0u8; 16], "h", "/", None), None);
    }
}

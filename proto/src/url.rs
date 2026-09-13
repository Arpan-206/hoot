//! Minimal `http://host[:port]/path` parsing. No allocation, no percent decoding.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Url<'a> {
    pub host: &'a str,
    pub port: u16,
    /// Always starts with `/`.
    pub path: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlError {
    /// Only plain `http://` is supported.
    Scheme,
    EmptyHost,
    BadPort,
}

/// Parse a plain HTTP URL. `https://` is rejected: the OS has no TLS yet.
pub fn parse(url: &str) -> Result<Url<'_>, UrlError> {
    let rest = url.strip_prefix("http://").ok_or(UrlError::Scheme)?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| UrlError::BadPort)?),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(UrlError::EmptyHost);
    }
    Ok(Url { host, port, path })
}

/// Parse a dotted IPv4 literal such as `192.168.1.9`.
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut n = 0;
    for part in s.split('.') {
        if n == 4 || part.is_empty() || part.len() > 3 {
            return None;
        }
        out[n] = part.parse::<u8>().ok()?;
        n += 1;
    }
    (n == 4).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_port_path() {
        let u = parse("http://192.168.1.9:8000/frame/arpan.rgb565").unwrap();
        assert_eq!(u.host, "192.168.1.9");
        assert_eq!(u.port, 8000);
        assert_eq!(u.path, "/frame/arpan.rgb565");
    }

    #[test]
    fn defaults_port_and_path() {
        let u = parse("http://frames.example").unwrap();
        assert_eq!((u.host, u.port, u.path), ("frames.example", 80, "/"));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse("https://x/").unwrap_err(), UrlError::Scheme);
        assert_eq!(parse("http://:80/").unwrap_err(), UrlError::EmptyHost);
        assert_eq!(parse("http://x:abc/").unwrap_err(), UrlError::BadPort);
    }

    #[test]
    fn ipv4_literals() {
        assert_eq!(parse_ipv4("192.168.1.9"), Some([192, 168, 1, 9]));
        assert_eq!(parse_ipv4("frames.example"), None);
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        assert_eq!(parse_ipv4("256.1.1.1"), None);
    }
}

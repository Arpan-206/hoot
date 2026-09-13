//! `application/x-www-form-urlencoded` bodies and HTML escaping, without
//! allocation.

/// Call `f(key, value)` for every field. Values are percent-decoded into a
/// stack buffer; longer values are cut at 160 bytes. Keys are passed as is.
pub fn fields(body: &str, mut f: impl FnMut(&str, &str)) {
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, raw) = pair.split_once('=').unwrap_or((pair, ""));
        let mut buf = [0u8; 160];
        let n = percent_decode(raw, &mut buf);
        let value = core::str::from_utf8(&buf[..n]).unwrap_or("");
        f(key, value);
    }
}

/// Decode `+` and `%XX` into `out`. Returns the decoded length.
pub fn percent_decode(s: &str, out: &mut [u8]) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() && n < out.len() {
        let b = bytes[i];
        if b == b'+' {
            out[n] = b' ';
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out[n] = (h << 4) | l;
                    i += 3;
                }
                _ => {
                    out[n] = b;
                    i += 1;
                }
            }
        } else {
            out[n] = b;
            i += 1;
        }
        n += 1;
    }
    // Never end in the middle of a multi-byte character.
    while n > 0 && core::str::from_utf8(&out[..n]).is_err() {
        n -= 1;
    }
    n
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Copy `s` into `out` with `& < > " '` escaped for HTML. Returns the length
/// written, or `None` if `out` is too small.
pub fn html_escape(s: &str, out: &mut [u8]) -> Option<usize> {
    let mut n = 0;
    for &b in s.as_bytes() {
        let rep: &[u8] = match b {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            b'\'' => b"&#39;",
            _ => core::slice::from_ref(&b),
        };
        if n + rep.len() > out.len() {
            return None;
        }
        out[n..n + rep.len()].copy_from_slice(rep);
        n += rep.len();
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fields() {
        let mut got = Vec::new();
        fields("ssid=Home+Net&password=p%40ss%2Fw%C3%B6rd&empty=&flag", |k, v| {
            got.push((k.to_string(), v.to_string()));
        });
        assert_eq!(
            got,
            vec![
                ("ssid".into(), "Home Net".into()),
                ("password".into(), "p@ss/wörd".into()),
                ("empty".into(), "".into()),
                ("flag".into(), "".into()),
            ]
        );
    }

    #[test]
    fn bad_percent_sequences_pass_through() {
        let mut out = [0u8; 16];
        let n = percent_decode("a%zz%4", &mut out);
        assert_eq!(&out[..n], b"a%zz%4");
    }

    #[test]
    fn truncation_keeps_utf8_valid() {
        let mut out = [0u8; 3];
        let n = percent_decode("a%C3%B6%C3%B6", &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "aö");
    }

    #[test]
    fn escapes_html() {
        let mut out = [0u8; 64];
        let n = html_escape("<b>&'\"x", &mut out).unwrap();
        assert_eq!(&out[..n], b"&lt;b&gt;&amp;&#39;&quot;x");
        assert_eq!(html_escape("<<<<", &mut [0u8; 8]), None);
    }
}

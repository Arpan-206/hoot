//! The message list the server sends the Sprig: one message per line,
//! newest first, tab-separated `id`, `seen` (0 or 1), `age_secs`, `text`.
//! Tabs and newlines never appear inside `text`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    pub id: u32,
    pub seen: bool,
    pub age_secs: u32,
    pub text: &'a str,
}

/// Iterate over the well-formed lines of a list. Malformed lines are skipped.
pub fn parse(body: &str) -> impl Iterator<Item = Message<'_>> {
    body.lines().filter_map(|line| {
        let mut f = line.splitn(4, '\t');
        let id = f.next()?.trim().parse().ok()?;
        let seen = f.next()?.trim() == "1";
        let age_secs = f.next()?.trim().parse().ok()?;
        let text = f.next().unwrap_or("").trim_end_matches('\r');
        Some(Message { id, seen, age_secs, text })
    })
}

/// "now", "5m", "3h", "2d": how long ago, in one short token.
pub fn age_label(secs: u32, out: &mut [u8; 4]) -> &str {
    let (n, unit) = if secs < 60 {
        return "now";
    } else if secs < 3600 {
        (secs / 60, b'm')
    } else if secs < 86_400 {
        (secs / 3600, b'h')
    } else {
        ((secs / 86_400).min(999), b'd')
    };
    let mut i = 0;
    if n >= 100 {
        out[i] = b'0' + (n / 100) as u8;
        i += 1;
    }
    if n >= 10 {
        out[i] = b'0' + ((n / 10) % 10) as u8;
        i += 1;
    }
    out[i] = b'0' + (n % 10) as u8;
    i += 1;
    out[i] = unit;
    core::str::from_utf8(&out[..i + 1]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_newest_first() {
        let body = "1712\t0\t42\tHii there\n1700\t1\t86500\told one\ngarbage\n\n3\t0\t7\t";
        let got: Vec<_> = parse(body).collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], Message { id: 1712, seen: false, age_secs: 42, text: "Hii there" });
        assert_eq!(got[1].seen, true);
        assert_eq!(got[2].text, "");
    }

    #[test]
    fn age_labels() {
        let mut b = [0u8; 4];
        assert_eq!(age_label(5, &mut b), "now");
        assert_eq!(age_label(300, &mut b), "5m");
        assert_eq!(age_label(3600 * 3 + 5, &mut b), "3h");
        assert_eq!(age_label(86_400 * 12, &mut b), "12d");
        assert_eq!(age_label(86_400 * 5000, &mut b), "999d");
    }
}

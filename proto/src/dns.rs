//! A captive-portal DNS responder: every A query gets the same address.

/// Copy the question name of `query` into `out` as dotted text. Returns the
/// length written, or `None` if the query is malformed or `out` too small.
pub fn question_name(query: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut i = 12;
    let mut n = 0;
    loop {
        let len = *query.get(i)? as usize;
        if len == 0 {
            return Some(n);
        }
        if len & 0xC0 != 0 {
            return None;
        }
        let label = query.get(i + 1..i + 1 + len)?;
        if n > 0 {
            *out.get_mut(n)? = b'.';
            n += 1;
        }
        out.get_mut(n..n + len)?.copy_from_slice(label);
        n += len;
        i += 1 + len;
    }
}

/// Build a reply to `query` in `out`. A queries get `ip`; other types get
/// an empty answer. Returns the length, or `None` if the query is malformed
/// or `out` is too small.
pub fn build_reply(query: &[u8], ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
    if query.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount != 1 || query[2] & 0x80 != 0 {
        return None; // not exactly one question, or already a response
    }
    // Walk the question name.
    let mut i = 12;
    loop {
        let len = *query.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len & 0xC0 != 0 {
            return None; // compression pointers do not appear in questions
        }
        i += 1 + len;
    }
    let qtype = u16::from_be_bytes([*query.get(i)?, *query.get(i + 1)?]);
    let question_end = i + 4;
    if query.len() < question_end {
        return None;
    }
    let is_a = qtype == 1;
    let answer_len = if is_a { 16 } else { 0 };
    let total = question_end + answer_len;
    if out.len() < total {
        return None;
    }
    out[..question_end].copy_from_slice(&query[..question_end]);
    out[2] = 0x81; // response, recursion desired copied
    out[3] = 0x80; // recursion available, no error
    out[6..8].copy_from_slice(&(if is_a { 1u16 } else { 0 }).to_be_bytes()); // ancount
    out[8..12].fill(0); // nscount, arcount
    if is_a {
        let a = &mut out[question_end..total];
        a[0..2].copy_from_slice(&[0xC0, 0x0C]); // pointer to the question name
        a[2..4].copy_from_slice(&1u16.to_be_bytes()); // type A
        a[4..6].copy_from_slice(&1u16.to_be_bytes()); // class IN
        a[6..10].copy_from_slice(&60u32.to_be_bytes()); // TTL
        a[10..12].copy_from_slice(&4u16.to_be_bytes());
        a[12..16].copy_from_slice(&ip);
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &[&str], qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    #[test]
    fn answers_a_queries_with_the_portal_address() {
        let q = query(&["captive", "apple", "com"], 1);
        let mut out = [0u8; 512];
        let n = build_reply(&q, [192, 168, 4, 1], &mut out).unwrap();
        assert_eq!(n, q.len() + 16);
        assert_eq!(&out[..2], &[0x12, 0x34]);
        assert_eq!(&out[2..4], &[0x81, 0x80]);
        assert_eq!(&out[6..8], &[0, 1]);
        assert_eq!(&out[n - 4..n], &[192, 168, 4, 1]);
        assert_eq!(&out[n - 16..n - 14], &[0xC0, 0x0C]);
    }

    #[test]
    fn extracts_the_question_name() {
        let q = query(&["captive", "apple", "com"], 1);
        let mut out = [0u8; 64];
        let n = question_name(&q, &mut out).unwrap();
        assert_eq!(&out[..n], b"captive.apple.com");
        assert_eq!(question_name(&q, &mut [0u8; 4]), None);
    }

    #[test]
    fn other_types_get_an_empty_answer() {
        let q = query(&["example", "org"], 28); // AAAA
        let mut out = [0u8; 512];
        let n = build_reply(&q, [192, 168, 4, 1], &mut out).unwrap();
        assert_eq!(n, q.len());
        assert_eq!(&out[6..8], &[0, 0]);
    }

    #[test]
    fn rejects_malformed() {
        let mut out = [0u8; 512];
        assert_eq!(build_reply(&[0u8; 5], [0; 4], &mut out), None);
        let mut q = query(&["a"], 1);
        q[2] |= 0x80; // a response, not a query
        assert_eq!(build_reply(&q, [0; 4], &mut out), None);
        let q = query(&["a"], 1);
        assert_eq!(build_reply(&q, [0; 4], &mut [0u8; 8]), None);
    }
}

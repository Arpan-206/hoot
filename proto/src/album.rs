//! The album list from the server: one photo id per line, newest first.
//! Ids are the upload time in seconds, so they also order the photos.

/// Fill `out` with the ids in order. Returns how many were read.
pub fn parse_ids(body: &str, out: &mut [u32]) -> usize {
    let mut n = 0;
    for line in body.lines() {
        if n == out.len() {
            break;
        }
        if let Ok(id) = line.trim().parse::<u32>() {
            out[n] = id;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ids_and_skips_junk() {
        let mut ids = [0u32; 4];
        let n = parse_ids("1789360000\r\n1789350000\n\nnot a number\n5\n", &mut ids);
        assert_eq!(n, 3);
        assert_eq!(&ids[..3], &[1_789_360_000, 1_789_350_000, 5]);
    }

    #[test]
    fn stops_at_capacity() {
        let mut ids = [0u32; 2];
        assert_eq!(parse_ids("1\n2\n3\n", &mut ids), 2);
        assert_eq!(ids, [1, 2]);
    }
}

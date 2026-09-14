//! Civil time from a count of seconds since 1970. No zones here: the
//! caller adds the offset first. Day numbers use Howard Hinnant's
//! "chrono-compatible low-level date algorithms".

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// 0 = Monday to 6 = Sunday.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl Civil {
    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS[self.weekday as usize % 7]
    }

    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month.clamp(1, 12) - 1) as usize]
    }

    /// Minutes since midnight.
    pub fn minute_of_day(&self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }
}

/// Whole days since 1970-01-01, negative before it.
pub fn days_from_secs(secs: i64) -> i64 {
    secs.div_euclid(86_400)
}

pub fn civil_from_secs(secs: i64) -> Civil {
    let days = days_from_secs(secs);
    let sod = secs.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 was a Thursday: 3 with Monday as 0.
    let weekday = (days + 3).rem_euclid(7) as u8;
    Civil {
        year,
        month,
        day,
        weekday,
        hour: (sod / 3600) as u8,
        minute: (sod / 60 % 60) as u8,
        second: (sod % 60) as u8,
    }
}

/// Year, month and day for a day number since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_a_thursday() {
        let c = civil_from_secs(0);
        assert_eq!((c.year, c.month, c.day), (1970, 1, 1));
        assert_eq!(c.weekday_name(), "Thu");
        assert_eq!((c.hour, c.minute, c.second), (0, 0, 0));
    }

    #[test]
    fn leap_day_2000() {
        let c = civil_from_secs(951_782_400);
        assert_eq!((c.year, c.month, c.day), (2000, 2, 29));
        assert_eq!(c.weekday_name(), "Tue");
    }

    #[test]
    fn a_known_afternoon() {
        let c = civil_from_secs(1_600_000_000);
        assert_eq!((c.year, c.month, c.day), (2020, 9, 13));
        assert_eq!((c.hour, c.minute, c.second), (12, 26, 40));
        assert_eq!(c.weekday_name(), "Sun");
        assert_eq!(c.month_name(), "Sep");
        assert_eq!(c.minute_of_day(), 12 * 60 + 26);
    }

    #[test]
    fn september_2026() {
        let c = civil_from_secs(1_789_344_000 + 10 * 3600 + 47 * 60 + 5);
        assert_eq!((c.year, c.month, c.day), (2026, 9, 14));
        assert_eq!((c.hour, c.minute, c.second), (10, 47, 5));
        assert_eq!(c.weekday_name(), "Mon");
    }

    #[test]
    fn before_the_epoch() {
        let c = civil_from_secs(-1);
        assert_eq!((c.year, c.month, c.day), (1969, 12, 31));
        assert_eq!((c.hour, c.minute, c.second), (23, 59, 59));
        assert_eq!(days_from_secs(-1), -1);
    }
}

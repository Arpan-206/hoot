//! The forecast the server prepares for a frame: a few tab-separated
//! lines, small enough for the device's 1 KiB body buffer.
//!
//! ```text
//! Manchester
//! 14    3    18    72    120        current: temp C, WMO code, wind km/h, humidity %, age s
//! 1    16    9    61             a day: weekday (0 = Mon), max, min, code; up to three
//! ```

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Day {
    pub weekday: u8,
    pub max: i16,
    pub min: i16,
    pub code: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forecast<'a> {
    pub place: &'a str,
    pub temp: i16,
    pub code: u8,
    pub wind: u8,
    pub humidity: u8,
    pub age_secs: u32,
    pub days: [Day; 3],
    pub day_count: usize,
}

pub fn parse(body: &str) -> Option<Forecast<'_>> {
    let mut lines = body.lines().map(|l| l.trim_end_matches('\r'));
    let place = lines.next()?.trim();
    let mut f = lines.next()?.split('\t');
    let temp = f.next()?.trim().parse().ok()?;
    let code = f.next()?.trim().parse().ok()?;
    let wind = f.next()?.trim().parse::<u32>().ok()?.min(255) as u8;
    let humidity = f.next()?.trim().parse::<u32>().ok()?.min(100) as u8;
    let age_secs = f.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let mut days = [Day::default(); 3];
    let mut day_count = 0;
    for line in lines.take(3) {
        let mut f = line.split('\t');
        let day = (|| {
            Some(Day {
                weekday: f.next()?.trim().parse::<u8>().ok()? % 7,
                max: f.next()?.trim().parse().ok()?,
                min: f.next()?.trim().parse().ok()?,
                code: f.next()?.trim().parse().ok()?,
            })
        })();
        if let Some(d) = day {
            days[day_count] = d;
            day_count += 1;
        }
    }
    Some(Forecast { place, temp, code, wind, humidity, age_secs, days, day_count })
}

/// The sky, from a WMO weather code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Showers,
    Thunder,
}

pub const fn sky(code: u8) -> Sky {
    match code {
        0 => Sky::Clear,
        1 | 2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51..=57 => Sky::Drizzle,
        61..=67 => Sky::Rain,
        71..=77 | 85 | 86 => Sky::Snow,
        80..=82 => Sky::Showers,
        95..=99 => Sky::Thunder,
        _ => Sky::Cloudy,
    }
}

impl Sky {
    pub const fn label(self) -> &'static str {
        match self {
            Sky::Clear => "clear",
            Sky::PartlyCloudy => "partly cloudy",
            Sky::Cloudy => "overcast",
            Sky::Fog => "fog",
            Sky::Drizzle => "drizzle",
            Sky::Rain => "rain",
            Sky::Snow => "snow",
            Sky::Showers => "showers",
            Sky::Thunder => "thunder",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_forecast() {
        let f = parse("Manchester\n14\t3\t18\t72\t120\n1\t16\t9\t61\n2\t-2\t-7\t71\n").unwrap();
        assert_eq!(f.place, "Manchester");
        assert_eq!((f.temp, f.code, f.wind, f.humidity, f.age_secs), (14, 3, 18, 72, 120));
        assert_eq!(f.day_count, 2);
        assert_eq!(f.days[1], Day { weekday: 2, max: -2, min: -7, code: 71 });
    }

    #[test]
    fn tolerates_a_short_body() {
        let f = parse("Leeds\r\n9\t0\t5\t40\r\n").unwrap();
        assert_eq!(f.day_count, 0);
        assert_eq!(f.age_secs, 0);
        assert!(parse("Leeds\nnot numbers\n").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn codes_map_to_skies() {
        assert_eq!(sky(0), Sky::Clear);
        assert_eq!(sky(2), Sky::PartlyCloudy);
        assert_eq!(sky(48), Sky::Fog);
        assert_eq!(sky(63), Sky::Rain);
        assert_eq!(sky(75), Sky::Snow);
        assert_eq!(sky(81), Sky::Showers);
        assert_eq!(sky(96), Sky::Thunder);
        assert_eq!(sky(200), Sky::Cloudy);
        assert!(Sky::PartlyCloudy.label().len() <= 13);
    }
}

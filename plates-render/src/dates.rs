//! Turning the dates a vault writes into the dates a feed reader will accept.
//!
//! A vault's frontmatter carries dates as whatever the author typed —
//! `2026-08-16`, `2026-08-16T09:30:00Z`, a YAML timestamp with a space in it.
//! That is right for a document: the grain a person wrote in is information,
//! and prov keeps it. It is wrong for syndication, where the formats are
//! specified and checked: Atom requires RFC 3339 and RSS 2.0 requires RFC 822
//! dates, and feeds carrying a bare `2026-08-16` are rejected by validators and
//! misparsed by readers.
//!
//! So this module reads the loose spelling once and writes the strict one, for
//! each of the two grammars that need it. A date with no time of day is read as
//! midnight UTC — the only reading available, and the one every static site
//! generator makes.
//!
//! Hand-rolled rather than `chrono`, because this crate must stay portable to
//! `wasm32-unknown-unknown` and free of a clock: nothing here asks what time it
//! is, it only re-spells a time it was given.

/// A moment, as precisely as the vault happened to write one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    /// Minutes east of UTC. Zero for a date written without an offset, which is
    /// also how a date written without a time of day is read.
    offset_minutes: i32,
}

/// The instant a required-but-missing feed date falls back to.
///
/// Atom makes `<updated>` mandatory on the feed and on every entry, so an entry
/// whose vault date is absent or unreadable still needs *something* valid. The
/// epoch says "no date known" in a way a reader sorts to the bottom, where
/// inventing a plausible one would quietly reorder somebody's archive.
pub const EPOCH_RFC3339: &str = "1970-01-01T00:00:00Z";

/// Read one of the date spellings a vault may hold.
///
/// Accepted: `YYYY-MM-DD`, optionally followed by `T` or a space and
/// `HH:MM[:SS]`, optionally followed by `Z` or `±HH[:]MM`. Trailing fractional
/// seconds are read and dropped — neither output grammar carries them.
///
/// `None` for anything else, including a well-formed string naming a day that
/// does not exist (`2026-02-30`): a date that cannot be placed on a calendar
/// cannot be given a weekday, and RFC 822 needs one.
pub fn parse(raw: &str) -> Option<Timestamp> {
    let s = raw.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }

    let year: i64 = digits(&s[0..4])? as i64;
    if bytes[4] != b'-' {
        return None;
    }
    let month = digits(&s[5..7])?;
    if bytes[7] != b'-' {
        return None;
    }
    let day = digits(&s[8..10])?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }

    let mut ts = Timestamp {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        offset_minutes: 0,
    };

    let rest = &s[10..];
    if rest.is_empty() {
        return Some(ts);
    }

    // A time of day, introduced by `T` (RFC 3339) or a space (YAML).
    let sep = rest.as_bytes()[0];
    if !matches!(sep, b'T' | b't' | b' ') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if rest.len() < 5 {
        return None;
    }
    ts.hour = digits(&rest[0..2])?;
    if rest.as_bytes()[2] != b':' {
        return None;
    }
    ts.minute = digits(&rest[3..5])?;
    let mut rest = &rest[5..];
    if rest.starts_with(':') {
        if rest.len() < 3 {
            return None;
        }
        ts.second = digits(&rest[1..3])?;
        rest = &rest[3..];
    }
    // Fractional seconds, which neither output grammar carries.
    if rest.starts_with('.') {
        let end = rest[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(rest.len(), |i| i + 1);
        rest = &rest[end..];
    }
    if ts.hour > 23 || ts.minute > 59 || ts.second > 60 {
        return None;
    }

    ts.offset_minutes = parse_offset(rest.trim())?;
    Some(ts)
}

/// Minutes east of UTC from a trailing `Z`, `±HH:MM`, `±HHMM` or nothing.
fn parse_offset(s: &str) -> Option<i32> {
    if s.is_empty() {
        return Some(0);
    }
    if s.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = match s.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = &s[1..];
    let (hh, mm) = match body.len() {
        5 if body.as_bytes()[2] == b':' => (&body[0..2], &body[3..5]),
        4 => (&body[0..2], &body[2..4]),
        2 => (&body[0..2], "00"),
        _ => return None,
    };
    let hours = digits(hh)? as i32;
    let minutes = digits(mm)? as i32;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

/// Parse a fixed-width run of ASCII digits. Rejects signs and spaces, which
/// `str::parse` would otherwise accept in the middle of a date.
fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

impl Timestamp {
    /// Day of the week, by Sakamoto's method — 0 is Sunday.
    ///
    /// RFC 822 puts the weekday in the date it specifies, so a feed cannot be
    /// written without deriving one.
    fn weekday(&self) -> usize {
        const OFFSETS: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let mut y = self.year;
        if self.month < 3 {
            y -= 1;
        }
        let idx =
            y + y / 4 - y / 100 + y / 400 + OFFSETS[(self.month - 1) as usize] + self.day as i64;
        idx.rem_euclid(7) as usize
    }

    /// RFC 3339, as Atom requires: `2026-08-16T09:30:00Z`.
    pub fn to_rfc3339(&self) -> String {
        let zone = if self.offset_minutes == 0 {
            "Z".to_string()
        } else {
            let (sign, abs) = signed(self.offset_minutes);
            format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
        };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{zone}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// RFC 822 with a four-digit year, as RSS 2.0 requires:
    /// `Sun, 16 Aug 2026 09:30:00 +0000`.
    pub fn to_rfc822(&self) -> String {
        let (sign, abs) = signed(self.offset_minutes);
        format!(
            "{}, {:02} {} {:04} {:02}:{:02}:{:02} {sign}{:02}{:02}",
            WEEKDAYS[self.weekday()],
            self.day,
            MONTHS[(self.month - 1) as usize],
            self.year,
            self.hour,
            self.minute,
            self.second,
            abs / 60,
            abs % 60,
        )
    }
}

fn signed(offset_minutes: i32) -> (char, i32) {
    if offset_minutes < 0 {
        ('-', -offset_minutes)
    } else {
        ('+', offset_minutes)
    }
}

/// Re-spell a vault date as RFC 3339, or `None` if it cannot be read.
pub fn to_rfc3339(raw: &str) -> Option<String> {
    parse(raw).map(|ts| ts.to_rfc3339())
}

/// Re-spell a vault date as RFC 822, or `None` if it cannot be read.
pub fn to_rfc822(raw: &str) -> Option<String> {
    parse(raw).map(|ts| ts.to_rfc822())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spelling almost every vault actually holds — a day, no time. It is
    /// what a feed validator rejects, and the reason this module exists.
    #[test]
    fn a_bare_day_becomes_midnight_utc() {
        assert_eq!(to_rfc3339("2026-08-16").unwrap(), "2026-08-16T00:00:00Z");
        assert_eq!(
            to_rfc822("2026-08-16").unwrap(),
            "Sun, 16 Aug 2026 00:00:00 +0000"
        );
    }

    #[test]
    fn a_full_rfc3339_instant_survives_the_round_trip() {
        assert_eq!(
            to_rfc3339("2026-08-16T09:30:15Z").unwrap(),
            "2026-08-16T09:30:15Z"
        );
        assert_eq!(
            to_rfc822("2026-08-16T09:30:15Z").unwrap(),
            "Sun, 16 Aug 2026 09:30:15 +0000"
        );
    }

    /// YAML writes a timestamp with a space where RFC 3339 writes a `T`, and a
    /// vault's metadata is YAML by default.
    #[test]
    fn a_yaml_timestamp_is_read_too() {
        assert_eq!(
            to_rfc3339("2026-08-16 09:30:15").unwrap(),
            "2026-08-16T09:30:15Z"
        );
    }

    #[test]
    fn an_offset_is_kept_in_both_grammars() {
        assert_eq!(
            to_rfc3339("2026-08-16T09:30:00+02:00").unwrap(),
            "2026-08-16T09:30:00+02:00"
        );
        assert_eq!(
            to_rfc822("2026-08-16T09:30:00+02:00").unwrap(),
            "Sun, 16 Aug 2026 09:30:00 +0200"
        );
        assert_eq!(
            to_rfc822("2026-08-16T09:30:00-0530").unwrap(),
            "Sun, 16 Aug 2026 09:30:00 -0530"
        );
    }

    #[test]
    fn seconds_and_fractions_are_optional() {
        assert_eq!(
            to_rfc3339("2026-08-16T09:30Z").unwrap(),
            "2026-08-16T09:30:00Z"
        );
        assert_eq!(
            to_rfc3339("2026-08-16T09:30:15.250Z").unwrap(),
            "2026-08-16T09:30:15Z"
        );
    }

    /// Weekdays are derived, so they are worth checking against dates whose
    /// answer is known — including a leap day, which is where an arithmetic
    /// slip shows up first.
    #[test]
    fn weekdays_are_derived_correctly() {
        assert!(to_rfc822("2024-02-29").unwrap().starts_with("Thu,"));
        assert!(to_rfc822("2000-01-01").unwrap().starts_with("Sat,"));
        assert!(to_rfc822("1970-01-01").unwrap().starts_with("Thu,"));
        assert!(to_rfc822("2026-01-15").unwrap().starts_with("Thu,"));
        assert!(to_rfc822("2100-03-01").unwrap().starts_with("Mon,"));
    }

    /// A day that is not on the calendar has no weekday, so it is refused
    /// rather than published as a date a reader would misplace.
    #[test]
    fn impossible_and_malformed_dates_are_refused() {
        for bad in [
            "",
            "not a date",
            "2026-13-01",
            "2026-02-30",
            "2023-02-29",
            "2026-00-10",
            "2026-08-00",
            "2026-8-16",
            "2026/08/16",
            "20260816",
            "2026-08-16X09:30:00Z",
            "2026-08-16T25:00:00Z",
            "2026-08-16T09:70:00Z",
            "2026-08-16T09:30:00+99:00",
        ] {
            assert!(parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    /// A leap day exists in a leap year and not in a common one.
    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(parse("2024-02-29").is_some());
        assert!(parse("2000-02-29").is_some());
        assert!(parse("1900-02-29").is_none());
        assert!(parse("2026-02-29").is_none());
    }
}

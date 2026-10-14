//! When a photo or home video was taken (docs/architecture.md section 4, Capture time).
//!
//! A capture time is the camera's clock where the picture was taken. Every viewer sees
//! that clock reading, in every time zone, the way photo apps show it, and the photo
//! timeline and the `YYYY/MM/` folders use its month. Files say when they were taken in
//! three ways, and each keeps what it knows:
//!
//! - a clock reading with its UTC offset (EXIF `OffsetTimeOriginal`, a QuickTime date
//!   with a zone): the clock and the exact moment are both known;
//! - a clock reading with nothing to say where (EXIF without an offset, a date in the
//!   file name): the clock is all there is, so it is kept as it is and never taken for
//!   UTC; the moment is that clock read in the server's time zone;
//! - a moment with no clock reading (the file's modification time, a container creation
//!   time written in UTC, a unix time in a file name): the clock is that moment read in
//!   the server's time zone.
//!
//! The server's time zone is the `TZ` environment variable (else `/etc/localtime`, else
//! UTC), read through the IANA time zone database of the system, or the copy built into
//! the binary when the system has none.

use std::sync::Once;

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{Offset, TimeZone};

/// Years a capture time may fall in: anything else is a camera with no clock set.
const YEARS: std::ops::RangeInclusive<i16> = 1800..=2200;

/// 1904-01-01T00:00:00Z, the zero point of QuickTime timestamps.
const QUICKTIME_EPOCH: i64 = -2_082_844_800;

/// Offsets beyond this many minutes from UTC do not exist on Earth (the widest is +14:00).
const MAX_OFFSET_MINUTES: i32 = 18 * 60;

/// What a file says about when it was taken, before the server's time zone is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// The camera's clock, with the UTC offset (minutes east) written beside it when the
    /// file has one.
    Clock {
        local: DateTime,
        offset: Option<i32>,
    },
    /// A moment (unix seconds) with no clock reading.
    Instant(i64),
}

impl Recorded {
    /// A clock reading from calendar fields. None for impossible dates and times, years
    /// outside 1800 to 2200, an offset no place uses, and the zeroed placeholders cameras
    /// write when the clock was never set (1970-01-01 and 1904-01-01 at midnight). A
    /// leap second (60) counts as 59.
    pub fn clock(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        offset: Option<i32>,
    ) -> Option<Self> {
        let year = i16::try_from(year).ok().filter(|y| YEARS.contains(y))?;
        let local = DateTime::new(
            year,
            i8::try_from(month).ok()?,
            i8::try_from(day).ok()?,
            i8::try_from(hour).ok()?,
            i8::try_from(minute).ok()?,
            i8::try_from(second.min(59)).ok()?,
            0,
        )
        .ok()?;
        if offset.is_some_and(|o| o.abs() > MAX_OFFSET_MINUTES) {
            return None;
        }
        let placeholder = |y: i16| DateTime::new(y, 1, 1, 0, 0, 0, 0).ok() == Some(local);
        if placeholder(1970) || placeholder(1904) {
            return None;
        }
        let recorded = Self::Clock { local, offset };
        if let Some(o) = offset {
            let at = at_offset(local, o)?;
            if at == 0 || at == QUICKTIME_EPOCH {
                return None;
            }
        }
        Some(recorded)
    }

    /// A moment written as a UTC clock (a container's creation time), with the same
    /// checks as [`Recorded::clock`].
    pub fn utc(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
    ) -> Option<Self> {
        Self::clock(year, month, day, hour, minute, second, None)?.in_utc()
    }

    /// This clock reading taken as UTC, for sources known to write UTC (a Pixel file
    /// name): the moment it names. A moment stays as it is.
    pub fn in_utc(self) -> Option<Self> {
        match self {
            Self::Clock { local, .. } => Self::instant(at_offset(local, 0)?),
            instant => Some(instant),
        }
    }

    /// A moment in unix seconds. None for the unix and QuickTime zero points, which mean
    /// "clock never set".
    pub fn instant(at: i64) -> Option<Self> {
        (at != 0 && at != QUICKTIME_EPOCH).then_some(Self::Instant(at))
    }

    /// The capture time, with `zone` (the server's time zone) filling in what the file
    /// left out. None when the moment falls outside the years a capture time may have.
    pub fn resolve(&self, zone: &TimeZone) -> Option<CaptureTime> {
        let taken = match *self {
            Self::Clock {
                local,
                offset: Some(offset),
            } => CaptureTime {
                at: at_offset(local, offset)?,
                local,
                offset: Some(offset),
            },
            Self::Clock {
                local,
                offset: None,
            } => CaptureTime {
                // A clock time the zone skips (the hour lost to daylight saving) counts
                // as the moment after the gap; one it repeats, as the first.
                at: local.to_zoned(zone.clone()).ok()?.timestamp().as_second(),
                local,
                offset: None,
            },
            Self::Instant(at) => CaptureTime {
                at,
                local: zone.to_datetime(Timestamp::from_second(at).ok()?),
                offset: None,
            },
        };
        YEARS.contains(&taken.local.year()).then_some(taken)
    }
}

fn at_offset(local: DateTime, offset_minutes: i32) -> Option<i64> {
    let offset = Offset::from_seconds(offset_minutes.checked_mul(60)?).ok()?;
    Some(
        local
            .to_zoned(TimeZone::fixed(offset))
            .ok()?
            .timestamp()
            .as_second(),
    )
}

/// When a photo or home video was taken (`items.taken_at`, `taken_local`, `taken_offset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureTime {
    /// The moment, unix seconds: exact when `offset` is known or the file gave a moment;
    /// otherwise the camera's clock read in the server's time zone.
    pub at: i64,
    /// The camera's clock where it was taken. What every viewer sees.
    pub local: DateTime,
    /// Minutes east of UTC, when the file wrote the offset beside the clock.
    pub offset: Option<i32>,
}

impl CaptureTime {
    /// `2024-06-13T12:21:00`: the stored and API form of the clock reading.
    pub fn local_text(&self) -> String {
        self.local.strftime("%Y-%m-%dT%H:%M:%S").to_string()
    }

    /// `2024-06`: the month the photo timeline groups it under.
    pub fn month(&self) -> String {
        format!("{:04}-{:02}", self.local.year(), self.local.month())
    }

    /// `(2024, 6)`: the `YYYY/MM/` folder a sorted file goes to.
    pub fn year_month(&self) -> (i16, i8) {
        (self.local.year(), self.local.month())
    }

    /// From the stored columns. A row or stored suggestion without a clock reading but
    /// with a moment (written before migration 0003) is read the way that migration
    /// reads it: the moment in UTC, which is how the old code stored a clock without an
    /// offset.
    pub fn from_stored(at: Option<i64>, local: Option<&str>, offset: Option<i32>) -> Option<Self> {
        let at = at?;
        let local = match local {
            Some(text) => text.parse::<DateTime>().ok()?,
            None => TimeZone::UTC.to_datetime(Timestamp::from_second(at).ok()?),
        };
        Some(Self { at, local, offset })
    }

    /// The three stored columns, in order: `taken_at`, `taken_local`, `taken_offset`.
    pub fn columns(taken: Option<&Self>) -> (Option<i64>, Option<String>, Option<i32>) {
        match taken {
            Some(t) => (Some(t.at), Some(t.local_text()), t.offset),
            None => (None, None, None),
        }
    }
}

/// Seconds a camera-style name's clock and a clip's creation time may be off a whole
/// offset: the phone names the file a moment before the recorder starts or stops.
const NAME_SLACK_S: i64 = 10;

/// The widest offset a name and a creation time may be apart by (+14:00 is the widest
/// on Earth, -12:00 the narrowest).
const NAME_OFFSET_MAX_S: i64 = 14 * 3600;

/// A clip's tags as a camera-style name dates them (section 4, Capture time). Android and
/// Samsung phones write a clip's container `creation_time` as a moment in UTC and name the
/// file after the phone's clock (`VID_20240630_223000`, `20240630_223000`), so the
/// difference between the two is the offset where it was filmed. The moment is when the
/// recording started or, as Android's recorder writes it, when it stopped (the clip's
/// length later). When exactly one whole quarter hour within 14 hours of UTC makes the
/// two agree, give or take ten seconds, the clip has the name's clock with that offset,
/// so the clock and the exact moment are both known. Anything else stays as it is.
pub fn with_name_clock(
    recorded: Option<Recorded>,
    name_clock: Option<DateTime>,
    duration_s: Option<f64>,
) -> Option<Recorded> {
    match (recorded, name_clock) {
        (Some(Recorded::Instant(moment)), Some(clock)) => {
            clock_from_name(clock, moment, duration_s).or(recorded)
        }
        _ => recorded,
    }
}

fn clock_from_name(clock: DateTime, moment: i64, duration_s: Option<f64>) -> Option<Recorded> {
    let apart = at_offset(clock, 0)?.checked_sub(moment)?;
    let length = duration_s
        .filter(|d| d.is_finite() && *d >= 1.0 && *d < 86_400.0)
        // Whole seconds of a clip under a day long.
        .map(|d| d.round() as i64);
    let mut offsets: Vec<i64> = [Some(0), length]
        .into_iter()
        .flatten()
        .filter_map(|lag| {
            let seconds = apart + lag;
            let offset = (seconds + 450).div_euclid(900) * 900;
            ((seconds - offset).abs() <= NAME_SLACK_S && offset.abs() <= NAME_OFFSET_MAX_S)
                .then_some(offset)
        })
        .collect();
    offsets.dedup();
    match offsets.as_slice() {
        [offset] => Some(Recorded::Clock {
            local: clock,
            offset: Some(i32::try_from(offset / 60).ok()?),
        }),
        _ => None,
    }
}

/// The first source that gives a capture time, in order of trust.
pub fn first_of<I>(sources: I, zone: &TimeZone) -> Option<CaptureTime>
where
    I: IntoIterator<Item = Option<Recorded>>,
{
    sources.into_iter().find_map(|source| source?.resolve(zone))
}

/// The server's time zone: `TZ`, else `/etc/localtime`, else UTC. A `TZ` that names no
/// zone is warned about once; no setting at all is simply UTC.
pub fn server_zone() -> TimeZone {
    match TimeZone::try_system() {
        Ok(zone) => zone,
        Err(err) => {
            if std::env::var_os("TZ").is_some_and(|tz| !tz.is_empty()) {
                static WARNED: Once = Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(error = %err, "TZ names no time zone this server knows; reading file times in UTC");
                });
            }
            TimeZone::UTC
        }
    }
}

/// The server's time zone for the log: its IANA name, else its offset now.
pub fn describe(zone: &TimeZone) -> String {
    match zone.iana_name() {
        Some(name) => name.to_string(),
        None => zone.to_offset(Timestamp::now()).to_string(),
    }
}

/// A capture time written in a QuickTime, MP4 or Matroska tag: `2024-06-13T12:21:00+0200`
/// or `+02:00` or `-05` (a clock with its offset), `2024-06-13T10:21:00.000000Z` (a
/// moment in UTC, as containers store their creation time), `2024-06-13 10:21:00`,
/// `2024:06:13 10:21:00` or `2024-06-13` (a clock with nothing to say where).
pub fn parse_tag_time(value: &str) -> Option<Recorded> {
    let s = value.trim();
    let num = |from: usize, to: usize| s.get(from..to).and_then(|t| t.parse::<i64>().ok());
    let sep = s.as_bytes().get(4).copied()?;
    if !matches!(sep, b'-' | b':') || s.as_bytes().get(7) != Some(&sep) {
        return None;
    }
    if !s.get(..10)?.bytes().all(|b| b.is_ascii_digit() || b == sep) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (mut hour, mut minute, mut second) = (0, 0, 0);
    let mut rest = s.get(10..)?;
    if let Some(clock) = rest.strip_prefix(['T', ' ']) {
        let b = clock.as_bytes();
        if b.len() < 5 || b[2] != b':' {
            return None;
        }
        let digits = |t: &str| t.bytes().all(|c| c.is_ascii_digit()).then_some(());
        digits(clock.get(0..2)?)?;
        digits(clock.get(3..5)?)?;
        hour = clock.get(0..2)?.parse().ok()?;
        minute = clock.get(3..5)?.parse().ok()?;
        rest = clock.get(5..)?;
        if let Some(sec) = rest.strip_prefix(':') {
            digits(sec.get(0..2)?)?;
            second = sec.get(0..2)?.parse().ok()?;
            rest = sec.get(2..)?;
        }
        if let Some(frac) = rest.strip_prefix(['.', ',']) {
            rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    match parse_zone(rest.trim())? {
        Zone::None => Recorded::clock(year, month, day, hour, minute, second, None),
        Zone::Offset(offset) => {
            Recorded::clock(year, month, day, hour, minute, second, Some(offset))
        }
        Zone::Utc => Recorded::utc(year, month, day, hour, minute, second),
    }
}

enum Zone {
    /// Nothing after the clock.
    None,
    /// `Z` or `UTC`: the clock is UTC, so the value is a moment.
    Utc,
    /// `+02:00`, `+0200`, `-05`: minutes east of UTC.
    Offset(i32),
}

fn parse_zone(s: &str) -> Option<Zone> {
    if s.is_empty() {
        return Some(Zone::None);
    }
    if s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Some(Zone::Utc);
    }
    let sign = match s.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = s[1..].chars().filter(|c| *c != ':').collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (h, m) = match digits.len() {
        2 => (digits.parse::<i32>().ok()?, 0),
        4 => (
            digits[..2].parse::<i32>().ok()?,
            digits[2..].parse::<i32>().ok()?,
        ),
        _ => return None,
    };
    (h <= 14 && m < 60).then_some(Zone::Offset(sign * (h * 60 + m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    fn clock(text: &str, offset: Option<i32>) -> Recorded {
        Recorded::Clock {
            local: text.parse().unwrap(),
            offset,
        }
    }

    #[test]
    fn a_clock_with_its_offset_keeps_both() {
        // 12:21 at +02:00 is 10:21 UTC, and it stays 12:21 wherever the server is.
        for server in ["UTC", "America/Los_Angeles", "Asia/Tokyo"] {
            let t = clock("2024-06-13T12:21:00", Some(120))
                .resolve(&zone(server))
                .unwrap();
            assert_eq!(t.at, 1_718_274_060, "{server}");
            assert_eq!(t.local_text(), "2024-06-13T12:21:00", "{server}");
            assert_eq!(t.offset, Some(120));
            assert_eq!(t.month(), "2024-06");
        }
    }

    #[test]
    fn a_clock_without_an_offset_is_never_taken_for_utc() {
        // The clock is what the camera showed; the server's zone only places the moment.
        let ny = clock("2024-06-30T22:30:00", None)
            .resolve(&zone("America/New_York"))
            .unwrap();
        assert_eq!(ny.local_text(), "2024-06-30T22:30:00");
        assert_eq!(ny.offset, None);
        assert_eq!(ny.month(), "2024-06", "not July, as UTC would make it");
        assert_eq!(ny.at, 1_719_801_000, "22:30 EDT is 02:30 UTC on July 1");
        let utc = clock("2024-06-30T22:30:00", None)
            .resolve(&TimeZone::UTC)
            .unwrap();
        assert_eq!(utc.local_text(), "2024-06-30T22:30:00");
        assert_eq!(utc.at, 1_719_786_600);
    }

    /// Android and Samsung clips: a UTC creation time and the phone's clock in the name.
    #[test]
    fn a_name_clock_and_a_utc_moment_give_the_offset() {
        // 2024-07-01T02:30:00Z, named 22:30 on June 30: filmed at -04:00.
        const MOMENT: i64 = 1_719_801_000;
        let name: DateTime = "2024-06-30T22:30:00".parse().unwrap();
        let paired = |moment: i64, length: Option<f64>| {
            with_name_clock(Some(Recorded::Instant(moment)), Some(name), length)
        };
        let filmed = clock("2024-06-30T22:30:00", Some(-240));
        let expected = Some(filmed);
        assert_eq!(paired(MOMENT, None), expected);
        for server in ["UTC", "America/New_York", "Asia/Tokyo"] {
            let t = filmed.resolve(&zone(server)).unwrap();
            assert_eq!(
                (t.at, t.local_text().as_str(), t.offset),
                (MOMENT, "2024-06-30T22:30:00", Some(-240)),
                "{server}"
            );
        }
        // A few seconds either way, and a moment written when a 95 second clip stopped.
        assert_eq!(paired(MOMENT + 2, None), expected);
        assert_eq!(paired(MOMENT - 3, Some(4.2)), expected);
        assert_eq!(paired(MOMENT + 95, Some(95.4)), expected);
        // Half and quarter hours exist: India, Nepal, the Chatham Islands.
        let kathmandu: DateTime = "2024-07-01T08:15:00".parse().unwrap();
        assert_eq!(
            with_name_clock(Some(Recorded::Instant(MOMENT)), Some(kathmandu), None),
            Some(Recorded::Clock {
                local: kathmandu,
                offset: Some(345)
            })
        );
        // Nothing fits: the moment stays as it was.
        for (moment, length) in [
            (MOMENT + 7 * 60, None),
            (MOMENT + 95, None),
            (MOMENT + 95, Some(30.0)),
            (MOMENT - 19 * 3600, None),
            (MOMENT + 15 * 3600, None),
            // Started and stopped a quarter hour apart: either offset would do.
            (MOMENT + 900, Some(900.0)),
        ] {
            assert_eq!(
                paired(moment, length),
                Some(Recorded::Instant(moment)),
                "{moment} {length:?}"
            );
        }
        // A clock from the tags, no name clock, or no moment: unchanged.
        let tagged = Some(clock("2024-06-30T22:30:00", Some(-240)));
        assert_eq!(with_name_clock(tagged, Some(name), None), tagged);
        let instant = Some(Recorded::Instant(MOMENT));
        assert_eq!(with_name_clock(instant, None, None), instant);
        assert_eq!(with_name_clock(None, Some(name), None), None);
    }

    #[test]
    fn a_moment_gets_its_clock_from_the_server_zone() {
        // 2024-07-01T02:30:00Z: the evening of June 30 in New York, July 1 in UTC and Tokyo.
        let at = 1_719_801_000;
        let read = |name: &str| Recorded::Instant(at).resolve(&zone(name)).unwrap();
        assert_eq!(read("America/New_York").local_text(), "2024-06-30T22:30:00");
        assert_eq!(read("America/New_York").month(), "2024-06");
        assert_eq!(read("UTC").local_text(), "2024-07-01T02:30:00");
        assert_eq!(read("Asia/Tokyo").local_text(), "2024-07-01T11:30:00");
        for name in ["America/New_York", "UTC", "Asia/Tokyo"] {
            assert_eq!(read(name).at, at, "the moment never moves");
            assert_eq!(read(name).offset, None);
        }
        // POSIX rules in TZ work too.
        let posix = TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!(
            Recorded::Instant(at).resolve(&posix).unwrap().local_text(),
            "2024-06-30T22:30:00"
        );
    }

    #[test]
    fn daylight_saving_edges() {
        let ny = zone("America/New_York");
        // 02:30 on 2024-03-10 does not exist in New York: the moment after the gap.
        let gap = clock("2024-03-10T02:30:00", None).resolve(&ny).unwrap();
        assert_eq!(gap.local_text(), "2024-03-10T02:30:00", "the clock is kept");
        assert_eq!(gap.at, 1_710_055_800);
        // 01:30 on 2024-11-03 happens twice: the first.
        let fold = clock("2024-11-03T01:30:00", None).resolve(&ny).unwrap();
        assert_eq!(fold.at, 1_730_611_800);
    }

    #[test]
    fn placeholders_and_impossible_values() {
        assert_eq!(Recorded::clock(1970, 1, 1, 0, 0, 0, None), None);
        assert_eq!(Recorded::clock(1904, 1, 1, 0, 0, 0, None), None);
        assert_eq!(Recorded::clock(1970, 1, 1, 2, 0, 0, Some(120)), None);
        assert_eq!(Recorded::clock(0, 0, 0, 0, 0, 0, None), None);
        assert_eq!(Recorded::clock(2024, 2, 30, 10, 0, 0, None), None);
        assert_eq!(Recorded::clock(2024, 13, 1, 0, 0, 0, None), None);
        assert_eq!(Recorded::clock(2024, 6, 13, 24, 0, 0, None), None);
        assert_eq!(Recorded::clock(1799, 6, 13, 0, 0, 0, None), None);
        assert_eq!(Recorded::clock(2024, 6, 13, 10, 0, 0, Some(20 * 60)), None);
        assert_eq!(
            Recorded::clock(1965, 7, 4, 12, 0, 0, Some(0))
                .unwrap()
                .resolve(&TimeZone::UTC)
                .unwrap()
                .at,
            -141_825_600
        );
        assert_eq!(
            Recorded::clock(2024, 6, 13, 10, 21, 60, None),
            Some(clock("2024-06-13T10:21:59", None)),
            "a leap second counts as 59"
        );
        assert_eq!(Recorded::instant(0), None);
        assert_eq!(Recorded::instant(QUICKTIME_EPOCH), None);
        assert_eq!(
            Recorded::Instant(i64::MAX).resolve(&TimeZone::UTC),
            None,
            "out of range"
        );
    }

    #[test]
    fn tag_times_with_and_without_zones() {
        assert_eq!(
            parse_tag_time("2024-06-13T12:21:00+0200"),
            Some(clock("2024-06-13T12:21:00", Some(120)))
        );
        assert_eq!(
            parse_tag_time("2024-06-13T12:21:00+02:00"),
            Some(clock("2024-06-13T12:21:00", Some(120)))
        );
        assert_eq!(
            parse_tag_time("2024-06-13T05:21:00-05"),
            Some(clock("2024-06-13T05:21:00", Some(-300)))
        );
        assert_eq!(
            parse_tag_time("2024-06-13T10:21:00+00:00"),
            Some(clock("2024-06-13T10:21:00", Some(0))),
            "an explicit +00:00 is a real offset"
        );
        // A container creation time in UTC is a moment, not a clock.
        assert_eq!(
            parse_tag_time("2024-06-13T10:21:00.000000Z"),
            Some(Recorded::Instant(1_718_274_060))
        );
        assert_eq!(
            parse_tag_time("2024-06-13 10:21:00 UTC"),
            Some(Recorded::Instant(1_718_274_060))
        );
        // No zone: the clock is all there is.
        assert_eq!(
            parse_tag_time("2024-06-13 10:21:00"),
            Some(clock("2024-06-13T10:21:00", None))
        );
        assert_eq!(
            parse_tag_time("2024:06:13 10:21:00"),
            Some(clock("2024-06-13T10:21:00", None))
        );
        assert_eq!(
            parse_tag_time("2024-06-13"),
            Some(clock("2024-06-13T00:00:00", None))
        );
        assert_eq!(parse_tag_time("1970-01-01T00:00:00.000000Z"), None);
        assert_eq!(parse_tag_time("1904-01-01T00:00:00Z"), None);
        assert_eq!(parse_tag_time("2001"), None);
        assert_eq!(parse_tag_time("June 13, 2024"), None);
        assert_eq!(parse_tag_time("2024-06-13T10:21:00 nonsense"), None);
        assert_eq!(parse_tag_time("2024-06-13T1"), None);
        assert_eq!(parse_tag_time("2024-06-13T+1:00:00"), None);
        assert_eq!(parse_tag_time("２０２４-06-13"), None);
    }

    #[test]
    fn stored_columns_round_trip() {
        let t = clock("2024-06-13T12:21:00", Some(120))
            .resolve(&TimeZone::UTC)
            .unwrap();
        let (at, local, offset) = CaptureTime::columns(Some(&t));
        assert_eq!(
            (at, local.as_deref(), offset),
            (Some(1_718_274_060), Some("2024-06-13T12:21:00"), Some(120))
        );
        assert_eq!(
            CaptureTime::from_stored(at, local.as_deref(), offset),
            Some(t)
        );
        assert_eq!(CaptureTime::columns(None), (None, None, None));
        // Before migration 0003: the moment read in UTC.
        let old = CaptureTime::from_stored(Some(1_718_274_060), None, None).unwrap();
        assert_eq!(old.local_text(), "2024-06-13T10:21:00");
        assert_eq!(
            CaptureTime::from_stored(None, Some("2024-06-13T10:21:00"), None),
            None
        );
    }
}

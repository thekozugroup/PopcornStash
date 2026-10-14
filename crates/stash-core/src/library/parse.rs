//! Release-name and camera-name parsing (section 4).
//!
//! [`parse_name`] looks at a file stem (or folder name) and pulls out what the
//! name says about the file: a movie title and year, a show with season and
//! episode numbers, or the tell-tale names that phones and cameras give their
//! files (with the capture date when the name carries one).

use std::sync::LazyLock;

use regex::Regex;

use crate::capture::Recorded;

/// What a file name says about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedName {
    /// Cleaned title (movie title, or show name for episodes).
    pub title: String,
    pub year: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    /// Last episode for multi-episode files (S01E02E03).
    pub episode_end: Option<i64>,
    pub episode_title: Option<String>,
    /// Release tags were present (1080p, BluRay, x264, WEB-DL, group…).
    pub has_release_tags: bool,
    /// The name looks like a phone/camera file (IMG_1234, VID_2024…, PXL_, GOPR, DJI_…).
    pub camera_style: bool,
    /// Date embedded in the name: the camera's clock (midnight when the name has no
    /// time), or a moment for names that carry one (`PXL_` names are UTC, `RPReplay_Final`
    /// names a unix time).
    pub date: Option<Recorded>,
}

macro_rules! re {
    ($pattern:expr) => {{
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("static regex is valid"));
        &*RE
    }};
}

/// Parse a file stem (no extension) or folder name.
pub fn parse_name(stem: &str) -> ParsedName {
    let stem = stem.trim();
    let mut out = ParsedName {
        date: find_date(stem),
        ..ParsedName::default()
    };

    if let Some(camera_date) = camera_name(stem) {
        out.camera_style = true;
        out.title = stem.to_string();
        if camera_date.is_some() {
            out.date = camera_date;
        }
        return out;
    }

    let norm = normalize(stem);
    let tokens: Vec<&str> = norm.split(' ').filter(|t| !t.is_empty()).collect();
    out.has_release_tags = tokens
        .iter()
        .enumerate()
        .skip(1)
        .any(|(i, t)| is_release_tag(t, i > 0 && is_year_token(tokens[i - 1]).is_some()));

    if let Some(marker) = find_episode(&norm) {
        let (show, year) =
            split_trailing_year(&clean_title(&title_before_tags(&norm[..marker.start])));
        out.title = smart_case(&show);
        out.year = year;
        out.season = marker.season;
        out.episode = marker.episode;
        out.episode_end = marker.episode_end;
        if marker.episode.is_some() {
            let after = clean_title(&title_until_tags(&norm[marker.end..]));
            out.episode_title = (!after.is_empty()).then_some(after);
        }
        return out;
    }

    // Movie-style (or plain) name: the title runs until the release tags start; the
    // year is the last plausible year token in that range (bracketed years win).
    let tag_start = first_tag_index(&tokens).unwrap_or(tokens.len());
    let head = &tokens[..tag_start];
    let mut year_at: Option<(usize, i64)> = None;
    let mut bracketed_at: Option<(usize, i64)> = None;
    for (i, token) in head.iter().enumerate().skip(1) {
        if let Some((year, bracketed)) = is_year_token(token)
            && plausible_year(year)
        {
            year_at = Some((i, year));
            if bracketed {
                bracketed_at = Some((i, year));
            }
        }
    }
    match bracketed_at.or(year_at) {
        Some((i, year)) => {
            out.title = smart_case(&clean_title(&head[..i].join(" ")));
            out.year = Some(year);
        }
        None => out.title = smart_case(&clean_title(&head.join(" "))),
    }
    if out.title.is_empty() {
        out.title = clean_title(&norm);
    }
    out
}

/// True when `year` is a believable release year: 1900 up to next year.
pub fn plausible_year(year: i64) -> bool {
    let next_year = i64::from(time::OffsetDateTime::now_utc().year()) + 1;
    (1900..=next_year).contains(&year)
}

/// `"Season 01"`, `"S2"`, `"Series 3"` → season number; `"Specials"` → 0.
pub fn season_folder(name: &str) -> Option<i64> {
    let name = name.trim();
    if name.eq_ignore_ascii_case("specials") || name.eq_ignore_ascii_case("special") {
        return Some(0);
    }
    re!(r"(?i)^(?:season|series|s)[\s._-]*(\d{1,3})$")
        .captures(name)
        .and_then(|c| c[1].parse().ok())
}

// ---------------------------------------------------------------------------
// Camera names and dates

/// `Some(date)` when the stem looks like a phone or camera file name.
fn camera_name(stem: &str) -> Option<Option<Recorded>> {
    // Names that carry a date and time. Pixel phones write theirs in UTC; the others
    // write the phone's clock.
    if let Some(c) =
        re!(r"(?i)^(IMG|VID|PANO|BURST|MVIMG|PXL|Screenshot)_(\d{8})_(\d{6})").captures(stem)
    {
        let date = compact_datetime(&c[2], Some(&c[3]));
        if c[1].eq_ignore_ascii_case("PXL") {
            return Some(date.and_then(Recorded::in_utc));
        }
        return Some(date);
    }
    if let Some(c) = re!(r"(?i)^Screenshot_(\d{8})-(\d{6})").captures(stem) {
        return Some(compact_datetime(&c[1], Some(&c[2])));
    }
    if let Some(c) = re!(r"(?i)^(?:IMG|VID)-(\d{8})-WA\d+").captures(stem) {
        return Some(compact_datetime(&c[1], None));
    }
    if let Some(c) = re!(r"^(\d{8})_(\d{6})").captures(stem) {
        return Some(compact_datetime(&c[1], Some(&c[2])));
    }
    if let Some(c) = re!(
        r"(?i)^(?:Screen Recording|Screenshot|Screen Shot|WhatsApp (?:Video|Image)) (\d{4})-(\d{2})-(\d{2})(?: at (\d{1,2})[.:](\d{2})[.:](\d{2})(?:\s?([AP])\.?M\.?)?)?"
    )
    .captures(stem)
    {
        let mut hour: i64 = c.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        if let Some(half) = c.get(7) {
            let pm = half.as_str().eq_ignore_ascii_case("p");
            hour = match (hour, pm) {
                (12, false) => 0,
                (12, true) => 12,
                (h, true) => h + 12,
                (h, false) => h,
            };
        }
        let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
        return Some(clock_from_parts(num(1), num(2), num(3), hour, num(5), num(6)));
    }
    if let Some(c) = re!(r"^(\d{4})-(\d{2})-(\d{2}) (\d{2})\.(\d{2})\.(\d{2})").captures(stem) {
        let num = |i: usize| c[i].parse::<i64>().unwrap_or(0);
        return Some(clock_from_parts(
            num(1),
            num(2),
            num(3),
            num(4),
            num(5),
            num(6),
        ));
    }
    if let Some(c) = re!(r"(?i)^RPReplay_Final(\d{9,10})").captures(stem) {
        let ts: i64 = c[1].parse().unwrap_or(0);
        return Some((ts > 946_684_800).then(|| Recorded::instant(ts)).flatten());
    }
    if let Some(c) = re!(r"(?i)^DJI_(\d{8})(\d{6})").captures(stem) {
        return Some(compact_datetime(&c[1], Some(&c[2])));
    }
    // Names without a date.
    let undated = re!(
        r"(?i)^(?:IMG_E?\d{4}|VID_\d{4}|MVI_\d{4}|MOV_\d{4}|DSC[FN_]?\d{4,5}|GOPR\d{4}|GP\d{6}|G[HXL]\d{6}|DJI_\d{4}|PXL_\d+|RPReplay|Screen Recording|C\d{4}$|P\d{7}$)"
    );
    undated.is_match(stem).then_some(None)
}

/// `YYYYMMDD` (+ optional `HHMMSS`) → the clock it names.
fn compact_datetime(date: &str, time: Option<&str>) -> Option<Recorded> {
    let d = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<i64>().ok());
    let t = |a: usize, b: usize| {
        time.and_then(|t| t.get(a..b))
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    clock_from_parts(d(0, 4)?, d(4, 6)?, d(6, 8)?, t(0, 2), t(2, 4), t(4, 6))
}

/// Calendar parts → the clock reading a name carries (no offset: names never say where),
/// None when the date is not real or outside 1970–2099.
fn clock_from_parts(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<Recorded> {
    if !(1970..=2099).contains(&year) || !(0..=59).contains(&second) {
        return None;
    }
    Recorded::clock(year, month, day, hour, minute, second, None)
}

/// A date anywhere in the name: `20240613`, `2024-06-13`, `2024_06_13`, `2024.06.13`,
/// optionally followed by a time (`_101500`, ` 10.15.00`, `T10:15:00`).
fn find_date(s: &str) -> Option<Recorded> {
    let re = re!(
        r"((?:19|20)\d{2})([-_.]?)(\d{2})([-_.]?)(\d{2})(?:[ _T.-]?(\d{2})[.:-]?(\d{2})[.:-]?(\d{2}))?"
    );
    let bytes = s.as_bytes();
    let mut start = 0;
    while start < s.len() {
        let Some(c) = re.captures_at(s, start) else {
            break;
        };
        let Some(whole) = c.get(0) else { break };
        let before_ok = whole.start() == 0 || !bytes[whole.start() - 1].is_ascii_digit();
        let date_end = c.get(5).map_or(whole.end(), |m| m.end());
        let (end, has_time) = match c.get(8) {
            Some(m) => (m.end(), true),
            None => (date_end, false),
        };
        let after_ok = end >= s.len() || !bytes[end].is_ascii_digit();
        let seps_match = c.get(2).map(|m| m.as_str()) == c.get(4).map(|m| m.as_str());
        if before_ok && seps_match {
            let num = |i: usize| {
                c.get(i)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0)
            };
            if after_ok {
                let parsed = if has_time {
                    clock_from_parts(num(1), num(3), num(5), num(6), num(7), num(8))
                } else {
                    clock_from_parts(num(1), num(3), num(5), 0, 0, 0)
                };
                if parsed.is_some() {
                    return parsed;
                }
            }
            // The time part may have swallowed digits that belong elsewhere: retry date only.
            let date_after_ok = date_end >= s.len() || !bytes[date_end].is_ascii_digit();
            if has_time
                && date_after_ok
                && let Some(ts) = clock_from_parts(num(1), num(3), num(5), 0, 0, 0)
            {
                return Some(ts);
            }
        }
        start = whole.start() + s[whole.start()..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

// ---------------------------------------------------------------------------
// Release names

struct EpisodeMarker {
    start: usize,
    end: usize,
    season: Option<i64>,
    episode: Option<i64>,
    episode_end: Option<i64>,
}

fn find_episode(norm: &str) -> Option<EpisodeMarker> {
    // S01E02, S01E02E03, S01E02-E03, S01E02-03, S01 E02
    if let Some(c) =
        re!(r"(?i)\bS(\d{1,2})\s?E(\d{1,3})((?:\s?-?\s?E\d{1,3}|-\d{1,3}\b)*)").captures(norm)
    {
        let whole = c.get(0)?;
        let first: i64 = c[2].parse().ok()?;
        let last = re!(r"\d{1,3}")
            .find_iter(c.get(3).map_or("", |m| m.as_str()))
            .filter_map(|m| m.as_str().parse::<i64>().ok())
            .last();
        return Some(EpisodeMarker {
            start: whole.start(),
            end: whole.end(),
            season: c[1].parse().ok(),
            episode: Some(first),
            episode_end: last.filter(|&l| l > first),
        });
    }
    // Season 1 Episode 2
    if let Some(c) =
        re!(r"(?i)\bseason\s*(\d{1,2})\s*[,.-]?\s*(?:episode|ep\.?)\s*(\d{1,3})\b").captures(norm)
    {
        let whole = c.get(0)?;
        return Some(EpisodeMarker {
            start: whole.start(),
            end: whole.end(),
            season: c[1].parse().ok(),
            episode: c[2].parse().ok(),
            episode_end: None,
        });
    }
    // 1x02 (not 1920x1080)
    if let Some(c) = re!(r"(?i)\b(\d{1,2})x(\d{2,3})\b(?:-(\d{2,3})\b)?").captures(norm) {
        let whole = c.get(0)?;
        let first: i64 = c[2].parse().ok()?;
        return Some(EpisodeMarker {
            start: whole.start(),
            end: whole.end(),
            season: c[1].parse().ok(),
            episode: Some(first),
            episode_end: c
                .get(3)
                .and_then(|m| m.as_str().parse::<i64>().ok())
                .filter(|&l| l > first),
        });
    }
    // Season packs: "Show S01", "Show Season 2" (no episode).
    if let Some(c) = re!(r"(?i)\b(?:S(\d{1,2})|season\s*(\d{1,2}))\b").captures(norm) {
        let whole = c.get(0)?;
        if whole.start() == 0 {
            return None;
        }
        let season = c
            .get(1)
            .or_else(|| c.get(2))
            .and_then(|m| m.as_str().parse::<i64>().ok());
        return Some(EpisodeMarker {
            start: whole.start(),
            end: whole.end(),
            season,
            episode: None,
            episode_end: None,
        });
    }
    None
}

/// Separators to spaces, `H.264` → `H264`, leading `[Group]` removed, whitespace collapsed.
/// Dots followed by a space ("Mr. Robot") are kept.
fn normalize(stem: &str) -> String {
    let s = re!(r"(?i)\b([hx])\.(26[45])\b").replace_all(stem, "$1$2");
    let s = re!(r"^\s*\[[^\]]*\]\s*(\S)").replace(&s, "$1");
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '_' => out.push(' '),
            '.' if chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `2019`, `(2019)`, `[2019]` → (year, bracketed).
fn is_year_token(token: &str) -> Option<(i64, bool)> {
    let c = re!(r"^([\(\[\{]?)((?:19|20)\d{2})[\)\]\}]?$").captures(token)?;
    Some((c[2].parse().ok()?, !c[1].is_empty()))
}

fn strip_brackets(token: &str) -> &str {
    token.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | '{' | '}'))
}

/// Resolution, source, codec, audio, HDR and well-known group tags.
fn is_strong_tag(token: &str) -> bool {
    let t = strip_brackets(token).to_ascii_lowercase();
    !t.is_empty()
        && re!(
            r"^(?:\d{3,4}[pi]|[48]k|uhd|fhd|blu-?ray|bd(?:rip|remux|25|50)?|br-?rip|remux|web(?:-?(?:dl|rip|hd))?|hdtv(?:rip)?|pdtv|sdtv|hd-?rip|dvd(?:rip|scr|r|5|9)?|hdcam|camrip|telesync|hdts|tvrip|vhsrip|r5|ppv(?:rip)?|amzn|nf|dsnp|hmax|atvp|pcok|hulu|itunes|[xh]26[45]|hevc|avc|xvid|divx|av1|vp9|mpeg-?2|10-?bit|8-?bit|hi10p?|hdr(?:10(?:\+|plus)?)?|dv|dovi|hlg|sdr|(?:aac|ac3|e-?ac-?3|dts(?:-?(?:hd|x|ma))?|ddp?|dd\+|truehd|atmos|flac|mp3|opus|lpcm)[\d.]*(?:ch)?|\d+ch|yify|yts(?:\.\w+)?|rarbg|eztv|ettv|tgx)(?:-[a-z0-9]+)?$"
        )
        .is_match(&t)
}

/// Edition words. They only count as release tags when written scene-style (all caps)
/// or right after a year, so titles like "The Extended Family" survive.
fn is_weak_tag(token: &str, after_year: bool) -> bool {
    let raw = strip_brackets(token);
    let t = raw.to_ascii_lowercase();
    let word = re!(
        r"^(?:proper|repack|rerip|extended|unrated|uncut|remastered|internal|limited|multi(?:subs?)?|dubbed|subbed|hardsub|imax|hybrid|complete)(?:-[a-z0-9]+)?$"
    )
    .is_match(&t);
    let shouting = raw.chars().any(|c| c.is_ascii_alphabetic())
        && !raw.chars().any(|c| c.is_ascii_lowercase());
    word && (shouting || after_year)
}

fn is_release_tag(token: &str, after_year: bool) -> bool {
    is_strong_tag(token) || is_weak_tag(token, after_year)
}

/// Index of the first release tag, never the first token (a title needs one word).
fn first_tag_index(tokens: &[&str]) -> Option<usize> {
    (1..tokens.len()).find(|&i| is_release_tag(tokens[i], is_year_token(tokens[i - 1]).is_some()))
}

/// The text before the first release tag (for the part ahead of an episode marker).
fn title_before_tags(s: &str) -> String {
    let tokens: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();
    let end = first_tag_index(&tokens).unwrap_or(tokens.len());
    tokens[..end].join(" ")
}

/// The text after an episode marker, up to the first release tag.
fn title_until_tags(s: &str) -> String {
    let tokens: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();
    let end = (0..tokens.len())
        .find(|&i| is_release_tag(tokens[i], i > 0 && is_year_token(tokens[i - 1]).is_some()))
        .unwrap_or(tokens.len());
    let mut kept = tokens[..end].to_vec();
    // A trailing "-GROUP" only shows up when tags follow; drop a bare trailing year too.
    if end < tokens.len() {
        while kept.last().is_some_and(|t| is_year_token(t).is_some()) {
            kept.pop();
        }
    }
    kept.join(" ")
}

/// "Show Name 2005" → ("Show Name", Some(2005)); never leaves the name empty.
fn split_trailing_year(s: &str) -> (String, Option<i64>) {
    let tokens: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();
    if tokens.len() >= 2
        && let Some((year, _)) = tokens.last().and_then(|t| is_year_token(t))
        && plausible_year(year)
    {
        return (
            clean_title(&tokens[..tokens.len() - 1].join(" ")),
            Some(year),
        );
    }
    (s.to_string(), None)
}

/// Trim separators and stray brackets from the ends and drop empty bracket pairs.
fn clean_title(s: &str) -> String {
    let mut t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let before = t.clone();
        t = re!(r"[\(\[\{]\s*[\)\]\}]").replace_all(&t, "").into_owned();
        t = t
            .trim_matches(|c: char| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '-' | '–' | '—' | '_' | '.' | ',' | ':' | ';' | '~' | '+' | '|'
                    )
            })
            .to_string();
        t = t
            .trim_end_matches(['(', '[', '{'])
            .trim_start_matches([')', ']', '}'])
            .to_string();
        t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t == before {
            return t;
        }
    }
}

/// Title-case names written entirely in lower case ("breaking bad" → "Breaking Bad").
fn smart_case(s: &str) -> String {
    if s.chars().any(char::is_uppercase) || !s.chars().any(char::is_lowercase) {
        return s.to_string();
    }
    s.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<Recorded> {
        Some(Recorded::clock(y, mo, d, h, mi, s, None).expect("a real clock reading"))
    }

    #[test]
    fn episodes() {
        let p = parse_name("The.Office.US.S02E03.720p.WEB-DL.x264-GRP");
        assert_eq!(p.title, "The Office US");
        assert_eq!(
            (p.season, p.episode, p.episode_end),
            (Some(2), Some(3), None)
        );
        assert_eq!(p.episode_title, None);
        assert!(p.has_release_tags);
        assert!(!p.camera_style);

        let p = parse_name("Breaking Bad - 1x02 - Cat's in the Bag");
        assert_eq!(p.title, "Breaking Bad");
        assert_eq!((p.season, p.episode), (Some(1), Some(2)));
        assert_eq!(p.episode_title.as_deref(), Some("Cat's in the Bag"));
        assert!(!p.has_release_tags);

        let p = parse_name("Show.Name.S01E02E03");
        assert_eq!(p.title, "Show Name");
        assert_eq!(
            (p.season, p.episode, p.episode_end),
            (Some(1), Some(2), Some(3))
        );

        let p = parse_name("show name season 1 episode 4");
        assert_eq!(p.title, "Show Name");
        assert_eq!((p.season, p.episode), (Some(1), Some(4)));

        let p = parse_name("show.name.s1e2");
        assert_eq!(p.title, "Show Name");
        assert_eq!((p.season, p.episode), (Some(1), Some(2)));

        let p = parse_name("Show Name - S01E02-E04 - Triple Bill [1080p]");
        assert_eq!(
            (p.season, p.episode, p.episode_end),
            (Some(1), Some(2), Some(4))
        );
        assert_eq!(p.episode_title.as_deref(), Some("Triple Bill"));
        assert!(p.has_release_tags);

        let p = parse_name("Mr. Robot S01E01 eps1.0_hellofriend.mov");
        assert_eq!(p.title, "Mr. Robot");
        assert_eq!(p.episode, Some(1));

        let p = parse_name("Doctor.Who.2005.S01E01.Rose.1080p.BluRay");
        assert_eq!(p.title, "Doctor Who");
        assert_eq!(p.year, Some(2005));
        assert_eq!(p.episode_title.as_deref(), Some("Rose"));

        let p = parse_name("[SubsPlease] Frieren - S01E05 (1080p)");
        assert_eq!(p.title, "Frieren");
        assert_eq!(p.episode, Some(5));
        assert_eq!(p.episode_title, None);

        let p = parse_name("1883.S01E01.720p");
        assert_eq!(p.title, "1883");
        assert_eq!(p.year, None);

        let p = parse_name("S01E02");
        assert_eq!(p.title, "");
        assert_eq!((p.season, p.episode), (Some(1), Some(2)));
    }

    #[test]
    fn season_packs() {
        let p = parse_name("The.Office.US.S02.720p.WEB-DL");
        assert_eq!(p.title, "The Office US");
        assert_eq!((p.season, p.episode), (Some(2), None));
        let p = parse_name("Ted Lasso Season 3");
        assert_eq!(p.title, "Ted Lasso");
        assert_eq!((p.season, p.episode), (Some(3), None));
        assert_eq!(season_folder("Season 01"), Some(1));
        assert_eq!(season_folder("S2"), Some(2));
        assert_eq!(season_folder("Specials"), Some(0));
        assert_eq!(season_folder("Seasonal"), None);
    }

    #[test]
    fn resolution_is_not_an_episode() {
        let p = parse_name("Some.Movie.2010.1920x1080.x264");
        assert_eq!(p.episode, None);
        assert_eq!(p.title, "Some Movie");
        assert_eq!(p.year, Some(2010));
    }

    #[test]
    fn movies() {
        let cases: &[(&str, &str, Option<i64>, bool)] = &[
            ("Heat.1995.1080p.BluRay.x264-GRP", "Heat", Some(1995), true),
            (
                "Blade Runner 2049 (2017)",
                "Blade Runner 2049",
                Some(2017),
                false,
            ),
            (
                "Blade.Runner.2049.2017.2160p.UHD.BluRay",
                "Blade Runner 2049",
                Some(2017),
                true,
            ),
            (
                "2001.A.Space.Odyssey.1968.1080p",
                "2001 A Space Odyssey",
                Some(1968),
                true,
            ),
            ("1917 (2019)", "1917", Some(2019), false),
            ("Avatar [2009] 4K", "Avatar", Some(2009), true),
            ("1917", "1917", None, false),
            (
                "Movie.Title.2019.EXTENDED.1080p.WEB-DL.DDP5.1.H.264-GRP",
                "Movie Title",
                Some(2019),
                true,
            ),
            (
                "The Extended Family (2010)",
                "The Extended Family",
                Some(2010),
                false,
            ),
            ("heat.1995.720p", "Heat", Some(1995), true),
            ("Some.Movie.1080p.BluRay.x264", "Some Movie", None, true),
            (
                "Spider-Man Into the Spider-Verse (2018) [1080p]",
                "Spider-Man Into the Spider-Verse",
                Some(2018),
                true,
            ),
            ("Amélie.2001.DVDRip", "Amélie", Some(2001), true),
            ("Cam (2018)", "Cam", Some(2018), false),
            ("Our Wedding", "Our Wedding", None, false),
        ];
        for &(input, title, year, tags) in cases {
            let p = parse_name(input);
            assert_eq!(p.title, title, "title of {input}");
            assert_eq!(p.year, year, "year of {input}");
            assert_eq!(p.has_release_tags, tags, "tags of {input}");
            assert_eq!(p.season, None, "season of {input}");
            assert!(!p.camera_style, "camera of {input}");
        }
    }

    #[test]
    fn camera_names() {
        let cases: &[(&str, Option<Recorded>)] = &[
            ("IMG_1234", None),
            ("IMG_E1234", None),
            ("VID_20240613_101500", ts(2024, 6, 13, 10, 15, 0)),
            ("IMG_20240613_101500", ts(2024, 6, 13, 10, 15, 0)),
            // Pixel names are UTC: a moment, not a clock.
            (
                "PXL_20240613_101500123",
                Some(Recorded::Instant(1_718_273_700)),
            ),
            (
                "PXL_20240613_101500123.MP",
                Some(Recorded::Instant(1_718_273_700)),
            ),
            ("20240613_101500", ts(2024, 6, 13, 10, 15, 0)),
            (
                "Screen Recording 2024-06-13 at 10.15.00",
                ts(2024, 6, 13, 10, 15, 0),
            ),
            (
                "Screen Recording 2024-06-13 at 3.15.00 PM",
                ts(2024, 6, 13, 15, 15, 0),
            ),
            ("Screenshot_20240613-101500", ts(2024, 6, 13, 10, 15, 0)),
            (
                "WhatsApp Video 2024-06-13 at 10.15.00",
                ts(2024, 6, 13, 10, 15, 0),
            ),
            ("VID-20240613-WA0001", ts(2024, 6, 13, 0, 0, 0)),
            ("2024-06-13 10.15.00", ts(2024, 6, 13, 10, 15, 0)),
            ("GOPR0123", None),
            ("GX010123", None),
            ("GH010123", None),
            ("DJI_0042", None),
            ("DJI_20240613101500_0001_D", ts(2024, 6, 13, 10, 15, 0)),
            ("MVI_1234", None),
            ("MOV_0001", None),
            ("DSC01234", None),
            ("DSCF1234", None),
            ("C0001", None),
            (
                "RPReplay_Final1690000000",
                Some(Recorded::Instant(1_690_000_000)),
            ),
        ];
        for &(input, date) in cases {
            let p = parse_name(input);
            assert!(p.camera_style, "{input} should be camera style");
            assert_eq!(p.date, date, "date of {input}");
            assert_eq!(p.title, input);
            assert!(!p.has_release_tags);
            assert_eq!(p.episode, None);
        }
    }

    #[test]
    fn dates_in_ordinary_names() {
        assert_eq!(
            parse_name("Birthday 2024-06-13").date,
            ts(2024, 6, 13, 0, 0, 0)
        );
        assert_eq!(parse_name("beach_20230701").date, ts(2023, 7, 1, 0, 0, 0));
        assert_eq!(
            parse_name("party 2023.12.31 22.05.10").date,
            ts(2023, 12, 31, 22, 5, 10)
        );
        assert_eq!(parse_name("trip-2022_08_15").date, ts(2022, 8, 15, 0, 0, 0));
        assert_eq!(parse_name("Heat.1995.1080p").date, None);
        assert_eq!(parse_name("2024-13-45 nonsense").date, None);
        assert_eq!(parse_name("2024-06_13 mixed").date, None);
        assert_eq!(parse_name("123202406131").date, None);
    }

    #[test]
    fn years_and_cleaning() {
        assert!(plausible_year(1995));
        assert!(!plausible_year(1899));
        assert!(!plausible_year(2099));
        assert_eq!(clean_title(" - Title (  ) - "), "Title");
        assert_eq!(clean_title("Title ["), "Title");
        assert_eq!(smart_case("breaking bad"), "Breaking Bad");
        assert_eq!(smart_case("iCarly"), "iCarly");
        assert_eq!(normalize("A.B_C  D"), "A B C D");
        assert_eq!(normalize("Mr. Robot"), "Mr. Robot");
        assert_eq!(normalize("Movie.H.264"), "Movie H264");
    }
}

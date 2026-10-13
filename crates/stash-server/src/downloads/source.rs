//! Link parsing helpers: what is stored as a download's `source`, magnet links,
//! and file names derived from URLs.

use percent_encoding::percent_decode_str;
use url::Url;

/// Query keys that look like credentials. A query containing any of them is not stored.
const SECRET_KEY_PARTS: [&str; 9] = [
    "token",
    "key",
    "sig",
    "auth",
    "password",
    "passwd",
    "secret",
    "session",
    "credential",
];

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_KEY_PARTS.iter().any(|part| key.contains(part))
}

/// Fragment added to a stored link that lost private parts (user info or a query with
/// credentials). Such a link cannot be fetched again from what is stored.
pub const REDACTED_MARK: &str = "redacted";

/// The URL as stored in `downloads.source`: no user info, no fragment, and no query
/// when any query key looks like a credential. When anything private was removed the
/// result ends in `#redacted`, so a restart knows the stored link is incomplete.
pub fn redact_url(url: &Url) -> String {
    let mut clean = url.clone();
    let had_user_info = !clean.username().is_empty() || clean.password().is_some();
    let _ = clean.set_username("");
    let _ = clean.set_password(None);
    clean.set_fragment(None);
    let secret_query = clean.query_pairs().any(|(k, _)| is_secret_key(&k));
    if secret_query {
        clean.set_query(None);
    }
    if had_user_info || secret_query {
        clean.set_fragment(Some(REDACTED_MARK));
    }
    clean.to_string()
}

/// True for a link that [`redact_url`] stripped of private parts (it ends in `#redacted`).
pub fn is_redacted(url: &Url) -> bool {
    url.fragment() == Some(REDACTED_MARK)
}

/// True when a stored `source` is a link that lost private parts.
pub fn is_redacted_source(source: &str) -> bool {
    Url::parse(source).is_ok_and(|u| is_redacted(&u))
}

/// A parsed magnet link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magnet {
    /// The `xt` value, e.g. `urn:btih:abcdef…`.
    pub xt: String,
    /// The info hash part of `xt`.
    pub hash: String,
    pub display_name: Option<String>,
}

impl Magnet {
    /// Parse `magnet:?xt=urn:btih:…`. Returns None unless a BitTorrent `xt` is present.
    pub fn parse(link: &str) -> Option<Self> {
        let link = link.trim();
        if !link
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("magnet:"))
        {
            return None;
        }
        let query = link[7..].trim_start_matches('?');
        let mut xt = None;
        let mut display_name = None;
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match key.to_ascii_lowercase().as_str() {
                "xt" if xt.is_none() => {
                    let lower = value.to_ascii_lowercase();
                    if lower.starts_with("urn:btih:") || lower.starts_with("urn:btmh:") {
                        let hash = value[9..].trim().to_string();
                        if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_alphanumeric()) {
                            xt = Some((value.to_string(), hash));
                        }
                    }
                }
                "dn" if display_name.is_none() => {
                    let name = value.trim().to_string();
                    if !name.is_empty() {
                        display_name = Some(name);
                    }
                }
                _ => {}
            }
        }
        let (xt, hash) = xt?;
        Some(Self {
            xt,
            hash,
            display_name,
        })
    }

    /// What is stored as `source`: the info hash and name only. Tracker URLs can
    /// carry private-tracker passkeys, so they are left out.
    pub fn redacted(&self) -> String {
        let mut out = format!("magnet:?xt={}", self.xt);
        if let Some(name) = &self.display_name {
            out.push_str("&dn=");
            out.extend(url::form_urlencoded::byte_serialize(name.as_bytes()));
        }
        out
    }

    /// A name to show until the engine knows the real one.
    pub fn name(&self) -> String {
        match &self.display_name {
            Some(name) => name.clone(),
            None => format!("Torrent {}", &self.hash[..self.hash.len().min(8)]),
        }
    }
}

/// The last non-empty path segment of a URL, percent-decoded.
pub fn name_from_url(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(segment)
        .decode_utf8_lossy()
        .trim()
        .to_string();
    (!decoded.is_empty()).then_some(decoded)
}

/// True when the URL path ends in `.torrent`.
pub fn is_torrent_url(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".torrent")
}

/// File extension to add when a name has none, from the response's content type.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match essence.as_str() {
        "video/mp4" => "mp4",
        "video/x-matroska" => "mkv",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-msvideo" => "avi",
        "video/mp2t" => "ts",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/flac" => "flac",
        "application/zip" => "zip",
        "application/pdf" => "pdf",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redact(s: &str) -> String {
        redact_url(&Url::parse(s).unwrap())
    }

    #[test]
    fn redaction() {
        assert_eq!(
            redact("https://user:pw@example.com/a.mkv"),
            "https://example.com/a.mkv#redacted"
        );
        assert_eq!(
            redact("https://example.com/a.mkv?token=abc&x=1"),
            "https://example.com/a.mkv#redacted"
        );
        assert_eq!(
            redact("https://cdn.example.com/v.mp4?X-Amz-Signature=zz"),
            "https://cdn.example.com/v.mp4#redacted"
        );
        assert_eq!(
            redact("https://example.com/a.mkv?api_KEY=1"),
            "https://example.com/a.mkv#redacted"
        );
        assert_eq!(
            redact("https://example.com/get?id=42&format=mp4"),
            "https://example.com/get?id=42&format=mp4"
        );
        assert_eq!(
            redact("https://example.com/a.mkv#frag"),
            "https://example.com/a.mkv"
        );
        assert!(is_redacted_source(&redact(
            "https://example.com/a.mkv?token=abc"
        )));
        assert!(!is_redacted_source(&redact(
            "https://example.com/a.mkv#redacted"
        )));
        assert!(!is_redacted_source("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn magnets() {
        let m = Magnet::parse(
            "magnet:?xt=urn:btih:0123456789ABCDEF0123456789abcdef01234567&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.example%2Fpasskey123",
        )
        .unwrap();
        assert_eq!(m.hash, "0123456789ABCDEF0123456789abcdef01234567");
        assert_eq!(m.name(), "Big Buck Bunny");
        let redacted = m.redacted();
        assert!(redacted.starts_with("magnet:?xt=urn:btih:0123456789ABCDEF"));
        assert!(redacted.contains("dn=Big+Buck+Bunny"));
        assert!(!redacted.contains("passkey123"));
        assert!(Magnet::parse("magnet:?dn=nothing").is_none());
        assert!(Magnet::parse("https://example.com").is_none());
        assert_eq!(
            Magnet::parse("MAGNET:?xt=urn:btih:abcdef1234")
                .unwrap()
                .name(),
            "Torrent abcdef12"
        );
    }

    #[test]
    fn url_names() {
        let n = |s: &str| name_from_url(&Url::parse(s).unwrap());
        assert_eq!(
            n("https://example.com/files/My%20Movie.mkv").as_deref(),
            Some("My Movie.mkv")
        );
        assert_eq!(n("https://example.com/dir/").as_deref(), Some("dir"));
        assert_eq!(n("https://example.com/"), None);
        assert!(is_torrent_url(
            &Url::parse("https://example.com/x.TORRENT?a=1").unwrap()
        ));
        assert_eq!(extension_for_mime("video/mp4; charset=binary"), Some("mp4"));
    }
}

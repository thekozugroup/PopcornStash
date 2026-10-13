//! Security middleware: response headers, CSRF origin checks, rate limiting, client IP.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::body::HttpBody;
use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::Method;
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use governor::{DefaultKeyedRateLimiter, Quota};
use stash_core::config::Config;

use crate::error::ApiError;
use crate::state::AppState;

/// Content-Security-Policy from section 7.
pub const CSP: &str = "default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; worker-src 'self' blob:; \
style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=()";
pub const HSTS: &str = "max-age=31536000";

/// The client address, honoring `X-Forwarded-For` only when `STASH_TRUST_PROXY` is set.
#[derive(Debug, Clone)]
pub struct ClientIp(pub Option<IpAddr>);

impl ClientIp {
    /// Resolve from request parts (works without `ConnectInfo`, e.g. in tests).
    pub fn from_parts(parts: &Parts, config: &Config) -> Self {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip());
        Self(client_ip(&parts.headers, peer, config.trust_proxy))
    }

    /// Text form for the database and logs.
    pub fn as_string(&self) -> Option<String> {
        self.0.map(|ip| ip.to_string())
    }

    /// Rate-limit key for `scope`, e.g. `auth:192.0.2.1`.
    ///
    /// IPv6 addresses are grouped by /64: one home connection or server gets a whole
    /// /64, so a per-address key would hand an attacker billions of fresh budgets.
    pub fn key(&self, scope: &str) -> String {
        match self.0.map(|ip| ip.to_canonical()) {
            Some(IpAddr::V6(ip)) => {
                let s = ip.segments();
                let network = Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0);
                format!("{scope}:{network}/64")
            }
            Some(ip) => format!("{scope}:{ip}"),
            None => format!("{scope}:unknown"),
        }
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self::from_parts(parts, &state.config))
    }
}

/// The peer address, or the last `X-Forwarded-For` entry when the proxy is trusted.
///
/// Reverse proxies (SWAG, Nginx Proxy Manager, Traefik, Cloudflare) append the address
/// they saw to whatever the client sent, so only the last entry is trustworthy; the
/// rest is client-controlled and would let anyone pick a fresh rate-limit budget or
/// write any address into the audit log. IPv4-mapped IPv6 addresses become IPv4.
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .next_back()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(parse_forwarded_ip);
        if let Some(ip) = forwarded {
            return Some(ip.to_canonical());
        }
    }
    peer.map(|ip| ip.to_canonical())
}

fn parse_forwarded_ip(entry: &str) -> Option<IpAddr> {
    let entry = entry.trim();
    entry
        .parse::<IpAddr>()
        .ok()
        .or_else(|| entry.parse::<SocketAddr>().ok().map(|s| s.ip()))
        .or_else(|| {
            entry
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .ok()
        })
}

/// A request path as it may appear in logs: invite and recovery codes (which work as
/// passwords until used) are replaced, and the query string (pairing codes, offsets)
/// is never included.
pub fn loggable_path(path: &str) -> String {
    const SECRET_AFTER: [&str; 3] = ["/invite/", "/recover/", "/api/invites/"];
    for prefix in SECRET_AFTER {
        if let Some(rest) = path.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return format!("{prefix}<redacted>");
        }
    }
    path.to_string()
}

/// Requests per minute allowed per key.
const PER_MINUTE: NonZeroU32 = NonZeroU32::new(10).expect("non-zero");
/// Short bursts allowed per key.
const BURST: NonZeroU32 = NonZeroU32::new(20).expect("non-zero");
/// How often idle keys are dropped.
const CLEANUP_EVERY: Duration = Duration::from_secs(60);

/// Per-IP token buckets for auth and pairing endpoints (10/min, burst 20).
pub struct RateLimiter {
    limiter: DefaultKeyedRateLimiter<String>,
    last_cleanup: Mutex<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::with_quota(Quota::per_minute(PER_MINUTE).allow_burst(BURST))
    }

    /// A limiter with a custom quota (tests).
    pub fn with_quota(quota: Quota) -> Self {
        Self {
            limiter: DefaultKeyedRateLimiter::keyed(quota),
            last_cleanup: Mutex::new(Instant::now()),
        }
    }

    /// Err(rate_limited) when `key` (e.g. "auth:1.2.3.4") is over its budget.
    pub fn check(&self, key: &str) -> Result<(), ApiError> {
        self.cleanup_if_due();
        match self.limiter.check_key(&key.to_string()) {
            Ok(()) => Ok(()),
            Err(_) => {
                tracing::warn!(key, "rate limit hit");
                Err(ApiError::rate_limited())
            }
        }
    }

    /// Number of keys currently tracked.
    pub fn tracked_keys(&self) -> usize {
        self.limiter.len()
    }

    /// Drop keys whose buckets are full again. Runs at most once a minute.
    fn cleanup_if_due(&self) {
        let Ok(mut last) = self.last_cleanup.try_lock() else {
            return;
        };
        if last.elapsed() < CLEANUP_EVERY {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.limiter.retain_recent();
        self.limiter.shrink_to_fit();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Adds CSP, HSTS (https only), nosniff, referrer policy, permissions policy, COOP.
/// A header a handler already set is left alone.
pub async fn security_headers(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    apply_security_headers(response.headers_mut(), state.config.is_https());
    response
}

/// The header set from section 7, without overriding headers already present.
pub fn apply_security_headers(headers: &mut HeaderMap, https: bool) {
    let mut set = |name: HeaderName, value: &'static str| {
        if !headers.contains_key(&name) {
            headers.insert(name, HeaderValue::from_static(value));
        }
    };
    set(header::CONTENT_SECURITY_POLICY, CSP);
    set(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(header::REFERRER_POLICY, "no-referrer");
    set(
        HeaderName::from_static("permissions-policy"),
        PERMISSIONS_POLICY,
    );
    set(
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    set(header::X_FRAME_OPTIONS, "DENY");
    if https {
        set(header::STRICT_TRANSPORT_SECURITY, HSTS);
    }
}

/// Device-flow endpoints called by the Chrome extension (Origin `chrome-extension://…`)
/// and native apps (no Origin). They never read the session cookie, so a forged
/// cross-site request gains nothing; their own rate limits and secrets protect them.
pub const CSRF_EXEMPT: [&str; 2] = ["/api/pairing/request", "/api/pairing/poll"];

/// True when an unsafe request to `path` must prove it comes from an allowed origin.
pub fn needs_origin_check(method: &Method, path: &str, headers: &HeaderMap) -> bool {
    let unsafe_method = matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    unsafe_method
        && path.starts_with("/api")
        && !CSRF_EXEMPT.contains(&path)
        && !has_bearer(headers)
}

/// Rejects cookie-authenticated unsafe requests whose Origin/Referer is not allowed,
/// or whose body is not JSON, a multipart form or raw bytes (section 7).
///
/// Applies to POST/PUT/PATCH/DELETE under `/api` without an `Authorization: Bearer`
/// header, including public endpoints: setup and sign-in must come from our own origin.
/// Only the cookie-free device pairing endpoints ([`CSRF_EXEMPT`]) are skipped.
/// Requests without a body (logout, pause, delete) need no `Content-Type`.
pub async fn csrf_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if needs_origin_check(request.method(), request.uri().path(), request.headers()) {
        let allowed = state.config.allowed_origins();
        if !origin_allowed(request.headers(), &allowed) {
            tracing::warn!(path = %request.uri().path(), "blocked a cross-site request");
            return Err(ApiError::forbidden("This request came from another site."));
        }
        let has_body = request.body().size_hint().exact() != Some(0);
        if has_body && !body_type_allowed(request.headers()) {
            tracing::debug!(path = %request.uri().path(), "blocked a request body that is not JSON");
            return Err(ApiError::bad_request("Send the request as JSON."));
        }
    }
    Ok(next.run(request).await)
}

/// The body types a cookie-authenticated unsafe request may carry: JSON, or a multipart
/// form or raw bytes for uploads. Each handler still checks the one it accepts. Plain
/// text and URL-encoded forms, which a cross-site HTML form can send, are refused.
pub fn body_type_allowed(headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let essence = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "application/json" | "multipart/form-data" | "application/octet-stream"
    )
}

/// True when the request carries `Authorization: Bearer …`.
pub fn has_bearer(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer "))
}

/// `Origin` (or `Referer` when `Origin` is absent) names one of `allowed`.
pub fn origin_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    let origin = match headers.get(header::ORIGIN) {
        Some(value) => value.to_str().ok().map(|s| s.trim().to_string()),
        None => headers
            .get(header::REFERER)
            .and_then(|v| v.to_str().ok())
            .and_then(|r| url::Url::parse(r.trim()).ok())
            .map(|u| u.origin().ascii_serialization()),
    };
    let Some(origin) = origin else { return false };
    // "null" origins (sandboxed frames, file://) never match: Url::origin of those is opaque.
    let origin = match url::Url::parse(&origin) {
        Ok(url) => url.origin().ascii_serialization(),
        Err(_) => return false,
    };
    origin != "null" && allowed.iter().any(|a| a == &origin)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                HeaderName::from_static(k),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn logged_paths_hide_codes() {
        let code = "Zx9_secretInviteCode123";
        for path in [
            format!("/invite/{code}"),
            format!("/recover/{code}"),
            format!("/api/invites/{code}"),
        ] {
            let logged = loggable_path(&path);
            assert!(!logged.contains(code), "{logged}");
            assert!(logged.ends_with("<redacted>"), "{logged}");
        }
        assert_eq!(loggable_path("/api/items/abc/file"), "/api/items/abc/file");
        assert_eq!(loggable_path("/api/invites"), "/api/invites");
        assert_eq!(loggable_path("/invite/"), "/invite/");
    }

    #[test]
    fn origin_checks() {
        let allowed = vec![
            "https://stash.example.com".to_string(),
            "http://localhost:8788".to_string(),
        ];
        assert!(origin_allowed(
            &headers(&[("origin", "https://stash.example.com")]),
            &allowed
        ));
        assert!(origin_allowed(
            &headers(&[("origin", "http://localhost:8788")]),
            &allowed
        ));
        assert!(!origin_allowed(
            &headers(&[("origin", "https://evil.example.com")]),
            &allowed
        ));
        assert!(!origin_allowed(
            &headers(&[("origin", "https://stash.example.com.evil.net")]),
            &allowed
        ));
        assert!(!origin_allowed(
            &headers(&[("origin", "http://stash.example.com")]),
            &allowed
        ));
        assert!(!origin_allowed(&headers(&[("origin", "null")]), &allowed));
        assert!(!origin_allowed(&headers(&[]), &allowed));
        // Referer only when Origin is absent.
        assert!(origin_allowed(
            &headers(&[("referer", "https://stash.example.com/settings?x=1")]),
            &allowed
        ));
        assert!(!origin_allowed(
            &headers(&[("referer", "https://evil.example.com/stash.example.com")]),
            &allowed
        ));
        assert!(!origin_allowed(
            &headers(&[
                ("origin", "https://evil.example.com"),
                ("referer", "https://stash.example.com/")
            ]),
            &allowed
        ));
        // Nothing allowed when no public URL is configured.
        assert!(!origin_allowed(
            &headers(&[("origin", "https://stash.example.com")]),
            &[]
        ));
    }

    #[test]
    fn which_requests_need_an_origin() {
        let none = HeaderMap::new();
        let bearer = headers(&[("authorization", "Bearer pst_abc")]);
        assert!(needs_origin_check(
            &Method::POST,
            "/api/auth/login/start",
            &none
        ));
        assert!(needs_origin_check(
            &Method::DELETE,
            "/api/me/sessions/x",
            &none
        ));
        assert!(needs_origin_check(&Method::PATCH, "/api/users/x", &none));
        assert!(needs_origin_check(&Method::PUT, "/api/settings", &none));
        assert!(!needs_origin_check(&Method::GET, "/api/me", &none));
        assert!(!needs_origin_check(
            &Method::POST,
            "/api/downloads",
            &bearer
        ));
        assert!(!needs_origin_check(
            &Method::POST,
            "/api/pairing/request",
            &none
        ));
        assert!(!needs_origin_check(
            &Method::POST,
            "/api/pairing/poll",
            &none
        ));
        assert!(needs_origin_check(
            &Method::POST,
            "/api/pairing/approve",
            &none
        ));
    }

    #[test]
    fn bearer_detection() {
        assert!(has_bearer(&headers(&[("authorization", "Bearer pst_abc")])));
        assert!(has_bearer(&headers(&[("authorization", "bearer pst_abc")])));
        assert!(!has_bearer(&headers(&[("authorization", "Basic abc")])));
        assert!(!has_bearer(&headers(&[("authorization", "Bearer ")])));
        assert!(!has_bearer(&headers(&[])));
    }

    #[test]
    fn client_ip_honors_proxy_setting() {
        let peer = Some(IpAddr::V4(Ipv4Addr::new(172, 18, 0, 2)));
        let h = headers(&[("x-forwarded-for", "10.0.0.1, 203.0.113.7")]);
        assert_eq!(client_ip(&h, peer, false), peer);
        assert_eq!(
            client_ip(&h, peer, true),
            Some("203.0.113.7".parse().unwrap())
        );
        assert_eq!(
            client_ip(&headers(&[("x-forwarded-for", "garbage")]), peer, true),
            peer
        );
        assert_eq!(
            client_ip(
                &headers(&[("x-forwarded-for", "[2001:db8::1]:443")]),
                None,
                true
            ),
            Some("2001:db8::1".parse().unwrap())
        );
        assert_eq!(client_ip(&headers(&[]), None, true), None);
    }

    #[test]
    fn client_ip_uses_the_entry_the_proxy_appended() {
        // SWAG, Nginx Proxy Manager, Traefik and Cloudflare append the real address,
        // so everything to its left is whatever the client chose to send.
        let peer = Some(IpAddr::V4(Ipv4Addr::new(172, 18, 0, 2)));
        let spoofed = headers(&[("x-forwarded-for", "6.6.6.6, 203.0.113.7")]);
        assert_eq!(
            client_ip(&spoofed, peer, true),
            Some("203.0.113.7".parse().unwrap())
        );
        // A garbage last entry is not trusted, and an earlier entry is never used instead.
        assert_eq!(
            client_ip(
                &headers(&[("x-forwarded-for", "6.6.6.6, nonsense")]),
                peer,
                true
            ),
            peer
        );
        // IPv4-mapped IPv6 peers (dual-stack listeners) are the IPv4 address.
        let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        assert_eq!(
            client_ip(&HeaderMap::new(), Some(mapped), false),
            Some("192.0.2.1".parse().unwrap())
        );
    }

    #[test]
    fn rate_limit_keys_group_ipv6_networks() {
        let key = |ip: &str| ClientIp(Some(ip.parse().unwrap())).key("auth");
        // One home connection gets a whole /64: it shares one budget.
        assert_eq!(
            key("2001:db8:1:2::1"),
            key("2001:db8:1:2:ffff:ffff:ffff:ffff")
        );
        assert_eq!(key("2001:db8:1:2::1"), "auth:2001:db8:1:2::/64");
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
        assert_eq!(key("192.0.2.1"), "auth:192.0.2.1");
        assert_ne!(key("192.0.2.1"), key("192.0.2.2"));
        assert_eq!(key("::ffff:192.0.2.1"), "auth:192.0.2.1");
        assert_eq!(ClientIp(None).key("auth"), "auth:unknown");
    }

    #[test]
    fn body_content_types() {
        assert!(body_type_allowed(&headers(&[(
            "content-type",
            "application/json"
        )])));
        assert!(body_type_allowed(&headers(&[(
            "content-type",
            "Application/JSON; charset=utf-8"
        )])));
        assert!(body_type_allowed(&headers(&[(
            "content-type",
            "multipart/form-data; boundary=x"
        )])));
        assert!(body_type_allowed(&headers(&[(
            "content-type",
            "application/octet-stream"
        )])));
        // The types a cross-site HTML form can send without a preflight.
        assert!(!body_type_allowed(&headers(&[(
            "content-type",
            "text/plain"
        )])));
        assert!(!body_type_allowed(&headers(&[(
            "content-type",
            "application/x-www-form-urlencoded"
        )])));
        assert!(!body_type_allowed(&headers(&[])));
    }

    #[test]
    fn rate_limiter_allows_burst_then_blocks() {
        let limiter = RateLimiter::new();
        for _ in 0..20 {
            assert!(limiter.check("auth:192.0.2.1").is_ok());
        }
        let err = limiter.check("auth:192.0.2.1").unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::RateLimited);
        // Other keys keep their own budget.
        assert!(limiter.check("auth:192.0.2.2").is_ok());
        assert!(limiter.check("pairing:192.0.2.1").is_ok());
        assert_eq!(limiter.tracked_keys(), 3);
    }

    #[test]
    fn security_header_set() {
        let mut h = HeaderMap::new();
        apply_security_headers(&mut h, false);
        assert_eq!(h[header::CONTENT_SECURITY_POLICY], CSP);
        assert!(CSP.contains("worker-src 'self' blob:"));
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(h[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(h["permissions-policy"], PERMISSIONS_POLICY);
        assert_eq!(h["cross-origin-opener-policy"], "same-origin");
        assert_eq!(h[header::X_FRAME_OPTIONS], "DENY");
        assert!(!h.contains_key(header::STRICT_TRANSPORT_SECURITY));

        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("sandbox"),
        );
        apply_security_headers(&mut h, true);
        assert_eq!(h[header::CONTENT_SECURITY_POLICY], "sandbox");
        assert_eq!(h[header::STRICT_TRANSPORT_SECURITY], HSTS);
    }
}

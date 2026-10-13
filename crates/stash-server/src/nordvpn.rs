//! NordVPN API: service credentials (NordLynx key), countries, recommended servers (section 5).
//!
//! The access token is only ever held in memory for the length of one request and is
//! never logged. Service credentials (private key, username, password) are likewise
//! never logged; their `Debug` output is redacted.

use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use url::Url;

/// Public NordVPN API.
pub const API_BASE: &str = "https://api.nordvpn.com/";
/// NordVPN CDN that serves per-server `.ovpn` files.
pub const DOWNLOADS_BASE: &str = "https://downloads.nordcdn.com/";
/// NordVPN's DNS servers, used in generated WireGuard files.
pub const NORDVPN_DNS: [&str; 2] = ["103.86.96.100", "103.86.99.100"];
/// Interface address NordLynx assigns to every client.
pub const NORDLYNX_ADDRESS: &str = "10.5.0.2/32";
pub const NORDLYNX_PORT: u16 = 51820;

const COUNTRIES_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Responses larger than this are treated as broken.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum NordVpnError {
    #[error(
        "NordVPN did not accept that access token. Create a new one in your Nord Account and try again."
    )]
    InvalidToken,
    #[error(
        "PopcornStash could not reach NordVPN. Check this server's internet connection and try again."
    )]
    Unreachable,
    #[error(
        "NordVPN has no recommended server for that choice right now. Pick another country and try again."
    )]
    NoServer,
    /// `detail` is for logs only and never contains secrets.
    #[error("NordVPN sent an answer PopcornStash could not use. Try again in a few minutes.")]
    Unexpected { detail: String },
}

impl NordVpnError {
    fn unexpected(detail: impl Into<String>) -> Self {
        Self::Unexpected {
            detail: detail.into(),
        }
    }
}

/// Server technology filter for recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NordTechnology {
    WireguardUdp,
    OpenvpnUdp,
}

impl NordTechnology {
    pub fn identifier(self) -> &'static str {
        match self {
            Self::WireguardUdp => "wireguard_udp",
            Self::OpenvpnUdp => "openvpn_udp",
        }
    }
}

/// Service credentials returned for an access token.
#[derive(Clone, Deserialize)]
pub struct NordCredentials {
    /// NordLynx (WireGuard) private key, base64.
    #[serde(default)]
    pub nordlynx_private_key: String,
    /// OpenVPN service username.
    #[serde(default)]
    pub username: String,
    /// OpenVPN service password.
    #[serde(default)]
    pub password: String,
}

impl std::fmt::Debug for NordCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NordCredentials")
            .field("nordlynx_private_key", &"<redacted>")
            .field("username", &"<redacted>")
            .field("password", &"<redacted>")
            .finish()
    }
}

/// One entry of `GET /api/vpn/nordvpn/countries`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NordCountry {
    pub id: u64,
    pub name: String,
    pub code: String,
}

/// A recommended server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NordServer {
    /// e.g. `nl812.nordvpn.com`.
    pub hostname: String,
    /// Public IP address of the server.
    pub station: String,
    /// e.g. `Netherlands #812`.
    pub name: String,
    pub country: Option<String>,
    /// WireGuard public key (only for `wireguard_udp`).
    pub public_key: Option<String>,
}

impl NordServer {
    /// `nl812` for `nl812.nordvpn.com`.
    pub fn short_name(&self) -> &str {
        self.hostname.split('.').next().unwrap_or(&self.hostname)
    }

    /// Friendly label, e.g. "NordVPN · Netherlands · nl812".
    pub fn label(&self) -> String {
        match &self.country {
            Some(country) => format!("NordVPN · {country} · {}", self.short_name()),
            None => format!("NordVPN · {}", self.short_name()),
        }
    }
}

// --- Raw API shapes ---------------------------------------------------------

#[derive(Deserialize)]
struct RawCountry {
    id: u64,
    name: String,
    code: String,
}

#[derive(Deserialize)]
struct RawServer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    station: String,
    #[serde(default)]
    locations: Vec<RawLocation>,
    #[serde(default)]
    technologies: Vec<RawTechnology>,
}

#[derive(Deserialize)]
struct RawLocation {
    country: Option<RawLocationCountry>,
}

#[derive(Deserialize)]
struct RawLocationCountry {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawTechnology {
    identifier: Option<String>,
    #[serde(default)]
    metadata: Vec<RawMetadata>,
}

#[derive(Deserialize)]
struct RawMetadata {
    name: Option<String>,
    value: Option<serde_json::Value>,
}

/// Parse `GET /v1/servers/countries`, sorted by name.
pub fn parse_countries(body: &[u8]) -> Result<Vec<NordCountry>, NordVpnError> {
    let raw: Vec<RawCountry> = serde_json::from_slice(body)
        .map_err(|e| NordVpnError::unexpected(format!("countries response: {e}")))?;
    let mut countries: Vec<NordCountry> = raw
        .into_iter()
        .filter(|c| !c.name.trim().is_empty())
        .map(|c| NordCountry {
            id: c.id,
            name: c.name.trim().to_string(),
            code: c.code.trim().to_ascii_uppercase(),
        })
        .collect();
    countries.sort_by_key(|c| c.name.to_lowercase());
    Ok(countries)
}

/// Parse `GET /v1/servers/recommendations` and take the first usable server.
pub fn parse_recommendation(
    body: &[u8],
    technology: NordTechnology,
) -> Result<NordServer, NordVpnError> {
    let raw: Vec<RawServer> = serde_json::from_slice(body)
        .map_err(|e| NordVpnError::unexpected(format!("recommendations response: {e}")))?;
    let server = raw.into_iter().next().ok_or(NordVpnError::NoServer)?;
    let hostname = server.hostname.trim().to_ascii_lowercase();
    if !is_hostname(&hostname) {
        return Err(NordVpnError::unexpected(
            "recommended server has no valid hostname",
        ));
    }
    let station = server.station.trim().to_string();
    if station.parse::<std::net::IpAddr>().is_err() && !is_hostname(&station) {
        return Err(NordVpnError::unexpected(
            "recommended server has no valid station address",
        ));
    }
    let public_key = server
        .technologies
        .iter()
        .filter(|t| t.identifier.as_deref() == Some(NordTechnology::WireguardUdp.identifier()))
        .flat_map(|t| t.metadata.iter())
        .find(|m| m.name.as_deref() == Some("public_key"))
        .and_then(|m| m.value.as_ref())
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string());
    if technology == NordTechnology::WireguardUdp {
        match &public_key {
            Some(key) if is_wireguard_key(key) => {}
            _ => {
                return Err(NordVpnError::unexpected(
                    "recommended server has no valid WireGuard public key",
                ));
            }
        }
    }
    // Names end up in the label, a comment line in wireguard.conf: no control characters.
    let country = server
        .locations
        .into_iter()
        .filter_map(|l| l.country.and_then(|c| c.name))
        .map(|n| display_text(&n))
        .find(|n| !n.is_empty());
    let name = match display_text(&server.name) {
        name if name.is_empty() => hostname.clone(),
        name => name,
    };
    Ok(NordServer {
        hostname,
        station,
        name,
        country,
        public_key,
    })
}

/// API text for labels: control characters dropped, whitespace collapsed, at most 80 chars.
fn display_text(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

/// Parse `GET /v1/users/services/credentials`. Never includes the body in errors.
pub fn parse_credentials(body: &[u8]) -> Result<NordCredentials, NordVpnError> {
    let creds: NordCredentials = serde_json::from_slice(body)
        .map_err(|_| NordVpnError::unexpected("credentials response was not the expected JSON"))?;
    Ok(NordCredentials {
        nordlynx_private_key: creds.nordlynx_private_key.trim().to_string(),
        username: creds.username.trim().to_string(),
        password: creds.password.trim().to_string(),
    })
}

/// A wg-quick file for a NordLynx server (section 5).
pub fn build_wireguard_conf(
    private_key: &str,
    server: &NordServer,
) -> Result<String, NordVpnError> {
    let private_key = private_key.trim();
    if !is_wireguard_key(private_key) {
        return Err(NordVpnError::unexpected(
            "NordLynx private key is not a WireGuard key",
        ));
    }
    let public_key = server
        .public_key
        .as_deref()
        .map(str::trim)
        .filter(|k| is_wireguard_key(k))
        .ok_or_else(|| NordVpnError::unexpected("server has no WireGuard public key"))?;
    let station = server.station.trim();
    let endpoint_host = match station.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(v6)) => format!("[{v6}]"),
        Ok(std::net::IpAddr::V4(v4)) => v4.to_string(),
        Err(_) if is_hostname(station) => station.to_string(),
        Err(_) => {
            return Err(NordVpnError::unexpected(
                "server has no valid station address",
            ));
        }
    };
    Ok(format!(
        "# {label}\n\
         [Interface]\n\
         PrivateKey = {private_key}\n\
         Address = {address}\n\
         DNS = {dns}\n\
         \n\
         [Peer]\n\
         PublicKey = {public_key}\n\
         AllowedIPs = 0.0.0.0/0\n\
         Endpoint = {endpoint_host}:{port}\n\
         PersistentKeepalive = 25\n",
        label = server.label(),
        address = NORDLYNX_ADDRESS,
        dns = NORDVPN_DNS.join(", "),
        port = NORDLYNX_PORT,
    ))
}

/// True for a base64 string that decodes to exactly 32 bytes.
pub fn is_wireguard_key(key: &str) -> bool {
    key.len() == 44
        && base64::engine::general_purpose::STANDARD
            .decode(key)
            .is_ok_and(|bytes| bytes.len() == 32)
}

/// Lowercase-insensitive DNS name check: labels of letters, digits and hyphens.
pub fn is_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

struct CachedCountries {
    at: Instant,
    countries: Vec<NordCountry>,
}

/// Client for the NordVPN public API. Base URLs are injectable for tests.
pub struct NordVpnClient {
    http: reqwest::Client,
    api_base: Url,
    downloads_base: Url,
    countries: tokio::sync::Mutex<Option<CachedCountries>>,
}

impl NordVpnClient {
    /// Client against the real NordVPN endpoints.
    pub fn new(http: reqwest::Client) -> Self {
        let api = Url::parse(API_BASE).expect("static url");
        let downloads = Url::parse(DOWNLOADS_BASE).expect("static url");
        Self::with_base_urls(http, api, downloads)
    }

    /// Client against other base URLs (tests, mirrors).
    pub fn with_base_urls(http: reqwest::Client, api_base: Url, downloads_base: Url) -> Self {
        Self {
            http,
            api_base: with_trailing_slash(api_base),
            downloads_base: with_trailing_slash(downloads_base),
            countries: tokio::sync::Mutex::new(None),
        }
    }

    /// Exchange an access token for service credentials. The token is sent as the
    /// basic-auth password for user `token` and is not kept.
    pub async fn credentials(&self, access_token: &str) -> Result<NordCredentials, NordVpnError> {
        let url = self.api_url("v1/users/services/credentials")?;
        let response = self
            .http
            .get(url)
            .basic_auth("token", Some(access_token))
            .send()
            .await
            .map_err(|e| send_error("credentials", &e))?;
        match response.status().as_u16() {
            200..=299 => {}
            // 400 is NordVPN's answer to a token that is not in its format ("Invalid
            // authorization header"), for example a pasted password or a cut-off token.
            400 | 401 | 403 => return Err(NordVpnError::InvalidToken),
            status => {
                return Err(NordVpnError::unexpected(format!(
                    "credentials answered HTTP {status}"
                )));
            }
        }
        let body = read_body(response, "credentials").await?;
        let creds = parse_credentials(&body)?;
        if creds.nordlynx_private_key.is_empty() && creds.username.is_empty() {
            return Err(NordVpnError::unexpected("credentials response was empty"));
        }
        Ok(creds)
    }

    /// Countries with NordVPN servers, sorted by name. Cached for 24 hours; a stale
    /// list is served when NordVPN cannot be reached.
    pub async fn countries(&self) -> Result<Vec<NordCountry>, NordVpnError> {
        let mut cache = self.countries.lock().await;
        if let Some(cached) = cache.as_ref()
            && cached.at.elapsed() < COUNTRIES_TTL
        {
            return Ok(cached.countries.clone());
        }
        match self.fetch_countries().await {
            Ok(countries) => {
                *cache = Some(CachedCountries {
                    at: Instant::now(),
                    countries: countries.clone(),
                });
                Ok(countries)
            }
            Err(e) => match cache.as_ref() {
                Some(stale) => {
                    tracing::warn!(error = %e, "NordVPN country list refresh failed; using the cached list");
                    Ok(stale.countries.clone())
                }
                None => Err(e),
            },
        }
    }

    async fn fetch_countries(&self) -> Result<Vec<NordCountry>, NordVpnError> {
        let url = self.api_url("v1/servers/countries")?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| send_error("countries", &e))?;
        if !response.status().is_success() {
            return Err(NordVpnError::unexpected(format!(
                "countries answered HTTP {}",
                response.status().as_u16()
            )));
        }
        let body = read_body(response, "countries").await?;
        let countries = parse_countries(&body)?;
        if countries.is_empty() {
            return Err(NordVpnError::unexpected("country list was empty"));
        }
        Ok(countries)
    }

    /// The server NordVPN recommends for this technology, optionally within a country.
    pub async fn recommended_server(
        &self,
        country_id: Option<u64>,
        technology: NordTechnology,
    ) -> Result<NordServer, NordVpnError> {
        let mut url = self.api_url("v1/servers/recommendations")?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair(
                "filters[servers_technologies][identifier]",
                technology.identifier(),
            );
            if let Some(id) = country_id {
                query.append_pair("filters[country_id]", &id.to_string());
            }
            query.append_pair("limit", "1");
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| send_error("recommendations", &e))?;
        if !response.status().is_success() {
            return Err(NordVpnError::unexpected(format!(
                "recommendations answered HTTP {}",
                response.status().as_u16()
            )));
        }
        let body = read_body(response, "recommendations").await?;
        parse_recommendation(&body, technology)
    }

    /// The `.ovpn` file NordVPN publishes for one server (UDP).
    pub async fn openvpn_config(&self, hostname: &str) -> Result<String, NordVpnError> {
        let hostname = hostname.trim().to_ascii_lowercase();
        if !is_hostname(&hostname) {
            return Err(NordVpnError::unexpected("server hostname is not valid"));
        }
        let path = format!("configs/files/ovpn_udp/servers/{hostname}.udp.ovpn");
        let url = self
            .downloads_base
            .join(&path)
            .map_err(|e| NordVpnError::unexpected(format!("openvpn config url: {e}")))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| send_error("openvpn config", &e))?;
        if !response.status().is_success() {
            return Err(NordVpnError::unexpected(format!(
                "openvpn config answered HTTP {}",
                response.status().as_u16()
            )));
        }
        let body = read_body(response, "openvpn config").await?;
        String::from_utf8(body).map_err(|_| NordVpnError::unexpected("openvpn config is not text"))
    }

    fn api_url(&self, path: &str) -> Result<Url, NordVpnError> {
        self.api_base
            .join(path)
            .map_err(|e| NordVpnError::unexpected(format!("api url: {e}")))
    }
}

fn with_trailing_slash(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

fn send_error(what: &str, error: &reqwest::Error) -> NordVpnError {
    // reqwest errors carry the URL but never the basic-auth header, so they are safe to log.
    tracing::warn!(request = what, error = %error, "NordVPN request failed");
    NordVpnError::Unreachable
}

async fn read_body(response: reqwest::Response, what: &str) -> Result<Vec<u8>, NordVpnError> {
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err(NordVpnError::unexpected(format!(
            "{what} response is too large"
        )));
    }
    let mut response = response;
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(NordVpnError::unexpected(format!(
                        "{what} response is too large"
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(e) => return Err(send_error(what, &e)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const PRIVATE_KEY: &str = "UL1kK5Yvz2nPdEPsY0uqRLUIu4PdKPXhUcxhkFxpVF0=";
    pub const SERVER_KEY: &str = "SyzWv/OB1omrgW0JCwqBZcZ0QIdr4zU/LaeMiB/V3XE=";

    pub fn countries_fixture() -> String {
        serde_json::json!([
            {"id": 153, "name": "Netherlands", "code": "NL", "serverCount": 260,
             "cities": [{"id": 6076868, "name": "Amsterdam", "latitude": 52.35, "longitude": 4.916667,
                         "dns_name": "amsterdam", "serverCount": 260}]},
            {"id": 2, "name": "Albania", "code": "AL", "serverCount": 6,
             "cities": [{"id": 308615, "name": "Tirana", "latitude": 41.3275, "longitude": 19.818889,
                         "dns_name": "tirana", "serverCount": 6}]},
            {"id": 228, "name": "United States", "code": "US", "serverCount": 1970, "cities": []},
            {"id": 81, "name": "germany", "code": "de", "serverCount": 300, "cities": []}
        ])
        .to_string()
    }

    pub fn recommendation_fixture() -> String {
        serde_json::json!([{
            "id": 929912,
            "created_at": "2018-12-19 10:15:42",
            "name": "Netherlands #812",
            "station": "185.153.177.12",
            "ipv6_station": "",
            "hostname": "nl812.nordvpn.com",
            "load": 14,
            "status": "online",
            "locations": [{
                "id": 133,
                "created_at": "2017-06-15 14:06:47",
                "updated_at": "2017-06-15 14:06:47",
                "latitude": 52.35,
                "longitude": 4.916667,
                "country": {"id": 153, "name": "Netherlands", "code": "NL",
                            "city": {"id": 6076868, "name": "Amsterdam", "latitude": 52.35,
                                     "longitude": 4.916667, "dns_name": "amsterdam", "hub_score": 0}}
            }],
            "services": [{"id": 1, "name": "VPN", "identifier": "vpn"}],
            "technologies": [
                {"id": 3, "name": "OpenVPN UDP", "identifier": "openvpn_udp", "metadata": [],
                 "pivot": {"technology_id": 3, "server_id": 929912, "status": "online"}},
                {"id": 35, "name": "Wireguard", "identifier": "wireguard_udp",
                 "metadata": [{"name": "public_key", "value": SERVER_KEY}],
                 "pivot": {"technology_id": 35, "server_id": 929912, "status": "online"}}
            ],
            "groups": [{"id": 11, "title": "Standard VPN servers", "identifier": "legacy_standard"}],
            "specifications": [],
            "ips": [{"id": 1, "ip": {"ip": "185.153.177.12", "version": 4}, "type": "entry"}]
        }])
        .to_string()
    }

    pub fn credentials_fixture() -> String {
        serde_json::json!({
            "id": 123456789,
            "created_at": "2024-01-01 10:00:00",
            "updated_at": "2024-01-01 10:00:00",
            "username": "svcUser123",
            "password": "svcPass456",
            "nordlynx_private_key": PRIVATE_KEY
        })
        .to_string()
    }

    #[test]
    fn countries_parse_and_sort() {
        let countries = parse_countries(countries_fixture().as_bytes()).unwrap();
        let names: Vec<_> = countries.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Albania", "germany", "Netherlands", "United States"]
        );
        assert_eq!(countries[1].code, "DE");
        assert_eq!(
            countries[2],
            NordCountry {
                id: 153,
                name: "Netherlands".into(),
                code: "NL".into()
            }
        );
        assert!(parse_countries(b"{\"error\":1}").is_err());
    }

    #[test]
    fn recommendation_parse() {
        let server = parse_recommendation(
            recommendation_fixture().as_bytes(),
            NordTechnology::WireguardUdp,
        )
        .unwrap();
        assert_eq!(server.hostname, "nl812.nordvpn.com");
        assert_eq!(server.station, "185.153.177.12");
        assert_eq!(server.name, "Netherlands #812");
        assert_eq!(server.country.as_deref(), Some("Netherlands"));
        assert_eq!(server.public_key.as_deref(), Some(SERVER_KEY));
        assert_eq!(server.label(), "NordVPN · Netherlands · nl812");
        assert!(matches!(
            parse_recommendation(b"[]", NordTechnology::OpenvpnUdp),
            Err(NordVpnError::NoServer)
        ));
    }

    #[test]
    fn recommendation_without_wireguard_key() {
        let mut value: serde_json::Value = serde_json::from_str(&recommendation_fixture()).unwrap();
        value[0]["technologies"] = serde_json::json!([]);
        let body = value.to_string();
        assert!(parse_recommendation(body.as_bytes(), NordTechnology::WireguardUdp).is_err());
        let server = parse_recommendation(body.as_bytes(), NordTechnology::OpenvpnUdp).unwrap();
        assert!(server.public_key.is_none());
    }

    #[test]
    fn recommendation_rejects_injected_hostname() {
        let mut value: serde_json::Value = serde_json::from_str(&recommendation_fixture()).unwrap();
        value[0]["hostname"] = serde_json::json!("nl812.nordvpn.com/../../evil");
        assert!(
            parse_recommendation(value.to_string().as_bytes(), NordTechnology::OpenvpnUdp).is_err()
        );
        let mut value: serde_json::Value = serde_json::from_str(&recommendation_fixture()).unwrap();
        value[0]["station"] = serde_json::json!("1.2.3.4\nPostUp = rm -rf /");
        assert!(
            parse_recommendation(value.to_string().as_bytes(), NordTechnology::WireguardUdp)
                .is_err()
        );
    }

    /// The country name ends up in the label, which is a comment line in wireguard.conf.
    #[test]
    fn recommendation_country_cannot_add_lines() {
        let mut value: serde_json::Value = serde_json::from_str(&recommendation_fixture()).unwrap();
        value[0]["locations"][0]["country"]["name"] =
            serde_json::json!("Netherlands\n[Interface]\nPostUp = id\r");
        let server =
            parse_recommendation(value.to_string().as_bytes(), NordTechnology::WireguardUdp)
                .unwrap();
        assert!(!server.label().chars().any(char::is_control));
        let conf = build_wireguard_conf(PRIVATE_KEY, &server).unwrap();
        assert_eq!(conf.matches('\n').count(), 11, "{conf}");
        assert!(crate::api::vpn::validate_wireguard(&conf).is_ok(), "{conf}");
    }

    #[test]
    fn credentials_parse_and_redact() {
        let creds = parse_credentials(credentials_fixture().as_bytes()).unwrap();
        assert_eq!(creds.nordlynx_private_key, PRIVATE_KEY);
        assert_eq!(creds.username, "svcUser123");
        assert_eq!(creds.password, "svcPass456");
        let debug = format!("{creds:?}");
        assert!(
            !debug.contains(PRIVATE_KEY)
                && !debug.contains("svcPass456")
                && !debug.contains("svcUser123")
        );
        match parse_credentials(b"{\"nordlynx_private_key\": 5, \"password\": \"leak-me\"}") {
            Err(e) => assert!(!format!("{e:?}").contains("leak-me")),
            Ok(_) => panic!("wrong type must fail"),
        }
    }

    #[test]
    fn wireguard_conf_shape() {
        let server = parse_recommendation(
            recommendation_fixture().as_bytes(),
            NordTechnology::WireguardUdp,
        )
        .unwrap();
        let conf = build_wireguard_conf(PRIVATE_KEY, &server).unwrap();
        assert!(
            conf.contains(
                "[Interface]\nPrivateKey = UL1kK5Yvz2nPdEPsY0uqRLUIu4PdKPXhUcxhkFxpVF0=\n"
            )
        );
        assert!(conf.contains("Address = 10.5.0.2/32\n"));
        assert!(conf.contains("DNS = 103.86.96.100, 103.86.99.100\n"));
        assert!(conf.contains(&format!("[Peer]\nPublicKey = {SERVER_KEY}\n")));
        assert!(conf.contains("AllowedIPs = 0.0.0.0/0\n"));
        assert!(conf.contains("Endpoint = 185.153.177.12:51820\n"));
        assert!(conf.contains("PersistentKeepalive = 25\n"));
        assert!(build_wireguard_conf("not a key", &server).is_err());
        assert!(build_wireguard_conf(&format!("{PRIVATE_KEY}\nPostUp = id"), &server).is_err());
    }

    #[test]
    fn key_and_host_checks() {
        assert!(is_wireguard_key(PRIVATE_KEY));
        assert!(!is_wireguard_key("AAAA"));
        assert!(is_hostname("nl812.nordvpn.com"));
        assert!(!is_hostname("nl812.nordvpn.com/x"));
        assert!(!is_hostname(""));
        assert!(!is_hostname("-a.com"));
    }

    #[tokio::test]
    async fn token_rejected_maps_to_friendly_message() {
        use axum::http::StatusCode;
        use axum::routing::get;
        // NordVPN answers 401 for an unknown token and 400 ("Invalid authorization
        // header") for one that is not in its format.
        for status in [StatusCode::UNAUTHORIZED, StatusCode::BAD_REQUEST] {
            let app = axum::Router::new().route(
                "/v1/users/services/credentials",
                get(move || async move { (status, "{}") }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let base = Url::parse(&format!("http://{addr}/")).unwrap();
            let client = NordVpnClient::with_base_urls(
                reqwest::Client::builder().no_proxy().build().unwrap(),
                base.clone(),
                base,
            );
            let err = client.credentials("bad-token").await.unwrap_err();
            assert!(matches!(err, NordVpnError::InvalidToken), "{status}: {err}");
            assert_eq!(
                err.to_string(),
                "NordVPN did not accept that access token. Create a new one in your Nord Account and try again."
            );
        }
    }

    #[tokio::test]
    async fn countries_are_cached() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new().route(
            "/v1/servers/countries",
            axum::routing::get(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    countries_fixture()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let base = Url::parse(&format!("http://{addr}")).unwrap();
        let client = NordVpnClient::with_base_urls(
            reqwest::Client::builder().no_proxy().build().unwrap(),
            base.clone(),
            base,
        );
        assert_eq!(client.countries().await.unwrap().len(), 4);
        assert_eq!(client.countries().await.unwrap()[0].name, "Albania");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}

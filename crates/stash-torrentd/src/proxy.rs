//! HTTP proxy on 127.0.0.1:9092 for web downloads through the VPN (section 9).
//!
//! Runs inside the engine process, so every outbound connection belongs to the
//! engine user (policy routing + kill switch) and is additionally bound to the
//! tunnel interface. Supports `CONNECT host:443` and absolute-form `http://`
//! requests, only to ports 80 and 443, never to private, loopback, link-local
//! or ULA addresses (checked after DNS resolution), and answers 503 while the
//! tunnel is not usable (the engine is blocked by the VPN state).

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use ipnet::IpNet;
use socket2::{Domain, Socket, Type};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio_util::sync::CancellationToken;

use crate::sys::constant_time_eq;

type ProxyBody = BoxBody<Bytes, hyper::Error>;

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Internal token; clients send `Proxy-Authorization: Basic base64("stash:<token>")`.
    pub token: String,
    /// Interface outbound sockets bind to (the tunnel). None only without a VPN.
    pub bind_device: Option<String>,
    /// Destination ports allowed (80 and 443).
    pub allowed_ports: Vec<u16>,
    /// Networks allowed despite being private (tests only; empty in production).
    pub allow_nets: Vec<IpNet>,
    pub connect_timeout: Duration,
    pub max_connections: usize,
}

impl ProxyConfig {
    pub fn new(token: String, bind_device: Option<String>) -> Self {
        Self {
            token,
            bind_device,
            allowed_ports: vec![80, 443],
            allow_nets: Vec::new(),
            connect_timeout: Duration::from_secs(15),
            max_connections: 256,
        }
    }
}

/// True for addresses on the public internet.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            let s = v6.segments();
            let nat64 = s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0];
            if nat64 {
                let o = v6.octets();
                return is_public_v4(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // ULA fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // site-local (deprecated)
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || s[0..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible / reserved
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || o[0] >= 240
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0/24
        || (o[0] == 198 && (o[1] & 0xfe) == 18)) // benchmarking 198.18/15
}

fn allowed_ip(ip: IpAddr, config: &ProxyConfig) -> bool {
    is_public(ip) || config.allow_nets.iter().any(|n| n.contains(&ip))
}

/// Check `Proxy-Authorization`.
pub fn proxy_auth_ok(headers: &HeaderMap, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let Some(value) = headers
        .get(header::PROXY_AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(encoded) = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))
    else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let expected = format!("stash:{token}");
    constant_time_eq(&decoded, expected.as_bytes())
}

fn empty() -> ProxyBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn text(status: StatusCode, message: &'static str) -> Response<ProxyBody> {
    let mut response = Response::new(
        Full::new(Bytes::from_static(message.as_bytes()))
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[derive(Debug)]
enum TargetError {
    Port,
    Forbidden,
    Resolve,
    Connect,
    Timeout,
}

impl TargetError {
    fn response(&self) -> Response<ProxyBody> {
        match self {
            TargetError::Port => text(
                StatusCode::FORBIDDEN,
                "Only ports 80 and 443 are allowed.\n",
            ),
            TargetError::Forbidden => text(
                StatusCode::FORBIDDEN,
                "That address is not on the public internet.\n",
            ),
            TargetError::Resolve => text(
                StatusCode::BAD_GATEWAY,
                "The host name could not be resolved.\n",
            ),
            TargetError::Connect => text(
                StatusCode::BAD_GATEWAY,
                "The server could not be reached.\n",
            ),
            TargetError::Timeout => text(
                StatusCode::GATEWAY_TIMEOUT,
                "The server did not answer in time.\n",
            ),
        }
    }
}

async fn connect_socket(addr: SocketAddr, config: &ProxyConfig) -> std::io::Result<TcpStream> {
    let socket = Socket::new(
        Domain::for_address(addr),
        Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_nonblocking(true)?;
    socket.set_cloexec(true)?;
    if let Some(device) = &config.bind_device {
        socket.bind_device(Some(device.as_bytes()))?;
    }
    TcpSocket::from_std_stream(socket.into())
        .connect(addr)
        .await
}

/// Resolve, filter and connect to `host:port`.
async fn connect_target(
    host: &str,
    port: u16,
    config: &ProxyConfig,
) -> Result<TcpStream, TargetError> {
    if !config.allowed_ports.contains(&port) {
        return Err(TargetError::Port);
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::time::timeout(
            config.connect_timeout,
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| TargetError::Timeout)?
        .map_err(|_| TargetError::Resolve)?
        .collect(),
    };
    if addrs.is_empty() {
        return Err(TargetError::Resolve);
    }
    // Every resolved address must be public: a name that also resolves to a
    // private address is refused outright (DNS rebinding).
    if addrs.iter().any(|a| !allowed_ip(a.ip(), config)) {
        return Err(TargetError::Forbidden);
    }
    let mut last = TargetError::Connect;
    for addr in addrs {
        match tokio::time::timeout(config.connect_timeout, connect_socket(addr, config)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => {
                tracing::debug!(%addr, error = %e, "proxy connect failed");
                last = TargetError::Connect;
            }
            Err(_) => last = TargetError::Timeout,
        }
    }
    Err(last)
}

const HOP_BY_HOP: [&str; 9] = [
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let listed: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_ascii_lowercase()))
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok())
        .collect();
    for name in listed {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(name);
    }
}

async fn handle(
    request: Request<Incoming>,
    config: Arc<ProxyConfig>,
    running: watch::Receiver<bool>,
) -> Result<Response<ProxyBody>, Infallible> {
    if !proxy_auth_ok(request.headers(), &config.token) {
        let mut response = text(
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "Proxy authentication required.\n",
        );
        response.headers_mut().insert(
            header::PROXY_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"PopcornStash\""),
        );
        return Ok(response);
    }
    if !*running.borrow() {
        return Ok(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "The VPN is not connected.\n",
        ));
    }
    if request.method() == Method::CONNECT {
        return Ok(connect(request, &config).await);
    }
    Ok(forward(request, &config).await)
}

async fn connect(request: Request<Incoming>, config: &ProxyConfig) -> Response<ProxyBody> {
    let Some(authority) = request.uri().authority().cloned() else {
        return text(StatusCode::BAD_REQUEST, "CONNECT needs host:port.\n");
    };
    let Some(port) = authority.port_u16() else {
        return text(StatusCode::BAD_REQUEST, "CONNECT needs host:port.\n");
    };
    let mut upstream = match connect_target(authority.host(), port, config).await {
        Ok(stream) => stream,
        Err(e) => return e.response(),
    };
    tokio::spawn(async move {
        match hyper::upgrade::on(request).await {
            Ok(upgraded) => {
                let mut client = TokioIo::new(upgraded);
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            }
            Err(e) => tracing::debug!(error = %e, "CONNECT upgrade failed"),
        }
    });
    Response::new(empty())
}

async fn forward(request: Request<Incoming>, config: &ProxyConfig) -> Response<ProxyBody> {
    let uri = request.uri().clone();
    if uri.scheme_str() != Some("http") {
        return text(
            StatusCode::BAD_REQUEST,
            "Use CONNECT for https, or an absolute http:// URL.\n",
        );
    }
    let Some(authority) = uri.authority().cloned() else {
        return text(StatusCode::BAD_REQUEST, "The URL has no host.\n");
    };
    let port = authority.port_u16().unwrap_or(80);
    let stream = match connect_target(authority.host(), port, config).await {
        Ok(stream) => stream,
        Err(e) => return e.response(),
    };
    let (mut sender, connection) =
        match hyper::client::conn::http1::handshake(TokioIo::new(stream)).await {
            Ok(pair) => pair,
            Err(_) => {
                return text(
                    StatusCode::BAD_GATEWAY,
                    "The server could not be reached.\n",
                );
            }
        };
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let (mut parts, body) = request.into_parts();
    let origin_form = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    parts.uri = match origin_form.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return text(StatusCode::BAD_REQUEST, "The URL is not valid.\n"),
    };
    strip_hop_by_hop(&mut parts.headers);
    if let Ok(host) = HeaderValue::from_str(authority.as_str()) {
        parts.headers.insert(header::HOST, host);
    }
    parts.version = hyper::Version::HTTP_11;
    match sender.send_request(Request::from_parts(parts, body)).await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop(&mut parts.headers);
            Response::from_parts(parts, body.boxed())
        }
        Err(_) => text(
            StatusCode::BAD_GATEWAY,
            "The server closed the connection.\n",
        ),
    }
}

/// Serve the proxy until `cancel`. `running` is false while the tunnel is not usable.
pub async fn serve(
    listener: TcpListener,
    config: Arc<ProxyConfig>,
    running: watch::Receiver<bool>,
    cancel: CancellationToken,
) {
    let limit = Arc::new(Semaphore::new(config.max_connections));
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => return,
            r = listener.accept() => r,
        };
        let Ok((stream, _)) = accepted else { continue };
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            continue;
        };
        let config = config.clone();
        let running = running.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service =
                service_fn(move |request| handle(request, config.clone(), running.clone()));
            let connection = hyper::server::conn::http1::Builder::new()
                .preserve_header_case(true)
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades();
            if let Err(e) = connection.await {
                tracing::debug!(error = %e, "proxy connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    const TOKEN: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";

    /// Tiny HTTP server that echoes the request head it received.
    async fn echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut head = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        if line == "\r\n" {
                            break;
                        }
                        head.push_str(&line);
                    }
                    let body = format!("echo\n{head}");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    reader
                        .get_mut()
                        .write_all(response.as_bytes())
                        .await
                        .unwrap();
                });
            }
        });
        addr
    }

    async fn start_proxy(
        config: ProxyConfig,
        running: bool,
    ) -> (SocketAddr, watch::Sender<bool>, CancellationToken) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = watch::channel(running);
        let cancel = CancellationToken::new();
        tokio::spawn(serve(listener, Arc::new(config), rx, cancel.clone()));
        (addr, tx, cancel)
    }

    fn auth() -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("stash:{TOKEN}"))
        )
    }

    async fn raw(proxy: SocketAddr, request: String) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(proxy).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        (stream, String::from_utf8_lossy(&buf[..n]).into_owned())
    }

    fn test_config(upstream: SocketAddr) -> ProxyConfig {
        let mut config = ProxyConfig::new(TOKEN.into(), None);
        config.allowed_ports = vec![upstream.port()];
        config.allow_nets = vec!["127.0.0.0/8".parse().unwrap()];
        config
    }

    #[tokio::test]
    async fn requires_proxy_auth() {
        let upstream = echo_server().await;
        let (proxy, _tx, cancel) = start_proxy(test_config(upstream), true).await;
        let (_, head) = raw(
            proxy,
            format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\n\r\n"),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 407"), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("proxy-authenticate: basic")
        );
        let wrong = base64::engine::general_purpose::STANDARD.encode("stash:nope");
        let (_, head) = raw(proxy, format!("GET http://{upstream}/ HTTP/1.1\r\nHost: {upstream}\r\nProxy-Authorization: Basic {wrong}\r\n\r\n")).await;
        assert!(head.starts_with("HTTP/1.1 407"), "{head}");
        cancel.cancel();
    }

    #[tokio::test]
    async fn connect_tunnels_bytes() {
        let upstream = echo_server().await;
        let (proxy, _tx, cancel) = start_proxy(test_config(upstream), true).await;
        let (mut stream, head) = raw(proxy, format!("CONNECT {upstream} HTTP/1.1\r\nHost: {upstream}\r\nProxy-Authorization: {}\r\n\r\n", auth())).await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        stream
            .write_all(b"GET /inside HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut body = String::new();
        stream.read_to_string(&mut body).await.unwrap();
        assert!(body.contains("echo\nGET /inside HTTP/1.1"), "{body}");
        cancel.cancel();
    }

    #[tokio::test]
    async fn forwards_absolute_form_without_proxy_headers() {
        let upstream = echo_server().await;
        let (proxy, _tx, cancel) = start_proxy(test_config(upstream), true).await;
        let client = reqwest::Client::builder()
            .proxy(
                reqwest::Proxy::http(format!("http://{proxy}"))
                    .unwrap()
                    .basic_auth("stash", TOKEN),
            )
            .build()
            .unwrap();
        let body = client
            .get(format!("http://{upstream}/file.bin?x=1"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            body.starts_with("echo\nGET /file.bin?x=1 HTTP/1.1"),
            "{body}"
        );
        assert!(
            !body.to_ascii_lowercase().contains("proxy-authorization"),
            "{body}"
        );
        cancel.cancel();
    }

    #[tokio::test]
    async fn refuses_ports_private_targets_and_blocked_state() {
        let upstream = echo_server().await;
        // Port not allowed.
        let mut config = test_config(upstream);
        config.allowed_ports = vec![443];
        let (proxy, _tx, cancel) = start_proxy(config, true).await;
        let (_, head) = raw(
            proxy,
            format!(
                "CONNECT {upstream} HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                auth()
            ),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
        cancel.cancel();

        // Private target without an allowlist.
        let mut config = test_config(upstream);
        config.allow_nets.clear();
        let (proxy, _tx, cancel) = start_proxy(config, true).await;
        for target in [
            upstream.to_string(),
            format!("localhost:{}", upstream.port()),
        ] {
            let (_, head) = raw(
                proxy,
                format!(
                    "CONNECT {target} HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                    auth()
                ),
            )
            .await;
            assert!(head.starts_with("HTTP/1.1 403"), "{target}: {head}");
        }
        let (_, head) = raw(
            proxy,
            format!(
                "GET http://{upstream}/ HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                auth()
            ),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
        cancel.cancel();

        // Blocked engine.
        let (proxy, tx, cancel) = start_proxy(test_config(upstream), false).await;
        let (_, head) = raw(
            proxy,
            format!(
                "CONNECT {upstream} HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                auth()
            ),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 503"), "{head}");
        tx.send(true).unwrap();
        let (_, head) = raw(
            proxy,
            format!(
                "CONNECT {upstream} HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                auth()
            ),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        // https absolute-form is refused (CONNECT is the way).
        let (_, head) = raw(
            proxy,
            format!(
                "GET https://{upstream}/ HTTP/1.1\r\nProxy-Authorization: {}\r\n\r\n",
                auth()
            ),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 400"), "{head}");
        cancel.cancel();
    }

    #[test]
    fn public_address_classification() {
        for ip in [
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "240.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "192.0.0.8",
            "::1",
            "::",
            "fd00::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "2001:db8::1",
            "64:ff9b::a00:1",
            "::127.0.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn hop_by_hop_headers_are_removed() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "connection",
            HeaderValue::from_static("keep-alive, x-secret"),
        );
        headers.insert("x-secret", HeaderValue::from_static("1"));
        headers.insert("proxy-authorization", HeaderValue::from_static("Basic x"));
        headers.insert("accept", HeaderValue::from_static("*/*"));
        strip_hop_by_hop(&mut headers);
        assert_eq!(headers.len(), 1);
        assert!(headers.contains_key("accept"));
    }
}

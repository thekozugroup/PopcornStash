//! DNS forwarder on 127.0.0.54:5354 (section 5, step 4).
//!
//! The kill switch DNATs every port-53 packet from the engine user to this
//! forwarder. It relays raw DNS messages (UDP and TCP) to the tunnel's DNS
//! servers through sockets carrying mark [`FWMARK`](stash_core::internal_api::FWMARK),
//! which policy routing sends into the tunnel or nowhere. When no upstream
//! answers, clients get SERVFAIL right away instead of timing out.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream, UdpSocket};
use tokio::sync::{Semaphore, watch};
use tokio_util::sync::CancellationToken;

/// Fallback upstreams when the tunnel did not announce any (NordVPN's resolvers).
pub const DEFAULT_UPSTREAMS: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::new(103, 86, 96, 100)),
    IpAddr::V4(Ipv4Addr::new(103, 86, 99, 100)),
];

const HEADER_LEN: usize = 12;
/// Largest query we accept from a client (EDNS queries are far smaller).
pub const MAX_QUERY: usize = 4096;
/// Largest message on the wire (TCP length prefix limit).
const MAX_MESSAGE: usize = 65535;

#[derive(Debug, Clone)]
pub struct ForwarderConfig {
    /// SO_MARK for upstream sockets. None in tests (setting a mark needs NET_ADMIN).
    pub mark: Option<u32>,
    /// Per-upstream timeout.
    pub timeout: Duration,
    /// Concurrent UDP queries in flight.
    pub max_inflight: usize,
    /// Concurrent TCP client connections.
    pub max_tcp_clients: usize,
    /// Upstream port (53 in production; tests use a fake server's port).
    pub upstream_port: u16,
}

impl Default for ForwarderConfig {
    fn default() -> Self {
        Self {
            mark: Some(stash_core::internal_api::FWMARK),
            timeout: Duration::from_secs(2),
            max_inflight: 256,
            max_tcp_clients: 64,
            upstream_port: 53,
        }
    }
}

/// True for an upstream the forwarder may use: it must be reached through the
/// tunnel. Loopback (Docker's resolver is 127.0.0.11 and answers from the
/// host network), unspecified, multicast and broadcast addresses are refused
/// (a WireGuard `DNS =` line or a pushed OpenVPN option could name them).
/// The kill switch also drops marked packets that would not leave via `psw0`.
pub fn usable_upstream(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback() || v4.is_unspecified() || v4.is_multicast() || v4.is_broadcast())
        }
        IpAddr::V6(v6) => {
            let mapped_ok = v6
                .to_ipv4_mapped()
                .is_none_or(|v4| usable_upstream(&IpAddr::V4(v4)));
            mapped_ok && !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast())
        }
    }
}

/// Build a SERVFAIL answer for `query`: same id, opcode, RD and question, QR and RA set.
/// Returns None when the message is too short or not a query.
pub fn servfail(query: &[u8]) -> Option<Vec<u8>> {
    if query.len() < HEADER_LEN || query[2] & 0x80 != 0 {
        return None;
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    let question = if qdcount == 1 {
        question_end(query).map(|end| &query[HEADER_LEN..end])
    } else {
        None
    };
    let mut out = Vec::with_capacity(HEADER_LEN + question.map_or(0, <[u8]>::len));
    out.extend_from_slice(&query[0..2]);
    // QR=1, keep opcode and RD; AA=0, TC=0.
    out.push(0x80 | (query[2] & 0x79));
    // RA=1, Z=0, RCODE=2 (SERVFAIL).
    out.push(0x80 | 0x02);
    out.extend_from_slice(&(u16::from(question.is_some())).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    if let Some(question) = question {
        out.extend_from_slice(question);
    }
    Some(out)
}

/// End offset of the first question (name + type + class), if well formed.
pub fn question_end(message: &[u8]) -> Option<usize> {
    let mut pos = HEADER_LEN;
    let mut total = 0usize;
    loop {
        let len = usize::from(*message.get(pos)?);
        if len == 0 {
            pos += 1;
            break;
        }
        // Compression pointers are not valid in a query's first name.
        if len & 0xC0 != 0 {
            return None;
        }
        total += len + 1;
        if total > 255 {
            return None;
        }
        pos += 1 + len;
    }
    let end = pos + 4;
    (end <= message.len()).then_some(end)
}

/// True when `response` answers `query` (same id, QR set).
pub fn matches_query(query: &[u8], response: &[u8]) -> bool {
    response.len() >= HEADER_LEN
        && query.len() >= 2
        && response[0..2] == query[0..2]
        && response[2] & 0x80 != 0
}

fn truncated(response: &[u8]) -> bool {
    response.len() >= HEADER_LEN && response[2] & 0x02 != 0
}

fn upstream_socket(
    domain: Domain,
    ty: Type,
    protocol: Protocol,
    mark: Option<u32>,
) -> std::io::Result<Socket> {
    let socket = Socket::new(domain, ty, Some(protocol))?;
    socket.set_nonblocking(true)?;
    socket.set_cloexec(true)?;
    if let Some(mark) = mark {
        socket.set_mark(mark)?;
    }
    Ok(socket)
}

async fn query_udp(
    upstream: SocketAddr,
    query: &[u8],
    config: &ForwarderConfig,
) -> std::io::Result<Vec<u8>> {
    let domain = Domain::for_address(upstream);
    let socket = upstream_socket(domain, Type::DGRAM, Protocol::UDP, config.mark)?;
    let bind: SocketAddr = if upstream.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
    };
    socket.bind(&bind.into())?;
    let socket = UdpSocket::from_std(socket.into())?;
    socket.connect(upstream).await?;
    socket.send(query).await?;
    let mut buf = vec![0u8; MAX_MESSAGE];
    let deadline = tokio::time::Instant::now() + config.timeout;
    loop {
        let n = tokio::time::timeout_at(deadline, socket.recv(&mut buf))
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "upstream timed out")
            })??;
        if matches_query(query, &buf[..n]) {
            buf.truncate(n);
            return Ok(buf);
        }
        // A stray datagram with the wrong id: keep waiting until the deadline.
    }
}

async fn query_tcp(
    upstream: SocketAddr,
    query: &[u8],
    config: &ForwarderConfig,
) -> std::io::Result<Vec<u8>> {
    let domain = Domain::for_address(upstream);
    let socket = upstream_socket(domain, Type::STREAM, Protocol::TCP, config.mark)?;
    let socket = TcpSocket::from_std_stream(socket.into());
    let work = async {
        let mut stream = socket.connect(upstream).await?;
        write_framed(&mut stream, query).await?;
        let response = read_framed(&mut stream, MAX_MESSAGE).await?;
        if matches_query(query, &response) {
            Ok(response)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mismatched response",
            ))
        }
    };
    tokio::time::timeout(config.timeout * 2, work)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "upstream timed out"))?
}

async fn write_framed(stream: &mut TcpStream, message: &[u8]) -> std::io::Result<()> {
    let len = u16::try_from(message.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "message too large"))?;
    let mut framed = Vec::with_capacity(message.len() + 2);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(message);
    stream.write_all(&framed).await
}

async fn read_framed(stream: &mut TcpStream, max: usize) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len).await?;
    let len = usize::from(u16::from_be_bytes(len));
    if len < HEADER_LEN || len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad message length",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Resolve `query` through the first upstream that answers; SERVFAIL otherwise.
/// `prefer_tcp` is used for queries that arrived over TCP.
pub async fn forward(
    query: &[u8],
    upstreams: &[IpAddr],
    config: &ForwarderConfig,
    prefer_tcp: bool,
) -> Option<Vec<u8>> {
    if query.len() < HEADER_LEN || query.len() > MAX_QUERY || query[2] & 0x80 != 0 {
        return None;
    }
    for ip in upstreams {
        if config.mark.is_some() && !usable_upstream(ip) {
            tracing::debug!(%ip, "skipping a DNS upstream that is not reached through the tunnel");
            continue;
        }
        let upstream = SocketAddr::new(*ip, config.upstream_port);
        let result = if prefer_tcp {
            query_tcp(upstream, query, config).await
        } else {
            match query_udp(upstream, query, config).await {
                Ok(response) if truncated(&response) => {
                    query_tcp(upstream, query, config).await.or(Ok(response))
                }
                other => other,
            }
        };
        match result {
            Ok(response) => return Some(response),
            Err(e) => tracing::debug!(%upstream, error = %e, "DNS upstream did not answer"),
        }
    }
    servfail(query)
}

/// Shared state of a running forwarder.
#[derive(Clone)]
pub struct Forwarder {
    upstreams: watch::Receiver<Vec<IpAddr>>,
    config: Arc<ForwarderConfig>,
}

impl Forwarder {
    pub fn new(upstreams: watch::Receiver<Vec<IpAddr>>, config: ForwarderConfig) -> Self {
        Self {
            upstreams,
            config: Arc::new(config),
        }
    }

    fn current_upstreams(&self) -> Vec<IpAddr> {
        let list: Vec<IpAddr> = self
            .upstreams
            .borrow()
            .iter()
            .copied()
            // Without a mark (tests) nothing is tunnel-bound, so nothing is filtered.
            .filter(|ip| self.config.mark.is_none() || usable_upstream(ip))
            .collect();
        if list.is_empty() {
            DEFAULT_UPSTREAMS.to_vec()
        } else {
            list
        }
    }

    /// Bind UDP and TCP on `bind` and serve until `cancel`.
    pub async fn run(self, bind: SocketAddr, cancel: CancellationToken) -> std::io::Result<()> {
        let udp = Arc::new(UdpSocket::bind(bind).await?);
        let tcp = TcpListener::bind(bind).await?;
        tracing::info!(%bind, "DNS forwarder listening");
        let udp_task = tokio::spawn(self.clone().serve_udp(udp, cancel.clone()));
        let tcp_task = tokio::spawn(self.serve_tcp(tcp, cancel.clone()));
        cancel.cancelled().await;
        udp_task.abort();
        tcp_task.abort();
        Ok(())
    }

    /// Bind both sockets first (so callers learn about errors), then serve in the background.
    pub async fn spawn(
        self,
        bind: SocketAddr,
        cancel: CancellationToken,
    ) -> std::io::Result<SocketAddr> {
        let udp = Arc::new(UdpSocket::bind(bind).await?);
        let local = udp.local_addr()?;
        // Bind TCP to the same port (matters when `bind` asked for port 0).
        let tcp = TcpListener::bind(local).await?;
        tokio::spawn(self.clone().serve_udp(udp, cancel.clone()));
        tokio::spawn(self.serve_tcp(tcp, cancel));
        Ok(local)
    }

    async fn serve_udp(self, socket: Arc<UdpSocket>, cancel: CancellationToken) {
        let limit = Arc::new(Semaphore::new(self.config.max_inflight));
        let mut buf = vec![0u8; MAX_MESSAGE];
        loop {
            let received = tokio::select! {
                _ = cancel.cancelled() => return,
                r = socket.recv_from(&mut buf) => r,
            };
            let (n, peer) = match received {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::debug!(error = %e, "DNS forwarder receive error");
                    continue;
                }
            };
            if !(HEADER_LEN..=MAX_QUERY).contains(&n) {
                continue;
            }
            let Ok(permit) = limit.clone().try_acquire_owned() else {
                // Overloaded: answer SERVFAIL immediately instead of queueing.
                if let Some(answer) = servfail(&buf[..n]) {
                    let _ = socket.send_to(&answer, peer).await;
                }
                continue;
            };
            let query = buf[..n].to_vec();
            let socket = socket.clone();
            let this = self.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let upstreams = this.current_upstreams();
                if let Some(answer) = forward(&query, &upstreams, &this.config, false).await {
                    let _ = socket.send_to(&answer, peer).await;
                }
            });
        }
    }

    async fn serve_tcp(self, listener: TcpListener, cancel: CancellationToken) {
        let limit = Arc::new(Semaphore::new(self.config.max_tcp_clients));
        loop {
            let accepted = tokio::select! {
                _ = cancel.cancelled() => return,
                r = listener.accept() => r,
            };
            let Ok((mut stream, _)) = accepted else {
                continue;
            };
            let Ok(permit) = limit.clone().try_acquire_owned() else {
                continue;
            };
            let this = self.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = stream.set_nodelay(true);
                // Several queries per connection; idle connections close after 10 s.
                loop {
                    let query = match tokio::time::timeout(
                        Duration::from_secs(10),
                        read_framed(&mut stream, MAX_QUERY),
                    )
                    .await
                    {
                        Ok(Ok(query)) => query,
                        _ => return,
                    };
                    let upstreams = this.current_upstreams();
                    let Some(answer) = forward(&query, &upstreams, &this.config, true).await else {
                        return;
                    };
                    if write_framed(&mut stream, &answer).await.is_err() {
                        return;
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A query for example.com A with id 0x1234 and RD set.
    fn query() -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in ["example", "com"] {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.extend_from_slice(&[0, 0, 1, 0, 1]);
        q
    }

    fn answer_for(query: &[u8]) -> Vec<u8> {
        let mut a = query.to_vec();
        a[2] = 0x81;
        a[3] = 0x80;
        a[7] = 1; // ANCOUNT=1
        a.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        a
    }

    #[test]
    fn servfail_keeps_id_and_question() {
        let q = query();
        let s = servfail(&q).unwrap();
        assert_eq!(&s[0..2], &[0x12, 0x34]);
        assert_eq!(s[2], 0x81, "QR set, RD kept");
        assert_eq!(s[3] & 0x0F, 2, "RCODE SERVFAIL");
        assert_eq!(&s[4..6], &[0, 1]);
        assert_eq!(&s[12..], &q[12..]);
        assert!(servfail(&q[..5]).is_none());
        // Responses are never answered.
        assert!(servfail(&answer_for(&q)).is_none());
    }

    #[test]
    fn question_parsing_is_bounded() {
        let q = query();
        assert_eq!(question_end(&q), Some(q.len()));
        assert_eq!(question_end(&q[..q.len() - 1]), None);
        let mut pointer = q.clone();
        pointer[12] = 0xC0;
        assert_eq!(question_end(&pointer), None);
        let mut long = vec![0u8; 12];
        for _ in 0..10 {
            long.push(63);
            long.extend_from_slice(&[b'a'; 63]);
        }
        long.extend_from_slice(&[0, 0, 1, 0, 1]);
        assert_eq!(
            question_end(&long),
            None,
            "names over 255 bytes are invalid"
        );
        // A SERVFAIL for a malformed question carries no question.
        let mut bad = q.clone();
        bad.truncate(14);
        let s = servfail(&bad).unwrap();
        assert_eq!(s.len(), 12);
        assert_eq!(&s[4..6], &[0, 0]);
    }

    #[test]
    fn response_matching() {
        let q = query();
        assert!(matches_query(&q, &answer_for(&q)));
        let mut other = answer_for(&q);
        other[1] = 0x35;
        assert!(!matches_query(&q, &other));
        assert!(!matches_query(&q, &q), "a query is not a response");
    }

    async fn fake_upstream(answer: bool) -> (SocketAddr, CancellationToken) {
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = udp.local_addr().unwrap();
        let tcp = TcpListener::bind(addr).await.unwrap();
        let cancel = CancellationToken::new();
        let c1 = cancel.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                tokio::select! {
                    _ = c1.cancelled() => return,
                    r = udp.recv_from(&mut buf) => {
                        let (n, peer) = r.unwrap();
                        if answer {
                            // Send a stray datagram with the wrong id first; it must be ignored.
                            let mut stray = answer_for(&buf[..n]);
                            stray[0] ^= 0xFF;
                            udp.send_to(&stray, peer).await.unwrap();
                            udp.send_to(&answer_for(&buf[..n]), peer).await.unwrap();
                        }
                    }
                }
            }
        });
        let c2 = cancel.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = tokio::select! {
                    _ = c2.cancelled() => return,
                    r = tcp.accept() => r.unwrap(),
                };
                tokio::spawn(async move {
                    while let Ok(q) = read_framed(&mut stream, 4096).await {
                        write_framed(&mut stream, &answer_for(&q)).await.unwrap();
                    }
                });
            }
        });
        (addr, cancel)
    }

    fn test_config(port: u16) -> ForwarderConfig {
        ForwarderConfig {
            mark: None,
            timeout: Duration::from_millis(300),
            max_inflight: 8,
            max_tcp_clients: 4,
            upstream_port: port,
        }
    }

    #[tokio::test]
    async fn forwards_udp_and_tcp_and_servfails() {
        let (upstream, stop) = fake_upstream(true).await;
        let (tx, rx) = watch::channel(vec![upstream.ip()]);
        let cancel = CancellationToken::new();
        let forwarder = Forwarder::new(rx, test_config(upstream.port()));
        let bind = forwarder
            .spawn("127.0.0.1:0".parse().unwrap(), cancel.clone())
            .await
            .unwrap();

        // UDP through the forwarder.
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.send_to(&query(), bind).await.unwrap();
        let mut buf = [0u8; 512];
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], &answer_for(&query())[..]);

        // TCP through the forwarder, two queries on one connection.
        let mut stream = TcpStream::connect(bind).await.unwrap();
        for _ in 0..2 {
            write_framed(&mut stream, &query()).await.unwrap();
            let answer = read_framed(&mut stream, 4096).await.unwrap();
            assert_eq!(answer, answer_for(&query()));
        }

        // Upstream gone (tunnel down): SERVFAIL quickly.
        stop.cancel();
        let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let dead_port = dead.local_addr().unwrap().port();
        drop(dead);
        let _ = tx;
        let config = test_config(dead_port);
        let started = std::time::Instant::now();
        let answer = forward(&query(), &[upstream.ip()], &config, false)
            .await
            .unwrap();
        assert_eq!(answer[3] & 0x0F, 2);
        assert!(started.elapsed() < Duration::from_secs(2));

        // Oversized and malformed queries are dropped.
        assert!(
            forward(&vec![0u8; MAX_QUERY + 1], &[upstream.ip()], &config, false)
                .await
                .is_none()
        );
        assert!(
            forward(&[1, 2, 3], &[upstream.ip()], &config, false)
                .await
                .is_none()
        );
        cancel.cancel();
    }

    #[tokio::test]
    async fn silent_upstream_times_out_to_servfail() {
        let (upstream, stop) = fake_upstream(false).await;
        let config = test_config(upstream.port());
        let started = std::time::Instant::now();
        let answer = forward(&query(), &[upstream.ip()], &config, false)
            .await
            .unwrap();
        assert_eq!(answer[3] & 0x0F, 2);
        assert!(started.elapsed() >= Duration::from_millis(250));
        stop.cancel();
    }

    #[test]
    fn default_upstreams_when_none_known() {
        let (_tx, rx) = watch::channel(Vec::new());
        let forwarder = Forwarder::new(rx, ForwarderConfig::default());
        assert_eq!(forwarder.current_upstreams(), DEFAULT_UPSTREAMS.to_vec());
    }

    /// `DNS = 127.0.0.11` (Docker's resolver) would send the engine's queries
    /// out of the host network instead of the tunnel.
    #[test]
    fn loopback_and_local_upstreams_are_never_used() {
        let tunnel_dns: IpAddr = "10.66.0.1".parse().unwrap();
        let (_tx, rx) = watch::channel(vec![
            "127.0.0.11".parse().unwrap(),
            "::1".parse().unwrap(),
            "::ffff:127.0.0.1".parse().unwrap(),
            "0.0.0.0".parse().unwrap(),
            "224.0.0.251".parse().unwrap(),
            "255.255.255.255".parse().unwrap(),
            tunnel_dns,
        ]);
        let forwarder = Forwarder::new(rx, ForwarderConfig::default());
        assert_eq!(forwarder.current_upstreams(), vec![tunnel_dns]);
        let (_tx, rx) = watch::channel(vec!["127.0.0.11".parse().unwrap()]);
        let forwarder = Forwarder::new(rx, ForwarderConfig::default());
        assert_eq!(forwarder.current_upstreams(), DEFAULT_UPSTREAMS.to_vec());
        assert!(usable_upstream(&"192.168.1.1".parse().unwrap()));
        assert!(usable_upstream(&"2606:4700:4700::1111".parse().unwrap()));
    }
}

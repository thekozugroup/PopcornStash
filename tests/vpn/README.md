# VPN end-to-end test

`scripts/vpn-e2e.sh` checks the promise of section 5 of `docs/architecture.md`
against real containers, for WireGuard and OpenVPN: **only the torrent engine's
traffic and DNS go through the tunnel, the tunnel adds nothing to the main
routing table, the kill switch blocks the engine when the tunnel is down, and
the web app keeps working on the normal network throughout.** No internet
access is needed; everything runs on a private Docker network.

```
            private Docker network 172.31.66.0/24 (SUBNET_PREFIX)
   ┌──────────────────────────┬────────────────────────┬────────────────────────┐
   │ main  .30                │ wgserver  .10           │ echo  .20               │
   │ PopcornStash image       │ tests/vpn/wgserver.sh   │ busybox                 │
   │ popcornstash run         │  wg0 10.66.0.1/24       │ tests/vpn/echo.sh       │
   │  psw0 ───────────────────┼─▶ masquerade ──────────▶│  httpd: /cgi-bin/ip     │
   │  WireGuard 10.66.0.2, or │ ovpnserver  .11         │   answers caller's IP   │
   │  OpenVPN 10.67.0.2       │ tests/vpn/ovpnserver.sh │  dnsd: echo.test → .20  │
   │  (kill switch)           │  tun0 10.67.0.1/24      │                         │
   └──────────────────────────┴────────────────────────┴────────────────────────┘
```

- **echo** answers `GET /cgi-bin/ip` with the caller's address and runs a DNS
  server that knows `echo.test`, a name Docker's own resolver does not. HTTP binds
  IPv4 explicitly, so IPv4 fixture addresses are not reported as IPv6-mapped addresses.
- **wgserver** is the VPN provider: one WireGuard peer (kernel module, or
  boringtun when the kernel has none) that masquerades tunnel traffic, so the
  echo server sees `wgserver`'s address for anything that came through the VPN.
- **ovpnserver** is an OpenVPN provider as most run it: username and password
  logins (`auth-user-pass-verify`), `topology subnet` with a /24, a pushed DNS
  server (`echo`), and masquerade. Its test CA and server key are made with
  `openssl` in the image when the test starts.
- **main** is PopcornStash started with the settings of `compose.yaml`
  (`cap_drop: ALL` plus the listed capabilities, `/dev/net/tun`, read-only root
  with noexec tmpfs `/run` and `/tmp`, `rp_filter=2`) and a pre-written
  `/config/torrent/vpn/{wireguard.conf,vpn.json}` pointing at `wgserver`, with
  `Address = 10.66.0.2/24` and `DNS = <echo>`. Later the test writes new files
  into the VPN folder as the web server's user and asks for a reload, as the web
  server does after an upload. Before starting the unchanged `tini -- popcornstash run`
  command, a fixture wrapper creates an unrelated empty nftables table and records
  every existing table. Setup and both protocol reloads must preserve them. This
  also tests hosts whose Docker networking uses legacy iptables and creates no
  nftables tables of its own.

## Run

```sh
# Builds the image first (base images from Docker Hub):
scripts/vpn-e2e.sh

# Where Docker Hub is rate limited, behind a TLS-intercepting proxy:
DOCKER_HUB_MIRROR=1 BUILD_CA=/path/to/ca.pem scripts/vpn-e2e.sh

# Re-use an image that is already built:
SKIP_BUILD=1 IMAGE=popcornstash:dev DOCKER_HUB_MIRROR=1 scripts/vpn-e2e.sh
```

Needs `docker`, `curl` and `jq` on the host, and a host kernel with `/dev/net/tun`.
The host needs no WireGuard tools; keys are generated with `wg` inside the image.
Logs and command output are saved in `evidence/vpn-e2e/` (`KEEP=1` leaves the
containers running for inspection).

## What it checks

| Area | Check |
|---|---|
| Tunnel | The supervisor brings up `psw0` and the engine's `/health` reports `vpn.state = connected`; the engine starts its librqbit session (on IPv4 sockets when the kernel has no IPv6) |
| Routing | As the engine user (`docker exec --user 6881`), `curl` to the echo server reports **wgserver's** address; as `PUID` it reports **the container's own** address |
| Bypass | The engine user cannot leave through `eth0` even when binding to it (`curl --interface eth0`), or set WireGuard's privileged transport mark |
| DNS | As the engine user `echo.test` resolves (DNAT to the forwarder on 127.0.0.54:5354, then through the tunnel to the echo DNS); as `PUID` it does not (Docker's resolver, normal network) |
| Rules | `nft list table inet popcornstash` holds the port-53 DNAT and the engine-only output filter; `ip rule` holds priorities 99–102 for fwmark `0x5150` and `uidrange 6881-6881`; table 51820 is `default dev psw0`; every preexisting nftables table survives setup and reloads |
| Main table | With `Address = 10.66.0.2/24` the main table has no `psw0` route, `psw0` carries `10.66.0.2/32`, and `ip route get 10.66.0.1 uid PUID` leaves through `eth0` (the web server's traffic is never pulled into the tunnel) |
| Secrets | Neither the engine user nor `PUID` can read the WireGuard settings (`wg showconf psw0`) |
| Processes | `serve` runs as `PUID`, `torrent-engine` as 6881, the supervisor as root; `serve` and the engine have `CapEff = 0` and `NoNewPrivs = 1` |
| Proxy | The CONNECT proxy on 127.0.0.1:9092 asks for credentials (407) and refuses private targets (403) |
| Kill switch | After `docker stop wgserver`: the engine user has no connectivity (not even via `eth0`), its DNS fails instead of leaking to Docker's resolver, while `PUID` traffic and the web app's `/api/health` keep working |
| Recovery | After `docker start wgserver` the engine user is tunneled again without restarting PopcornStash |
| Reload | `POST /vpn/reload` on the internal API (what the web server does after a VPN change) restarts the engine, the supervisor re-applies the VPN, and the engine is tunneled again |
| DNS upstreams | After a reload with `DNS = 127.0.0.11, <echo>`, engine DNS still resolves `echo.test` through the tunnel and Docker's resolver is never asked (the echo container's name does not resolve) |
| OpenVPN | After switching to OpenVPN and a reload: connected; the engine is tunneled (echo sees `ovpnserver`) and resolves through the pushed DNS; `PUID` goes direct; the pushed /24 adds no main-table route and `psw0` carries `10.67.0.2/32`; OpenVPN runs as root on root-only copies in `/run/popcornstash/openvpn` (not the web server's files) that the engine cannot read; with the server stopped the engine has no connectivity while the web app works; it recovers when the server returns |
| Shutdown | `docker stop` stops the children, the tunnel and the rules, and the container exits 0; the logs contain neither the WireGuard private key, the OpenVPN password nor the internal token |

The internal token is read from the web server's environment inside the
container and never printed.

## Limitations

- **Kernel WireGuard** is only exercised where the host has the module; on
  hosts without it both sides use boringtun.
- **Kernels without IPv6** (booted with `ipv6.disable=1`): librqbit 9 opens its
  sockets on `[::]`; the vendored `librqbit-dualstack-sockets` patch falls back to
  `0.0.0.0` there, so the engine's session starts on IPv4 and the session check
  runs on such hosts too. boringtun has the same problem and is patched in the
  `Dockerfile`. `scripts/fullstack-e2e.sh` downloads a real torrent through the
  tunnel (see `tests/fullstack/README.md`).
- The public-IP proof (`api.ipify.org` / NordVPN insights) needs the internet
  and is not checked here; its failure is only logged. It is unit-tested in the
  engine against local fakes, including a NordVPN `protected: false` answer.

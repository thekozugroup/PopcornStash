# VPN end-to-end test

`scripts/vpn-e2e.sh` checks the promise of section 5 of `docs/architecture.md`
against real containers: **only the torrent engine's traffic and DNS go through
the tunnel, the kill switch blocks the engine when the tunnel is down, and the
web app keeps working on the normal network throughout.** No internet access is
needed; everything runs on a private Docker network.

```
            private Docker network 172.31.66.0/24 (SUBNET_PREFIX)
   ┌──────────────────────────┬───────────────────────┬────────────────────────┐
   │ main  .30                │ wgserver  .10          │ echo  .20               │
   │ PopcornStash image       │ PopcornStash image     │ busybox                 │
   │ popcornstash run         │ tests/vpn/wgserver.sh  │ tests/vpn/echo.sh       │
   │  psw0 10.66.0.2 ─────────┼─▶ wg0 10.66.0.1        │  httpd: /cgi-bin/ip     │
   │  (WireGuard, kill switch)│   masquerade ─────────▶│   answers caller's IP   │
   │                          │                        │  dnsd: echo.test → .20  │
   └──────────────────────────┴───────────────────────┴────────────────────────┘
```

- **echo** answers `GET /cgi-bin/ip` with the caller's address and runs a DNS
  server that knows `echo.test`, a name Docker's own resolver does not.
- **wgserver** is the VPN provider: one WireGuard peer (kernel module, or
  boringtun when the kernel has none) that masquerades tunnel traffic, so the
  echo server sees `wgserver`'s address for anything that came through the VPN.
- **main** is PopcornStash started with the settings of `compose.yaml`
  (`cap_drop: ALL` plus the listed capabilities, `/dev/net/tun`, read-only root,
  `rp_filter=2`) and a pre-written `/config/torrent/vpn/{wireguard.conf,vpn.json}`
  pointing at `wgserver`, with `DNS = <echo>`.

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
| Tunnel | The supervisor brings up `psw0` and the engine's `/health` reports `vpn.state = connected`; the engine starts its librqbit session (see Limitations) |
| Routing | As the engine user (`docker exec --user 6881`), `curl` to the echo server reports **wgserver's** address; as `PUID` it reports **the container's own** address |
| Bypass | The engine user cannot leave through `eth0` even when binding to it (`curl --interface eth0`) |
| DNS | As the engine user `echo.test` resolves (DNAT to the forwarder on 127.0.0.54:5354, then through the tunnel to the echo DNS); as `PUID` it does not (Docker's resolver, normal network) |
| Rules | `nft list table inet popcornstash` holds the port-53 DNAT and the engine-only output filter; `ip rule` holds priorities 99–102 for fwmark `0x5150` and `uidrange 6881-6881`; table 51820 is `default dev psw0`; Docker's own tables are untouched |
| Processes | `serve` runs as `PUID`, `torrent-engine` as 6881, the supervisor as root; `serve` and the engine have `CapEff = 0` and `NoNewPrivs = 1` |
| Proxy | The CONNECT proxy on 127.0.0.1:9092 asks for credentials (407) and refuses private targets (403) |
| Kill switch | After `docker stop wgserver`: the engine user has no connectivity (not even via `eth0`), its DNS fails instead of leaking to Docker's resolver, while `PUID` traffic and the web app's `/api/health` keep working |
| Recovery | After `docker start wgserver` the engine user is tunneled again without restarting PopcornStash |
| Reload | `POST /vpn/reload` on the internal API (what the web server does after a VPN change) restarts the engine, the supervisor re-applies the VPN, and the engine is tunneled again |
| Shutdown | `docker stop` stops the children, the tunnel and the rules, and the container exits 0; the logs contain neither the WireGuard private key nor the internal token |

The internal token is read from the web server's environment inside the
container and never printed.

## Limitations

- **OpenVPN** is covered by unit tests (command line, hook script, `.ovpn`
  validation) but not end to end: there is no OpenVPN server in the test.
- **Kernels without IPv6** (booted with `ipv6.disable=1`): librqbit 9 always
  opens an IPv6 UDP socket for trackers, so the engine cannot start its torrent
  session there. The tunnel, routing, DNS and kill switch checks still run; the
  session check is reported as `SKIP` and the engine's `/health` explains the
  reason. boringtun has the same problem and is patched in the `Dockerfile`.
- The public-IP proof (`api.ipify.org` / NordVPN insights) needs the internet
  and is not checked here; its failure is only logged.

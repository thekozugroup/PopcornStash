# PopcornStash

A private home for your family's videos, photos and downloaded media. Part of the Popcorn series of open source video projects.

PopcornStash points at one folder on your NAS, Unraid share or home server and keeps it organized. Family members sign in with passkeys, browse Movies, Shows, Home Videos and Photos, and watch in any browser. New files arrive from torrents, web links, the Chrome extension, Android's share sheet, browser uploads, or by copying them into the Inbox folder, and are sorted automatically.

## What it does

- **Passkeys only.** No passwords. The owner invites family members, and each person adds passkeys for their own phone and laptop.
- **Torrents that only use your VPN.** Paste a NordVPN access token (or upload any WireGuard or OpenVPN file). Only the torrent engine's traffic enters the tunnel; a kill switch stops it if the tunnel drops. The rest of the app stays on your normal network.
- **Every way in leads to the same place.** Torrents, links, uploads and drops all go through `Downloads/In Progress` and `Downloads/Completed`, then get sorted into `Movies`, `Shows`, `Home Videos`, `Photos` or `Other`. Anything uncertain waits in Inbox for one tap of review.
- **Your folders stay yours.** Files outside `Inbox/` and `Downloads/` are indexed where they are and never moved unless you ask.
- **Plays everywhere.** Videos play directly when the browser can, and are transcoded on the fly (HLS, with optional Intel/AMD/NVIDIA hardware encoding) when it cannot. Photos from phones, including HEIC, show in a timeline.
- **Nothing leaves the server.** No metadata scraping, no accounts, no telemetry.

## Quick start

You need Docker and an HTTPS address for the server (passkeys require HTTPS on a domain name; Tailscale, Caddy, Nginx Proxy Manager or Cloudflare Tunnel all work, see [docs/deployment.md](docs/deployment.md)).

```sh
git clone https://github.com/thekozugroup/PopcornStash
cd PopcornStash
cp .env.example .env        # set STASH_PUBLIC_URL, APPDATA_PATH and STASH_PATH
docker compose up -d
docker compose logs popcornstash | grep -A 1 "setup code"
```

Open your HTTPS address, enter the setup code and create the owner passkey. Then set up the VPN in Settings to turn on torrents.

On Unraid, use the template in [unraid/popcornstash.xml](unraid/popcornstash.xml).

## Documentation

- [Installing and running PopcornStash](docs/deployment.md): HTTPS options, Compose, Unraid, first run, recovery, folders, hardware acceleration, backups.
- [Torrents and your VPN](docs/vpn.md): how torrent traffic is kept inside the tunnel, NordVPN and WireGuard/OpenVPN setup, checking that it works.
- [Chrome extension](docs/extension.md): pairing, sending links and downloads, privacy.
- [Security](docs/security.md): the threat model and what each layer protects.
- [Architecture](docs/architecture.md): the API, database, folder layout and process model.

## How it is built

- One Docker image and one container. A small root supervisor sets up the VPN and kill switch, then runs the web server as your `PUID:PGID` and the torrent engine as its own user, both without privileges.
- Rust for everything on the server: axum, SQLite, webauthn-rs, librqbit, ffmpeg and libvips for media.
- The web UI is React with Meta's [Astryx](https://astryx.atmeta.com) design system and a Popcorn theme shared with Popcorn SR.
- The Chrome extension is a small Manifest V3 extension in TypeScript.

## Development

```sh
npm --prefix web ci && npm --prefix web run build   # the server serves web/dist
cargo test --workspace
scripts/dev-fixture.sh /tmp/stash                     # a small sample library
STASH_ROOT=/tmp/stash STASH_CONFIG=/tmp/stash-config \
  STASH_PUBLIC_URL=http://localhost:8788 cargo run -p popcornstash -- serve
```

`popcornstash serve` alone runs everything except torrents. End-to-end tests live in [tests/e2e](tests/e2e), [tests/vpn](tests/vpn) and [tests/fullstack](tests/fullstack); `scripts/docker-smoke.sh` checks a built image.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).

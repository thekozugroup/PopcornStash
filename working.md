# PopcornStash — iteration 1

## Objective
Ship the Docker server for PopcornStash: a passkey-gated, private family library for videos, photos and downloaded media, with built-in transcoding, torrents routed only through a VPN (NordVPN quick setup), a web download manager, resumable uploads, an Inbox folder, automatic sorting, a Chrome extension and a PWA share target. The Android app is a later phase; the API and device pairing are ready for it.

## Acceptance
- Rust backend (axum, SQLite, webauthn-rs, librqbit); web UI in React + Astryx with the Popcorn design world.
- Passkey-only access, first-run setup code, invites, device pairing, CSRF/origin checks, rate limits.
- `Downloads/In Progress` → `Downloads/Completed` → sorted library for torrents, links, uploads and Inbox drops. Files outside Inbox/Downloads never moved implicitly.
- One container. A root supervisor runs the web server as PUID:PGID and the torrent engine as its own user; only the engine user is routed into the VPN (policy routing + nftables kill switch + device-bound sockets + DNS forced through the tunnel + Landlock). WireGuard (kernel or boringtun) and OpenVPN; NordVPN token setup.
- Direct play when possible; HLS transcoding otherwise; thumbnails for video and photos (HEIC included).
- One Docker image and one service (compose and a single Unraid template), CI, Chrome extension.
- Verified by Rust tests, Playwright with a virtual authenticator, Docker smoke, and a VPN kill-switch test against a local WireGuard peer.

## Status
Started 2026-10-12. See the commit log on main for progress.

## Evidence
- `tests/e2e/`: Playwright against the real server and a fresh sample stash (35 tests), including a regression check for each bug found in exploratory QA and in the final design preview on 2026-10-13.
- `scripts/fullstack-e2e.sh` (`tests/fullstack/`): the whole journey in Docker through the real UI: first-run setup, a WireGuard file, a magnet downloaded from a local seeder only through the VPN (the tracker and the seeder see only the VPN peer's address), sorted into `Movies/Lantern Bay (2018)/` as 1000:1000, played; the VPN peer stopped and restarted. Screenshots and records in `evidence/fullstack/` (git-ignored).
- `evidence/preview/` (git-ignored): every page as the owner at 1440 light, 1440 dark and 390 light, on a fuller sample library with illustrated stills; the design preview.
- `evidence/design-qa/` (git-ignored): the design director's pass of 2026-10-13 (Taste + Impeccable critique and audit; Nielsen 28 to 32 of 40, audit 15 to 18 of 20, detector clean), with matching before and after shots (63 pairs: 16 pages and 5 empty states at 1440 light, 1440 dark and 390 light); DESIGN.md now documents the system as built.
- `evidence/preview-fixes/` (git-ignored): before and after shots of the seven fixes from the final design preview (one poster per show; WireGuard and OpenVPN named in the audit log; file names wrapping at their separators; the VPN status line and sidebar tag; the photo viewer's controls and inset; the phone section picker in Settings; Home keeping every kind, photos included), at 1440 and 390 on the preview library. The connected WireGuard VPN and the in-progress torrent in those shots come from Playwright route mocks, since `popcornstash serve` alone runs with torrents off.
- `evidence/final/` (git-ignored): the complete design preview, re-captured on 2026-10-13 after the seven fixes, with nothing mocked: the `popcornstash:dev` image built from the tree, run with compose.yaml's flags beside the fullstack echo, tracker, seeder and WireGuard peer, on the preview library (11 movies, 3 shows, phone clips, EXIF photos, music, PDFs, 2 Inbox files). A real torrent held at 64% through the VPN (the tracker saw only the VPN peer's address), the extension paired, two members and an open invite. 70 PNGs (every page at 1440 light, 1440 dark and 390 light, plus the extension popup and options in light and dark) with captions in `index.json`; each image checked by eye and every page checked for sideways scroll.
- `docs/security.md`: threat model, layers, reporting and known limitations from the whole-system security audit of 2026-10-13. Each defect it found has a regression test that fails without its fix (cookie leak on proxied redirects, partial files the engine could read, private /config modes, untrusted image loaders, RAW-named files opening libvips' untrusted loaders, fail-closed paths, one-entry torrent content, bounded engine answers, codes in request logs), and `scripts/docker-smoke.sh` checks the private modes, that the engine user is shut out of them, and the ImageMagick policy. `cargo audit` is clean apart from the two advisories reviewed in `.cargo/audit.toml`; `npm audit` is clean for web, extension and e2e.

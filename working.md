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

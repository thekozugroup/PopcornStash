# PopcornStash — iteration 1

## Objective
Ship the Docker server for PopcornStash: a passkey-gated, private family library for videos, photos and downloaded media, with built-in transcoding, torrents routed only through a VPN (NordVPN quick setup), a web download manager, resumable uploads, an Inbox folder, automatic sorting, a Chrome extension and a PWA share target. The Android app is a later phase; the API and device pairing are ready for it.

## Acceptance
- Rust backend (axum, SQLite, webauthn-rs, librqbit); web UI in React + Astryx with the Popcorn design world.
- Passkey-only access, first-run setup code, invites, device pairing, CSRF/origin checks, rate limits.
- `Downloads/In Progress` → `Downloads/Completed` → sorted library for torrents, links, uploads and Inbox drops. Files outside Inbox/Downloads never moved implicitly.
- Torrent engine isolated in a VPN container with nftables kill switch and device-bound sockets; WireGuard (kernel or boringtun) and OpenVPN; NordVPN token setup.
- Direct play when possible; HLS transcoding otherwise; thumbnails for video and photos (HEIC included).
- Docker image, compose, Unraid templates, CI, Chrome extension.
- Verified by Rust tests, Playwright with a virtual authenticator, Docker smoke, and a VPN kill-switch test against a local WireGuard peer.

## Status
Started 2026-10-12. See the commit log on main for progress.

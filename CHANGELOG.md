# Changelog

Changes to PopcornStash, newest first. Version numbers follow [Semantic Versioning](https://semver.org/). Releases are built, checked and published from a developer machine with `scripts/release.sh`, which takes the release notes from this file (see "Releasing" in the README).

## [0.1.0] - 2026-10-13

The first release: the PopcornStash server as one Docker image for amd64 and arm64 (`ghcr.io/thekozugroup/popcornstash`), and the Chrome extension as `popcornstash-extension.zip`.

### Sign-in and people

- Everyone signs in with a passkey; there are no passwords. Passkeys need HTTPS on a domain name, or `http://localhost`, and the sign-in page says so on a bare IP address.
- A new install asks for a one-time setup code from the container log before the owner passkey is created. `popcornstash admin setup-code` prints a new code.
- Owners invite people with single-use links that expire after 72 hours, as members or as owners. Members can watch, browse, upload, add downloads and review the Inbox; only owners change settings and the VPN, manage people and empty the trash.
- Each person can add more passkeys. An owner who has lost every passkey can get a 30-minute recovery link with `popcornstash admin reset-owner`.
- Devices such as the Chrome extension pair with a short code that a signed-in person approves, and get a key that can only do what the device needs (the extension's can add downloads and see their progress).
- Cookie requests must come from the server's own origin, sign-in attempts are rate limited, pages are served with a strict Content Security Policy, and owner actions are written to an activity log. docs/security.md describes the threat model.

### Library

- One library folder holds `Movies`, `Shows`, `Home Videos`, `Photos`, `Other`, `Inbox` and `Downloads`.
- Files that arrive in `Inbox/` or `Downloads/Completed/` are sorted by their names (movie titles and years, episode markers, release tags) and by camera metadata. Files PopcornStash is unsure about wait in the Inbox for one tap of review.
- Files anywhere else are indexed where they are and only moved when someone chooses Organize on the item.
- Deleted items go to a trash that keeps them for 30 days.
- Nothing about the library is looked up online.

### Playback and photos

- Videos play directly when the browser supports the file and are otherwise transcoded to HLS with ffmpeg, optionally with Intel or AMD (VAAPI), Intel Quick Sync or NVIDIA (NVENC) hardware encoding.
- Playback position is saved, and Home has Continue watching.
- Thumbnails are made for videos and photos, including HEIC and camera RAW files. Photos are shown in a timeline with a full-screen viewer.

### Bringing files in

- Files arrive from browser uploads (resumable, by button or by dropping on the page), web links, magnet links and `.torrent` files, the Chrome extension, Android's share sheet (the web app is a share target), and files copied into `Inbox/`.
- Everything passes through `Downloads/In Progress` and `Downloads/Completed`. The Downloads page shows progress and speed, and torrent upload and download limits are set in Settings.
- Links to private network addresses are refused and never fetched.

### Torrents and the VPN

- Torrents run in a separate engine (librqbit) as its own user, and only that user's traffic and DNS go through the VPN: policy routing, an nftables kill switch, sockets bound to the tunnel and Landlock. When the tunnel is down torrents stop; there is no fallback to the home connection. The rest of the app stays on the normal network.
- The VPN is set up in Settings with a NordVPN access token, or with any WireGuard or OpenVPN configuration file. WireGuard uses the kernel module when the host has it and boringtun otherwise. The VPN page shows the public address seen through the tunnel.
- Torrents work on hosts booted without IPv6, and a seeder that goes away and comes back is reconnected about 10 seconds after it returns (small patches to boringtun, librqbit and librqbit-dualstack-sockets, documented in `vendor/` and the Dockerfile).

### Running it

- One image and one container. A small supervisor starts as root, sets up the VPN and the kill switch, then runs the web server as `PUID:PGID` and the torrent engine as its own user, both without capabilities. Without `NET_ADMIN` or `/dev/net/tun` the web app still runs and explains why torrents are off.
- `compose.yaml`, an Unraid template (`unraid/popcornstash.xml`) and a `docker run` example in docs/deployment.md, all with the same container settings: read-only root filesystem, all capabilities dropped except the few the supervisor needs, and `no-new-privileges`.
- Optional built-in HTTPS, and `popcornstash healthcheck` for the container health check.
- Images for linux/amd64 and linux/arm64. The arm64 image's Rust code is cross-compiled on the build machine.

### Web app and extension

- A React web app built with the Astryx design system and the Popcorn theme, in light and dark, usable on phones.
- A Manifest V3 Chrome extension that sends links, videos, images, magnet links and (if you want) every browser download to your server. It is not in the Chrome Web Store; it is loaded unpacked from the zip (docs/extension.md).

### How this release was checked

- Rust unit and integration tests, the web and extension type checks and unit tests, a Playwright suite against the real server with a virtual passkey authenticator, a Docker smoke test of the image with the compose settings, a VPN kill-switch test against local WireGuard and OpenVPN servers, and a full-stack test that downloads a torrent through the VPN in Docker and plays it in the browser. `scripts/check.sh --full` runs them all.

### Not yet

- The Android app. The API and device pairing are ready for it.

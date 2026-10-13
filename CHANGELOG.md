# Changelog

Changes to PopcornStash, newest first. Version numbers follow [Semantic Versioning](https://semver.org/). Releases are built, checked and published from a developer machine with `scripts/release.sh`, which takes the release notes from this file (see "Releasing" in the README).

## [0.1.0] - 2026-10-13

The first release: the PopcornStash server as one Docker image for amd64 and arm64 (`ghcr.io/thekozugroup/popcornstash`), and the Chrome extension as `popcornstash-extension.zip`. A few parts have not yet been tried with the real services or hardware, such as a NordVPN token, a real VPN connection, graphics-chip encoding and arm64 machines; "Not yet verified" lists them and what was checked instead.

### Sign-in and people

- Everyone signs in with a passkey; there are no passwords. Passkeys need HTTPS on a domain name, or `http://localhost`, and the sign-in page says so on a bare IP address.
- A new install asks for a one-time setup code from the container log before the owner passkey is created. `popcornstash admin setup-code` prints a new code.
- Owners invite people with single-use links that expire after 1, 3 or 7 days (3 by default), as members or as owners. Members can watch, browse, upload, add downloads and review the Inbox; only owners change settings and the VPN, manage people and empty the trash.
- Each person can add more passkeys. An owner who has lost every passkey can get a 30-minute recovery link with `popcornstash admin reset-owner`.
- Devices such as the Chrome extension pair with a short code that a signed-in person approves, and get a key that can only do what the device needs (the extension's can add downloads and see their progress).
- Cookie requests must come from the server's own origin, sign-in attempts are rate limited, pages are served with a strict Content Security Policy, and owner actions are written to an activity log. docs/security.md describes the threat model.

### Library

- One library folder holds `Movies`, `Shows`, `Home Videos`, `Photos`, `Other`, `Inbox` and `Downloads`.
- Files that arrive in `Inbox/` or `Downloads/Completed/` are sorted by their names (movie titles and years, episode markers, release tags) and by camera metadata. Files PopcornStash is unsure about wait in the Inbox for one tap of review.
- Documents, archives and other files that are not videos, photos or music wait in the Inbox for one review, then go to `Other/`, where everyone can reach them. Release files inside a download (`.nfo`, `.txt`, `.sfv`) stay in `Downloads/Completed/`, and subtitles move with their video.
- Posters, covers, fan art and screenshots that come with a movie, show or album are not added to Photos; other pictures in such a download wait in the Inbox. A download of only pictures, or one image sent from the extension, goes to Photos.
- Only `Inbox/` and `Downloads/Completed/` are sorted. Anything else in `Downloads/`, such as another download client's folders, is never moved or listed, so other apps can share the folder.
- Files anywhere else are indexed where they are and only moved when someone chooses Organize on the item.
- Browser uploads keep the date the browser reports for the file (from 1990 to now), so scans and exports without camera data are filed under their own month.
- Deleted items go to a trash that keeps them for 30 days by default (Settings, Library).
- Nothing about the library is looked up online.

### Playback and photos

- Videos play directly when the browser supports the file and are otherwise transcoded to HLS with ffmpeg, optionally with Intel or AMD (VAAPI), Intel Quick Sync or NVIDIA (NVENC) hardware encoding, chosen in Settings, System, Streaming, or fixed for the container with `STASH_HWACCEL`.
- The web player uses hls.js wherever the browser supports Media Source Extensions and the browser's own HLS only where they do not, so resuming a converted video starts at the resume point straight away.
- Playback position is saved, and Home has Continue watching.
- Thumbnails are made for videos and photos, including HEIC and camera RAW files. Photos are shown in a timeline with a full-screen viewer.

### Bringing files in

- Files arrive from browser uploads (resumable, by button or by dropping on the page), web links, magnet links and `.torrent` files, the Chrome extension, Android's share sheet (install the web app from Chrome; links, photos, videos and other files), and files copied into `Inbox/`.
- Files shared from Android wait on the phone until the person confirms on the share page, then upload through Downloads like any other upload. An upload cut off by a reload continues from the copy kept on the phone, for the person who started it only. Signing out deletes the kept files and that person's unfinished uploads, so nothing passes to the next person on a shared phone or tablet.
- Everything passes through `Downloads/In Progress` and `Downloads/Completed`. The Downloads page shows progress and speed. Torrent upload and download limits, and how many torrents download at once, are set in Settings.
- Links to private network addresses are refused and never fetched.

### Torrents and the VPN

- Torrents run in a separate engine (librqbit) as its own user, and only that user's traffic and DNS go through the VPN: policy routing, an nftables kill switch, sockets bound to the tunnel and Landlock. When the tunnel is down torrents stop; there is no fallback to the home connection. The rest of the app stays on the normal network.
- The VPN is set up in Settings with a NordVPN access token, or with any WireGuard or OpenVPN configuration file. WireGuard uses the kernel module when the host has it and boringtun otherwise. The VPN page shows the public address seen through the tunnel. When NordVPN says that address is not protected, torrents stop until a later check, made every minute, no longer says so, also across a reconnect or a restart.
- "Torrents at once" (1 to 50, 5 by default) limits how many torrents download at the same time. The others wait in line ("Waiting for a free slot") and start oldest first; seeding and paused torrents do not count.
- `STASH_ALLOW_NO_VPN`, for networks whose router already tunnels everything, runs torrents without the built-in VPN. The VPN page, the sidebar, the Downloads page, the share page and the extension then show the permanent warning "No VPN: torrents use this network", even when a VPN is saved. Web downloads then use the same network whatever "Web downloads use the VPN" says.
- Torrents work on hosts booted without IPv6, and a seeder that goes away and comes back is reconnected about 10 seconds after it returns (small patches to boringtun, librqbit and librqbit-dualstack-sockets, documented in `vendor/` and the Dockerfile).

### Running it

- One image and one container. A small supervisor starts as root, sets up the VPN and the kill switch, then runs the web server as `PUID:PGID` and the torrent engine as its own user, both without capabilities. Without `NET_ADMIN` or `/dev/net/tun` the web app still runs and explains why torrents are off.
- `compose.yaml`, an Unraid template (`unraid/popcornstash.xml`) and a `docker run` example in docs/deployment.md, all with the same container settings: read-only root filesystem, all capabilities dropped except the few the supervisor needs, and `no-new-privileges`. All three name the container `popcornstash`, so the documented `docker exec popcornstash …` commands work everywhere, Unraid included.
- When `PUID:PGID` cannot write the library folder, the log names the folder and the `chown` that fixes it.
- Optional built-in HTTPS, and `popcornstash healthcheck` for the container health check.
- Images for linux/amd64 and linux/arm64. The arm64 image's Rust code is cross-compiled on the build machine.
- Settings, System links the running version to its source code, and `/api/health` reports the same address.
- Licenses: the image carries PopcornStash's license and NOTICE, the licenses of every Rust crate and npm package it is built from, and a list of its Debian packages with their versions in `/usr/share/doc/popcornstash/`; the source of each Debian package, including the GPL builds of ffmpeg, x264 and x265, is at `https://snapshot.debian.org/package/<source package>/<source version>/`, and PopcornStash's own source for this release is at https://github.com/thekozugroup/PopcornStash/tree/v0.1.0.

### Web app and extension

- A React web app built with the Astryx design system and the Popcorn theme, in light and dark, usable on phones.
- A Manifest V3 Chrome extension that sends links, videos, images, magnet links and (if you want) every browser download to your server. It is not in the Chrome Web Store; it is loaded unpacked from the zip (docs/extension.md).

### How this release was checked

`scripts/release.sh` publishes only after all of these pass on the release commit:

- Rust formatting, clippy and the unit and integration tests; the web app's type checks and lint; the extension's type checks and unit tests; `cargo audit` and `npm audit`; the committed third-party license notices match the lockfiles.
- A Playwright suite against the real server with a virtual passkey authenticator.
- On the amd64 image that is published: a Docker smoke test with the compose settings (including the license files and a real HEIC photo sorted from the Inbox), a VPN kill-switch test against local WireGuard and OpenVPN servers, and a full-stack test that downloads a torrent through the VPN in Docker and plays it in the browser.
- The arm64 image passes the Docker smoke test under QEMU, apart from the VPN and engine checks that emulation cannot run (torrents stay off there).

### Not yet verified

These have not been verified against the real services or hardware yet. What was checked instead:

- The NordVPN access token setup has only run against a local stand-in for NordVPN's API, not with a real token. NordVPN's live country list, server recommendations and `.ovpn` files were checked against the code.
- No real WireGuard or OpenVPN connection to NordVPN or another commercial provider has been made. The tunnel, the kill switch, DNS through the tunnel and a whole torrent download were tested in Docker against local WireGuard and OpenVPN servers.
- The public-address proof has not run through a real tunnel. It is tested against local stand-ins for NordVPN's check and ipify.
- Hardware encoding (VAAPI, Quick Sync, NVENC) has not run on a real graphics chip. Only the ffmpeg command for each encoder is unit-tested, and the fallback was checked: a hardware encoder that fails falls back to the processor, so playback still works, more slowly.
- The VPN and kill switch have not run on real arm64 hardware. The arm64 image has only run under QEMU emulation, with torrents off.
- Sharing from a real Android phone has not been tried. The share target was tested in Chromium through the web app's service worker.

### Known limitations

- Every HLS stream re-encodes the video, even when only the container or the audio needs converting (for example an H.264 MKV, or an MP4 with DTS audio). On a slow NAS that means slower starts, and 2 streams at once by default. A remux path that copies the video is planned after 0.1.0.
- The Android app is a later phase. The API and device pairing are ready for it; until then, Android phones use the web app and its share target.

### Fixed before release

Problems found in the final review of 0.1.0. They affected only builds made from the source before this release.

- With `STASH_ALLOW_NO_VPN` and a saved VPN, the app said torrents were paused while they ran on the home connection. It now shows the no-VPN warning everywhere and hides the unused VPN setup.
- "Torrents at once" was saved but not enforced. It is now.
- Resuming a converted video in Chrome restarted the conversion twice. It now starts at the resume point.
- When NordVPN said the address was not protected, the VPN page said torrents had stopped, but they kept running. They now stop.
- Documents and other files that are not media stayed unsorted and could not be reached. They now wait in the Inbox, then go to `Other/`.
- Loose files and other download clients' folders in `Downloads/` were sorted into the library. They are now left alone.
- Posters and screenshots from movie and show downloads were added to Photos. They now stay with the download.
- Browser uploads without camera data were filed under the upload month. They now keep the file's own date.
- A library folder the web server could not write gave a bare "Permission denied" and a restart loop. The log now names the folder and the fix.
- Closing a photo while it was being converted left a temporary file in the cache. It is removed, and leftovers are cleaned up on start.
- The photo timeline returned 120 photos per page instead of 200.
- On Unraid the container was named `PopcornStash`, so `docker exec popcornstash …` failed. The template now names it `popcornstash`.
- `STASH_HWACCEL` was ignored once any setting had been saved. When set, it now decides, and Settings shows it greyed out.
- The image shipped without the license notices of the software it includes. They are now in `/usr/share/doc/popcornstash/`.
- Android's share sheet took only links. It now takes photos, videos and other files too.

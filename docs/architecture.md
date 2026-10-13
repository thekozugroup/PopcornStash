# PopcornStash architecture

This document is the contract between the Rust backend, the web UI, the Chrome extension and the future Android app. Change it in the same commit as any behavior it describes.

## 1. Shape of the system

One container, one binary (`popcornstash`), three processes.

```
                 HTTPS (reverse proxy, Tailscale, or built-in TLS)
                                    │ :8788
┌───────────────────────────────────▼────────────────────────────────────────────┐
│ popcornstash container  (NET_ADMIN, /dev/net/tun, read-only rootfs)              │
│                                                                                  │
│  popcornstash run   PID 1 under tini, root: the supervisor                       │
│   ├─ VPN tunnel psw0 (kernel WireGuard → boringtun fallback, or OpenVPN)         │
│   ├─ policy routing: only the engine user is routed into psw0                    │
│   ├─ nftables kill switch: the engine user may only send through psw0            │
│   ├─ DNS forwarder 127.0.0.54:5354 → VPN DNS through psw0 (engine DNS DNAT'd)   │
│   ├─ adopt socket /run/popcornstash/supervisor.sock (re-own finished torrents)   │
│   │                                                                              │
│   ├─ popcornstash serve           PUID:PGID, no capabilities                     │
│   │    API + embedded web UI, passkeys, library, sorter, ffmpeg, web downloads   │
│   │    reaches the engine on 127.0.0.1:9091                                      │
│   │                                                                              │
│   └─ popcornstash torrent-engine  STASH_TORRENT_UID:PGID, no capabilities,       │
│        Landlock: writes only Downloads/In Progress and its state directory       │
│        librqbit with every socket bound to psw0; API 127.0.0.1:9091;             │
│        CONNECT proxy 127.0.0.1:9092 (web downloads through the VPN)              │
└──────────────── /config (appdata) ─────────────── /stash (library root) ────────┘
```

The web app's own traffic (sign-in, the NordVPN API, web downloads unless they use the proxy) uses the normal network, so the UI stays reachable on the LAN whatever the VPN does. Only the engine's user is routed into the tunnel. When the tunnel is down the engine has no route and the kill switch drops its packets.

Isolation inside one container: the engine runs as its own user with no capabilities and cannot read the database, caches, secrets, VPN files or partial uploads (they are private to `PUID`: files 0600, folders 0700, so sharing the group `PGID` does not open them). Where the kernel supports Landlock it can only write to `Downloads/In Progress` and its state directory, and read nothing of the stash or `/config` beyond them; without Landlock the library is protected only by its own file permissions (docs/security.md). The web server runs as `PUID:PGID` with no capabilities. Only the supervisor keeps root, and it does not parse network input apart from DNS packets on loopback and one-line requests on its owner-only Unix socket.

Degraded modes, each shown in the UI as a plain sentence:
- Started without `NET_ADMIN` or `/dev/net/tun`: the supervisor runs only `serve`; torrents are unavailable ("Torrents need the container to have NET_ADMIN and /dev/net/tun.").
- Started as a non-root user (`--user`): `run` behaves like `serve`; torrents are unavailable.

### Binary subcommands (`crates/popcornstash`)

| Command | Runs as | Purpose |
|---|---|---|
| `run` | root (default CMD) | Supervisor: VPN, routing, kill switch, DNS forwarder, adopt socket. Spawns `serve` and `torrent-engine` with dropped privileges and restarts them |
| `serve` | PUID:PGID | Web app, API, library, media, downloads. Also runs alone for development (torrents disabled) |
| `torrent-engine` | engine user | librqbit + internal API + CONNECT proxy. Spawned by `run` only |
| `admin reset-owner` | `docker exec` | Prints a one-time owner recovery link (lost passkeys) |
| `admin setup-code` | `docker exec` | Prints a new first-run setup code (the one in the log stops working) |
| `healthcheck` | Docker HEALTHCHECK | Exit 0 when the web server's `/api/health` answers |

### Crates

| Crate | Responsibility |
|---|---|
| `stash-core` | Config, DB pool + migrations, models, library scanner/watcher/classifier/organizer, media probe/thumbnail/transcode, job queue, filename parser |
| `stash-server` | axum router, auth (webauthn-rs), sessions/tokens, security middleware, API handlers, download manager, uploads, engine client, supervisor client, NordVPN API client, SSE events, embedded UI |
| `stash-torrentd` | Supervisor (VPN config parsing, tunnel bring-up, policy routing, nftables kill switch, DNS forwarder, adopt socket, privilege drop, child processes) and the engine (librqbit, Landlock, internal API, CONNECT proxy) |
| `popcornstash` | clap CLI wiring the above |

## 2. Configuration

Environment variables:

| Variable | Default | Meaning |
|---|---|---|
| `STASH_ROOT` | `/stash` | The PopcornStash folder (any directory or Unraid share) |
| `STASH_CONFIG` | `/config` | Appdata: database, caches, secrets, torrent state |
| `STASH_PUBLIC_URL` | none | `https://stash.example.com`. Required for passkeys: sets the WebAuthn origin and RP ID. `http://localhost:8788` allowed for local testing |
| `STASH_EXTRA_ORIGINS` | none | Comma-separated extra allowed origins (same RP ID) |
| `STASH_BIND` | `0.0.0.0:8788` | Listen address |
| `STASH_TLS_CERT`, `STASH_TLS_KEY` | none | Optional PEM files for built-in HTTPS (rustls) |
| `STASH_TRUST_PROXY` | `false` | Use `X-Forwarded-For` for client IPs (rate limits, audit) |
| `STASH_HWACCEL` | empty | `none`, `vaapi`, `qsv`, `nvenc`. When set (not empty) it overrides the saved `transcode.hwaccel`: `GET /api/settings` shows its value and `PUT` refuses any other `transcode.hwaccel` with `bad_request` (the saved choice is kept). Empty or unset: the saved setting applies |
| `STASH_LOG` | `info` | tracing filter |
| `PUID`, `PGID` | `99`, `100` | User and group for the web server and for every file PopcornStash creates (Unraid nobody:users) |
| `STASH_TORRENT_UID` | `6881` | User the torrent engine runs as. Must differ from `PUID` and 0 |
| `STASH_ALLOW_NO_VPN` | `false` | Only for users whose router already tunnels everything. The engine runs without a tunnel; every VPN surface shows the permanent warning "No VPN: torrents use this network", even when a VPN is saved (it is not used, and its setup forms are hidden) |

Set by the supervisor for its children (not for users): `STASH_TORRENTD_URL=http://127.0.0.1:9091`, `STASH_TORRENTD_PROXY=http://127.0.0.1:9092`, `STASH_INTERNAL_TOKEN` (random per start), `STASH_SUPERVISOR_SOCKET=/run/popcornstash/supervisor.sock`, `STASH_TUNNEL_IF=psw0` (engine only), and in degraded mode `STASH_TORRENTS_OFF_REASON` (the sentence the UI shows). When `serve` runs without the supervisor these are unset and torrents are disabled. Children get `HOME=/tmp` (web server) or the engine state dir; the web server also gets the GPU render device's group as its only supplementary group when `/dev/dri/renderD128` exists (never group 0).

Persistent settings that the UI edits live in the `settings` table (section 6).

## 3. Folder layout

Created at startup if missing. Names are fixed.

```
<STASH_ROOT>/
  Movies/                     Movies/Title (Year)/Title (Year).ext
  Shows/                      Shows/Show Name/Season 01/Show Name - S01E02.ext
  Home Videos/                Home Videos/2024/06/original-name.ext
  Photos/                     Photos/2024/06/original-name.ext
  Other/                      Other/original-name.ext
  Inbox/                      Drop zone: anything placed here is sorted
  Downloads/
    In Progress/              Torrents and web downloads while incoming (the engine may write here)
    Completed/                Finished, waiting for the sorter
  .popcornstash/
    uploads/                  Partial browser uploads (<id>.part; folder 0700, files 0600)
    Trash/                    Soft-deleted items (30-day retention)
    stash.id                  Marker file (UUID) so a wrong mount is detected
<STASH_CONFIG>/
  popcornstash.db             SQLite (WAL); the database and its -wal/-shm files are 0600
  secrets/                    Reserved for key material (0700)
  pending-torrents/<id>.json  Torrents waiting for the VPN (folder 0700, files 0600)
  cache/                      0700: thumbnails, renditions and live transcodes are the family's pictures
  cache/thumbs/ab/<item>-sm.webp, -lg.webp
  cache/renditions/<item>.webp     Browser-friendly photo renditions (HEIC/RAW/TIFF)
  cache/transcode/<session>/       Live HLS segments (deleted on session end)
  torrent/
    vpn/vpn.json              { provider, protocol, label, server, country, created_at }   written by serve (0600)
    vpn/wireguard.conf        wg-quick format (0600)
    vpn/openvpn.ovpn          (0600)
    vpn/openvpn.auth          username\npassword (0600)
    engine/                   librqbit persistence and fastresume (engine user, 0700)
/run/popcornstash/            tmpfs, created by the supervisor
  status.json                 Tunnel state, written by the supervisor, read by the engine
  supervisor.sock             Adopt socket (owner PUID, 0600)
```

Ownership: `/config` and everything `serve` writes belong to `PUID:PGID`. The database (0600), `cache/`, `secrets/`, `torrent/vpn/` and `pending-torrents/` (0700) are narrowed to `PUID` on every start, because the engine shares the group `PGID`. `Downloads/` is created `PUID:PGID` 0755 (the engine may enter it, not write it). `Downloads/In Progress` is `PUID:PGID` mode 2775 so the engine (primary group `PGID`, umask 0002) can create torrent folders the web server can move. Nothing the web server writes for itself lives in `In Progress`: partial uploads are in `.popcornstash/uploads/`, and a web download's `<name>.part` there is 0600 and only resumed, written or moved on while it is a regular file owned by `PUID` (anything else at the name is removed and the download starts over). Uploads left in the old `In Progress/.uploads/` are moved over on start (regular files owned by `PUID` only). When a torrent finishes, `serve` asks the supervisor to adopt its folder: the supervisor changes ownership of that tree to `PUID:PGID` without following symlinks (an `openat`/`O_NOFOLLOW` walk that stays on one filesystem), so library files never end up owned by the engine user.

### Movement rules (safety)
- Only files under `Inbox/` and `Downloads/Completed/` are moved automatically.
- Everything else is indexed in place, except the rest of `Downloads/` (anything outside `In Progress/` and `Completed/`, such as another download client's folders), which is never moved and never indexed. Category comes from the top-level folder (`Movies/` → movie, `Shows/` → episode, `Home Videos/` → home_video, `Photos/` → photo, `Other/` → other). Files in the owner's own folders are classified heuristically and never moved without an explicit "Organize" action.
- Moves use `rename(2)`; across filesystems, copy + fsync + verify size + remove.
- Name collisions append ` (2)`, ` (3)` before the extension. Nothing is overwritten.
- Sidecar files with the same stem (`.srt`, `.ass`, `.vtt`, `.sub`, `.idx`, `.nfo`, `.xmp`, `.aae`) move with their media file.
- Partial files (`.part`, `.crdownload`, `.!qB`, `.tmp`, `~$*`), hidden files and `.popcornstash/` are ignored.
- A file is stable when size and mtime are unchanged for 10 seconds (copies over SMB take time).

## 4. Classification and sorting

`stash-core::library::classify(path, probe, exif) -> Classification { category, title, year, show, season, episode, episode_title, taken_at, confidence: High|Low, reason }`

Kinds by extension:
- video: mkv mp4 m4v mov avi webm ts m2ts mts wmv flv 3gp mpg mpeg vob ogv
- photo: jpg jpeg png heic heif webp gif tif tiff avif dng cr2 cr3 nef arw orf rw2 raf
- audio: mp3 m4a flac wav aac ogg opus (category other, sorted to `Other/` with High confidence)
- everything else: other

Rules, first match wins:
1. **Photo** → `Photos/YYYY/MM/`. Date from EXIF DateTimeOriginal, else filename date (`20240613`, `2024-06-13`, `IMG_20240613_...`), else mtime. Confidence High (pictures that may be artwork excepted, see below).
2. **Episode**: `S01E02`, `s1e2`, `1x02`, `Season 1 Episode 2`, `S01E02E03` (first episode). Show name = text before the marker with separators normalized, release tags removed. → `Shows/Show/Season 01/Show - S01E02.ext`. High when show name non-empty.
3. **Home video**: camera metadata (ffprobe tags `com.apple.quicktime.make`, `make`, `model`, `com.android.version`, handler `GoPro`, encoder `DJI`) or camera filenames (`IMG_####.MOV`, `VID_YYYYMMDD_*`, `PXL_*`, `MVI_*`, `GOPR*`, `GH0*`, `GX0*`, `DJI_*`, `DSC*`, `MOV_*`, `Screen Recording*`, `RPReplay*`). Date from `creation_time` tag, filename, mtime. → `Home Videos/YYYY/MM/`. High.
4. **Movie**: title followed by a year 1900–2099 (`Title.2019.1080p...`, `Title (2019)`), release tags stripped (resolution, source, codec, group). → `Movies/Title (Year)/Title (Year).ext`. High when title non-empty and the year is plausible; Low when a year is missing but release tags exist.
5. Anything else → `Other/`. Low.

Samples (`sample` in name and under 150 MB) and extras inside torrent folders are skipped and left in `Completed/`.

What a sort looks at (one top-level entry of `Inbox/` or `Downloads/Completed/`):
- A file that is not a video, photo or song and arrives on its own (an upload, a web download, a document copied into Inbox, or a folder with no media) follows rule 5: it waits in Inbox, suggested for `Other/`. A sidecar whose media file sits next to it is not sorted on its own; it moves with that file.
- In a folder that holds a video, photo or song, the other files stay where they are. Release clutter (`.nfo .sfv .url .md5 .sha1 .sha256 .sha512 .par2 .webloc .lnk`) is never sorted out of a folder.
- In a folder that holds a video or song, or whose name is a release name, pictures that are artwork stay where they are. Artwork means names like `poster`, `cover`, `folder`, `fanart`, `banner`, `thumb`, `screen01`, `<title>-poster`, `www.…`, or anything in `Screens/`, `Screenshots/`, `Artwork/`, `Scans/`, `Covers/` and similar folders.
- Other pictures in a Completed download that holds a video or song wait in Inbox, suggested for `Other/` (Low). Artwork-like pictures in other folders wait for review too.
- A single picture (the extension's "Send image") and a download of only pictures follow rule 1.
- A download folder is judged together with what earlier sorts took out of it (the items linked to its download).

Auto-sort (setting `auto_sort`, default on) moves High-confidence results immediately. Low-confidence results become items with `status = needs_review`, stay in place, and appear in Inbox with the suggested destination prefilled. The owner can accept, edit and accept, or ignore (marks it reviewed and leaves it in `Completed/`).

## 5. Processes and pipelines

### Ingest pipeline
```
torrent / web link / upload / Chrome / Android share
        │
        ▼
Downloads/In Progress/<name>      downloads row: state downloading
        │ complete
        ▼
Downloads/Completed/<name>        state completed → enqueue sort job
        │ sort job (per media file)
        ▼
library folder                    items row (status ready | needs_review), probe + thumb jobs
        ▲
Inbox/<name> ── watcher (stable) ─┘
```

### Jobs
A DB-backed queue (`jobs` table) with a worker pool: `scan`, `probe`, `thumb`, `sort`. Concurrency: probe 2, thumb 2, sort 1, scan 1. Failed jobs retry with backoff (3 attempts). Jobs survive restarts.

### Watcher
`notify` (inotify) on `STASH_ROOT` plus a full rescan every 30 minutes and on startup (inotify is unreliable on some network shares). Rescan reconciles: new files → items; vanished files → `status = missing` (kept 7 days, then removed); moved files are matched by `(size, quick_hash)` so favorites and progress survive. `quick_hash` = xxh3-64 of size + first 1 MiB + last 1 MiB.

### Media
- **Probe:** `ffprobe -v error -print_format json -show_format -show_streams`. Stored summary in `items.media_json` (section 6).
- **Thumbnails:** video frame at 10% (min 3s) via `ffmpeg -ss ... -frames:v 1 -vf scale=...`; photos via `vipsthumbnail --rotate` (HEIC, RAW, TIFF supported). Sizes: `sm` 400px long edge, `lg` 1280px. WebP q80. libvips runs with `VIPS_BLOCK_UNTRUSTED=1` (only its JPEG, PNG, WebP, GIF, TIFF and HEIF loaders) except for a file with a camera RAW name (`dng cr2 cr3 nef arw orf rw2 raf`) that also starts like a RAW format only ImageMagick reads (ORF, RW2, RAF, CR3); TIFF-based RAW files (DNG, CR2, NEF, ARW) and anything else under a RAW name get the trusted loaders only. The image's ImageMagick policy allows only the RAW coders (plus JPEG and TIFF), no delegates, filters or `@file` reads. ffmpeg always reads `file:<path>` (its default protocol whitelist then stays `file,crypto,data`, so a playlist inside a hostile file cannot make it fetch URLs). Each image is written to a hidden `.<name>.<16 hex>.tmp.webp` next to its place and renamed when complete; a cancelled conversion (a photo view the browser closes) removes it, and leftovers in `cache/thumbs` and `cache/renditions` are removed on start.
- **Photo renditions:** browsers get the original for jpg/png/webp/gif/avif; HEIC/RAW/TIFF get a cached 2560px WebP.
- **Playback decision:** client sends codec support. Direct play when container is mp4/m4v/mov/webm, video codec is supported (h264 always; hevc/av1/vp9 when the client says so), audio is aac/mp3/opus/flac (or absent), and the file is not HDR for an SDR-only client. Otherwise HLS. The web player plays HLS with hls.js wherever Media Source Extensions work and uses the browser's own HLS only where they do not, so a resumed stream starts at the resume segment.
- **HLS transcoding:** session per playback. A full VOD playlist with fixed 6s segments is generated up front. ffmpeg runs from the requested segment (`-ss`, `-force_key_frames expr:gte(t,n_forced*6)`, `-start_number`, `-output_ts_offset`), H.264 (libx264 veryfast CRF 23, or `h264_vaapi`/`h264_qsv`/`h264_nvenc`) + AAC stereo 160k, fMP4 or MPEG-TS segments in `cache/transcode/<session>/`. A segment request outside the produced range (behind the start, or more than 3 segments past the newest) restarts ffmpeg at that segment; requests just ahead wait up to 20s for the segment to appear. ffmpeg runs ahead without throttling in v1. Sessions idle for 60s are killed and their folder deleted. HDR sources are tone mapped to SDR (zscale + tonemap hable). Max 2 concurrent transcodes (setting). There is no remux path yet: every HLS stream re-encodes the video, also when only the container or the audio needed converting (an H.264 MKV, an MP4 with DTS audio).

### Web downloads
reqwest streaming into `In Progress/<name>.part` (0600), resume with `Range`, retry 3× with backoff, concurrency 3. Filename from `Content-Disposition`, else URL path, sanitized. Optional request headers from the extension (`Cookie`, `Referer`, `User-Agent`) are held in memory only and never written to the DB or logs. The forwarded `Cookie` never follows a redirect to another origin (scheme, host and port): reqwest drops it for direct downloads, and redirects followed by hand through the VPN proxy (downloads and `.torrent` links) drop it the same way for the rest of the chain. Setting `http_via_vpn`: `never` | `when_available` (default) | `always` (fails if the VPN is down). Magnet links and `.torrent` URLs are handed to the torrent client.

### Torrents (engine process, web server client)
- Adding a torrent while the VPN is down creates the download in `queued` with the message "Waiting for the VPN to connect"; the poller adds it to the engine once the engine is `running`. With torrents disabled (`STASH_TORRENTD_URL` unset) the API answers `vpn_unavailable`.
- The web server polls `GET /torrents` every 2s while any torrent is active (10s otherwise) and mirrors state into `downloads`.
- The engine downloads into `<STASH_ROOT>/Downloads/In Progress/<torrent name>` (same path in both processes).
- When a torrent finishes: if `seed_after_complete` is off (default), the web server calls `DELETE /torrents/:id?delete_files=false`, asks the supervisor to adopt the folder (section 3, Ownership), moves it to `Completed/` and calls `Library::enqueue_sort` (download state `sorting`; the sorter sets `sorted` and links `download_items`). If on, it seeds until `seed_ratio` (default 1.0) or `seed_hours` (default 24), then the same.
- **Torrents at once** (`torrent.max_active`, 1-50, default 5): at most that many torrents download at once; seeding and paused torrents take no slot. Extra torrents stay `queued` with `error` "Waiting for a free slot (N at once)." and start oldest first when a slot frees. The poller reads the setting on every poll, so a change applies without a restart: a raised limit starts waiting torrents, a lowered one pauses the newest running torrents in the engine and puts them back in line. Resuming a paused torrent while every slot is taken puts it back in line.
- DHT, PEX, uTP enabled. LSD disabled. Upload/download limits from settings.

### VPN (supervisor, `popcornstash run`)
Three independent layers keep torrent traffic in the tunnel: policy routing by user, an nftables kill switch by user, and librqbit binding every socket to `psw0`. DNS is forced through the tunnel as well.

Let `E` = `STASH_TORRENT_UID`, mark `0x5150`, routing table `51820`.

1. **Preflight.** Running as root, `NET_ADMIN` usable (an nftables dry run succeeds), `/dev/net/tun` present, `PUID` and `E` distinct and non-zero. Otherwise degraded mode (section 1).
2. **Prepare.** Random internal token (32 bytes hex) kept in memory. `/run/popcornstash` (0755). Ownership per section 3: `/config` tree `PUID:PGID`, `/config/torrent/engine` `E:PGID` 0700, `Downloads/In Progress` `PUID:PGID` 2775.
3. **Base policy, before any child starts** (IPv4 and IPv6 where applicable):
   - `ip rule add priority 100 uidrange E-E lookup 51820` and `ip rule add priority 101 uidrange E-E prohibit`. With no tunnel, table 51820 is empty and the engine has no route at all.
   - `ip rule add priority 99 fwmark 0x5150 lookup 51820` and `ip rule add priority 102 fwmark 0x5150 prohibit` for the supervisor's own DNS forwarder.
   - nftables table `inet popcornstash` (replaced atomically, Docker's tables untouched):
     - `nat_output` (type nat, hook output, priority -110): `meta skuid E udp/tcp dport 53 → dnat ip to 127.0.0.54:5354`.
     - `output` (filter, priority 0, policy accept): for `meta skuid E`: accept `oifname "lo"` with `ct state established,related`; accept `oifname "lo"` to `127.0.0.54` port 5354; accept `oifname "psw0"`; drop everything else (IPv4 and IPv6).
     - `input` (filter, policy accept): `iifname "psw0"` accept only `ct state established,related`, TCP and UDP 6881 (peers, uTP) and UDP 6882 (DHT); drop the rest, so nothing on the VPN side can reach the web server.
     - Connections that use the tunnel are tagged; a later packet of a tagged connection leaving through another interface is dropped.
     - `meta mark 0x5150 oifname != "psw0" drop`, so the DNS forwarder's marked queries can never reach Docker's resolver or the LAN.
     - A rule guard re-checks the rules and table every 5 s and re-applies them if something removed them.
4. **DNS forwarder** on `127.0.0.54:5354` (UDP and TCP) inside the supervisor. Upstream sockets carry mark `0x5150`, so they can only leave through `psw0`; with the tunnel down every query answers SERVFAIL. Upstreams: WireGuard `DNS=`, OpenVPN pushed DNS, else NordVPN 103.86.96.100 / 103.86.99.100.
5. **Tunnel** from `/config/torrent/vpn` (absent → no tunnel; the engine stays blocked):
   - **WireGuard:** `ip link add psw0 type wireguard`; if the kernel lacks WireGuard, spawn bundled `boringtun-cli --foreground --disable-drop-privileges psw0` and wait for its UAPI socket. `wg setconf psw0 <stripped conf>`, addresses always as /32 or /128 (a /24 would add a main-table route and pull the web server into the tunnel), the file's MTU or 1420, link up. `PersistentKeepalive = 25` is added when missing; the first handshake gets 45 s. The encrypted UDP comes from the kernel or the root-owned boringtun process, so it uses the normal routes.
   - **OpenVPN:** the validated config and credentials are written to root-only copies in `/run/popcornstash/openvpn/` (0700/0600) and OpenVPN reads only those: `openvpn --config <copy> --auth-user-pass <copy> --auth-nocache --dev psw0 --dev-type tun --route-noexec --ifconfig-noexec --pull-filter ignore redirect-gateway --pull-filter ignore route-ipv6 --pull-filter ignore ifconfig-ipv6 --script-security 2 --up '/bin/sh <hook>' --down '/bin/sh <hook>' --verb 3 --suppress-timestamps`. The hook sets the pushed address as a /32, the MTU and link up, and records pushed DNS. OpenVPN never changes the main routing table. The supervisor refuses the same directives as the web validator, plus `<connection>` blocks, and requires `client`.
   - Then `ip route replace default dev psw0 table 51820`.
6. **Children.** `popcornstash serve` as `PUID:PGID` and `popcornstash torrent-engine` as `E:PGID` (umask 0002), both with supplementary groups cleared, `PR_SET_NO_NEW_PRIVS`, and the environment from section 2. The engine applies Landlock to itself (best effort) and binds librqbit to `psw0`.
7. **Monitor** every 5s: WireGuard latest-handshake < 180s (boringtun reports no handshake once the last keys expire; such a gap counts as connected only until 180 s after the last handshake seen), OpenVPN process alive. Write `/run/popcornstash/status.json` (`TunnelStatus`). On failure the rules stay in place; tear down and retry with backoff (5s → 5 min). A crashed child restarts after 2s (backoff up to 1 min).
8. **Reload.** The engine exits with code 75 when the web server calls `POST /vpn/reload`; the supervisor tears the tunnel down, re-reads the files and repeats steps 5–6 for the engine only.
9. **Adopt socket** `/run/popcornstash/supervisor.sock` (owner `PUID`, 0600; peer uid checked with `SO_PEERCRED`). One JSON line per request: `{"op":"adopt","name":"<torrent folder or file name>"}` → `{"ok":true}` or `{"ok":false,"error":"..."}`. `name` is a single path component under `Downloads/In Progress`.
10. **Shutdown** on SIGTERM: stop children, tear down the tunnel, remove the rules.

`STASH_ALLOW_NO_VPN=true` skips steps 3 to 5 for the engine (no rules, no tunnel) and the engine runs unblocked; the UI shows a permanent warning. The status then always reports `state: "not_configured"` with `allow_no_vpn: true`, also with a saved VPN (`configured: true`) and while the engine restarts (a fresh supervisor status file with the kill switch off marks this mode).

Proof of protection: the engine checks its public IP through the tunnel every 10 minutes (NordVPN `https://api.nordvpn.com/v1/helpers/ips/insights` → `{ip, country, protected}`; other providers `https://api.ipify.org?format=json`). The service addresses are fixed in production and injectable in tests. When NordVPN answers `protected: false`, the engine stops torrents (engine `blocked`, reason "The VPN is connected, but the public address is not a VPN address. Torrents stay stopped until a later check passes.") and checks again every 60 s until a check no longer says `protected: false`.

### NordVPN setup (web server)
- `GET https://api.nordvpn.com/v1/users/services/credentials` with basic auth `token:<access token>` → `nordlynx_private_key`, `username`, `password` (service credentials).
- Countries: `GET https://api.nordvpn.com/v1/servers/countries` (cached 24h).
- Server: `GET https://api.nordvpn.com/v1/servers/recommendations?filters[servers_technologies][identifier]=wireguard_udp&filters[country_id]=<id>&limit=1` → `hostname`, `station`, technology metadata `public_key`.
- WireGuard file: `Address = 10.5.0.2/32`, `DNS = 103.86.96.100, 103.86.99.100`, `Endpoint = <station>:51820`, `AllowedIPs = 0.0.0.0/0`, `PersistentKeepalive = 25`.
- OpenVPN: the user uploads the `.ovpn` from nordvpn.com and the service credentials (or uses the token, which also returns them).
- The access token is used once and never stored.

## 6. Database (SQLite, `crates/stash-core/migrations`)

All ids are UUIDv7 strings unless noted. Times are unix seconds (INTEGER).

```sql
users(id PK, display_name, role CHECK(role IN ('owner','member')), webauthn_id UNIQUE, created_at)
passkeys(id PK, user_id → users ON DELETE CASCADE, name, credential_id UNIQUE, passkey_json, created_at, last_used_at)
sessions(id PK /* sha256(token) hex */, user_id, kind CHECK(kind IN ('cookie','bearer')), created_at, last_seen_at, expires_at, user_agent, ip)
api_tokens(id PK, user_id, name, token_hash UNIQUE, prefix, scopes /* JSON array */, client_kind, created_at, last_used_at, expires_at NULL)
invites(id PK, code_hash UNIQUE, role, created_by, created_at, expires_at, used_at NULL, used_by NULL)
recovery_codes(id PK, code_hash UNIQUE, kind CHECK(kind IN ('setup','owner_reset')), created_at, expires_at, used_at NULL)
pairing_requests(id PK, user_code UNIQUE, poll_secret_hash, client_name, client_kind, ip, status CHECK(status IN ('pending','approved','denied','delivered')), token_id NULL, pending_token NULL /* plaintext until first poll, then NULL */, approved_by NULL, created_at, expires_at)
audit_log(id INTEGER PK AUTOINCREMENT, at, user_id NULL, event, detail /* JSON */, ip)
settings(key PK, value /* JSON */)
items(id PK, rel_path UNIQUE, kind, category, status CHECK(status IN ('ready','needs_review','missing','trashed')),
      title, sort_title, year, show_name, season, episode, episode_title,
      size, mtime, quick_hash, mime, duration_s REAL, width, height, media_json, taken_at, camera,
      thumb_state CHECK(thumb_state IN ('pending','ready','failed','none')), suggestion_json NULL,
      favorite INTEGER DEFAULT 0, source_download_id NULL, added_at, updated_at)
item_progress(user_id, item_id, position_s REAL, duration_s REAL, finished INTEGER, updated_at, PRIMARY KEY(user_id, item_id))
downloads(id PK, kind CHECK(kind IN ('torrent','http','upload')), name, source /* URL without query secrets, magnet, or filename */,
          state CHECK(state IN ('queued','downloading','paused','seeding','completed','sorting','sorted','failed','cancelled')),
          progress REAL, total_bytes, done_bytes, down_bps, up_bps, peers, eta_s, error, rel_path, info_hash, engine_id,
          created_by, created_at, updated_at, completed_at, sorted_at)
download_items(download_id, item_id, PRIMARY KEY(download_id, item_id))
uploads(id PK, user_id, filename, size, received, mime, last_modified, state CHECK(state IN ('receiving','complete','cancelled')), created_at, updated_at)
jobs(id INTEGER PK AUTOINCREMENT, kind, item_id NULL, payload /* JSON */, state CHECK(state IN ('queued','running','done','failed')), attempts, error, run_after, created_at, updated_at)
trash(item_id PK, original_rel_path, trashed_rel_path, trashed_at, trashed_by)
```

`media_json` summary:
```json
{"container":"mov,mp4,m4a,3gp,3g2,mj2","duration_s":63.2,"bitrate":12000000,
 "video":{"codec":"hevc","profile":"Main 10","pix_fmt":"yuv420p10le","width":3840,"height":2160,"fps":29.97,"hdr":"hlg","rotation":90},
 "audio":[{"index":1,"codec":"aac","channels":2,"language":"und","title":null}],
 "subtitles":[{"index":2,"codec":"subrip","language":"eng","title":null,"external":false}],
 "tags":{"make":"Apple","model":"iPhone 15 Pro","creation_time":"2024-06-13T10:21:00Z"}}
```

`container` is null when ffprobe reports no format name.

Settings keys and defaults:
```json
{"auto_sort": true, "http_via_vpn": "when_available", "max_concurrent_downloads": 3,
 "torrent": {"seed_after_complete": false, "seed_ratio": 1.0, "seed_hours": 24, "max_active": 5, "down_limit_kbps": 0, "up_limit_kbps": 0},
 "transcode": {"hwaccel": "none", "max_height": 1080, "crf": 23, "max_sessions": 2},
 "trash_retention_days": 30}
```

## 7. Security model

- **Passkeys only.** No passwords. webauthn-rs, user verification required, discoverable credentials (passkeys) so sign-in needs no username. RP ID = host of `STASH_PUBLIC_URL`.
- **First run:** on startup with zero users, a setup code (`XXXX-XXXX-XXXX`, 60 bits, valid 24h, regenerated on restart until used) is printed to the log. Registering the owner requires it, so nobody else on the network can claim a fresh install.
- **Recovery:** `docker exec popcornstash popcornstash admin reset-owner` prints a one-time link valid 30 minutes that lets the owner register a new passkey.
- **Invites:** owner creates invite links (role member or owner, default expiry 72h, single use).
- **Sessions:** 256-bit random token in cookie `__Host-stash` (`Secure` when the public URL is https, `HttpOnly`, `SameSite=Strict`, `Path=/`). Stored as sha256. 30-day sliding expiry, absolute max 180 days. Cookie name falls back to `stash` (no `__Host-` prefix) on http://localhost.
- **Bearer tokens:** `pst_` + 43 base64url chars. Stored as sha256 with an 8-char display prefix. Scopes: `full`, `downloads:create`, `downloads:read`, `library:read`. Created by device pairing (Chrome → `downloads:create downloads:read`; Android → `full`; `client_kind: "other"` → `downloads:create` only) or manually in Settings → Devices.
- **Bearer sessions:** `POST /api/auth/login/finish` with `want_token: true` returns a bearer session, not a device token: a `sessions` row of kind `bearer` with the user's full access, the same 30-day sliding expiry and 180-day absolute limit as the cookie, listed under `/api/me/sessions` (not `/api/tokens`) and ended by logging out with it or deleting the session. The token is sent as `Authorization: Bearer`, like a device token.
- **CSRF:** unsafe methods (POST/PUT/PATCH/DELETE) under `/api` without a bearer header require `Origin` (or `Referer`) to match an allowed origin, and a body, when present, must be `application/json`, `multipart/form-data` or `application/octet-stream`. Bearer requests skip the check. `POST /api/pairing/request` and `/api/pairing/poll` are exempt (the Chrome extension sends a `chrome-extension://` origin and native apps send none); they never read the cookie.
- **Rate limits:** auth start/finish, pairing request/poll and invite lookups: 10/min per client, burst 20. IPv6 clients are grouped by /64. With `STASH_TRUST_PROXY`, the client address is the last `X-Forwarded-For` entry (the one the trusted proxy appended). Failed auth is audit-logged.
- **Headers:** CSP `default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Permissions-Policy: camera=(), microphone=(), geolocation=()`, `Cross-Origin-Opener-Policy: same-origin`, HSTS when https.
- **Paths:** every filesystem path from a request is resolved under its root and rejected if it escapes (canonicalize + prefix check, no symlink escape). The check fails closed: a link whose target is missing, or a path that cannot be resolved, is refused too. Uploaded filenames are sanitized (no separators, no control chars, no leading dots, max 200 bytes).
- **Internal API:** the engine listens on 127.0.0.1 only and requires `Authorization: Bearer <internal token>`; the token is 32 random bytes hex, generated by the supervisor at each start and passed to both children in the environment, never written to disk.
- **Secrets at rest:** VPN files 0600; the database 0600; `cache/`, `secrets/`, `pending-torrents/` and `.popcornstash/uploads/` 0700. Device token plaintext is shown once (pairing delivers it once, then the row is wiped).
- **Logs:** request spans (debug level) carry the method and the path only; invite and recovery codes in paths are replaced with `<redacted>` and query strings are never logged.
- **Engine answers:** the web server reads at most 8 MiB from the engine and passes on at most 300 characters (one line) of any engine message, so a compromised engine cannot exhaust its memory or put a long text in front of people.
- **Roles:** owner can do everything. Member can browse, play, upload, add downloads, review Inbox, manage own passkeys/sessions/devices. Owner-only: VPN, settings, people/invites, trash emptying, deleting items, audit log.

## 8. HTTP API

JSON, snake_case. Errors: `{"error": {"code": "not_found", "message": "Human readable sentence."}}` with matching HTTP status. Codes: `bad_request`, `unauthorized`, `forbidden`, `not_found`, `method_not_allowed` (405), `conflict`, `payload_too_large` (413), `rate_limited`, `setup_required`, `passkeys_unavailable`, `vpn_unavailable`, `internal`.

Lists use cursor pagination: `?limit=50&cursor=<opaque>` → `{"items": [...], "next_cursor": "..." | null}`.

### Shared shapes
```ts
User { id, display_name, role: "owner"|"member", created_at }
Passkey { id, name, created_at, last_used_at|null }
Session { id, kind, created_at, last_seen_at, user_agent|null, ip|null, current: bool }
Token { id, name, prefix, scopes: string[], client_kind, created_at, last_used_at|null }
Item {
  id, kind: "video"|"photo"|"audio"|"other", category: "movie"|"episode"|"home_video"|"photo"|"other",
  status: "ready"|"needs_review"|"missing"|"trashed", title, year|null, show_name|null, season|null, episode|null,
  episode_title|null, rel_path, size, duration_s|null, width|null, height|null, taken_at|null, camera|null,
  thumb: string|null /* URL */, favorite: bool, added_at, progress: {position_s, duration_s, finished}|null
}
ItemDetail = Item & { media: MediaSummary|null, mime|null, suggestion: Classification|null, source_download_id|null,
  next_episode_id|null, previous_episode_id|null }
Classification { category, title, year, show_name, season, episode, episode_title, taken_at, confidence: "high"|"low", reason, destination: string /* rel path */ }
Download { id, kind: "torrent"|"http"|"upload", name, source, state, progress, total_bytes|null, done_bytes,
  down_bps, up_bps, peers|null, eta_s|null, error|null, created_at, completed_at|null, item_ids: string[] }
VpnStatus { configured: bool, reachable: bool /* torrentd answered */, state: "connected"|"connecting"|"disconnected"|"error"|"not_configured"|"unreachable",
  provider: "nordvpn"|"wireguard"|"openvpn"|null, protocol: "wireguard"|"openvpn"|null, label|null, server|null, country|null,
  public_ip|null, protected: bool|null, since|null, error|null, kill_switch: bool, allow_no_vpn: bool,
  torrents_off: bool /* torrents are off on this server by design (degraded mode or `serve` alone); `error` says why */ }
```

### Public
| Method | Path | Body → Response |
|---|---|---|
| GET | `/api/health` | → `{status:"ok", version, source}`. `source` is `https://github.com/thekozugroup/PopcornStash/tree/v<version>` for a release build (built with `POPCORNSTASH_RELEASE=<version>`), and the repository address otherwise |
| GET | `/api/auth/state` | → `{setup_required, authenticated, user|null, public_url|null, rp_id|null, passkeys_ready: bool, reason|null}` |
| POST | `/api/auth/setup/start` | `{setup_code, display_name}` → `{flow_id, options}` (options = webauthn-rs `CreationChallengeResponse`) |
| POST | `/api/auth/setup/finish` | `{flow_id, credential, passkey_name?}` → `{user}` + cookie |
| POST | `/api/auth/login/start` | `{}` → `{flow_id, options}` (`RequestChallengeResponse`, discoverable) |
| POST | `/api/auth/login/finish` | `{flow_id, credential, want_token?: bool, device_name?}` → `{user, token?}` + cookie unless `want_token` |
| POST | `/api/auth/invite/start` | `{code, display_name}` → `{flow_id, options}` |
| POST | `/api/auth/invite/finish` | `{flow_id, credential, passkey_name?}` → `{user}` + cookie |
| POST | `/api/auth/recover/start` | `{code}` → `{flow_id, options}` |
| POST | `/api/auth/recover/finish` | `{flow_id, credential, passkey_name?}` → `{user}` + cookie |
| GET | `/api/invites/:code` | → `{valid, role, expires_at}` (no auth; reveals nothing else) |
| POST | `/api/pairing/request` | `{client_name, client_kind: "chrome-extension"|"android"|"other"}` → `{pairing_id, user_code, poll_secret, expires_at, verify_url}` |
| POST | `/api/pairing/poll` | `{pairing_id, poll_secret}` → 202 `{status:"pending"}` / 200 `{status:"approved", token, scopes, server_name}` / 410 `{status:"denied"|"expired"}` / 404 `not_found` for an unknown `pairing_id`, a wrong `poll_secret`, or a request purged 24 hours after it expired (until then a late poll gets 410 `expired`) |

### Authenticated (cookie or bearer)
| Method | Path | Scope | Body → Response |
|---|---|---|---|
| POST | `/api/auth/logout` | any | → 204 |
| GET | `/api/me` | any | → `User` |
| GET/POST | `/api/me/passkeys` (+ `/start`, `/finish`) | full | list → `Passkey[]`; add flow like setup: `POST /api/me/passkeys` or `/start` → `{flow_id, options}`, then `/finish` `{flow_id, credential, passkey_name?}` → `Passkey` (not `{user}`; no new cookie) |
| PATCH/DELETE | `/api/me/passkeys/:id` | full | `{name}`; delete refuses the last passkey (409) |
| GET/DELETE | `/api/me/sessions[/:id]` | full | → `Session[]` |
| GET/POST/DELETE | `/api/tokens[/:id]` | full | create `{name, scopes}` → `Token & {token}` (once) |
| GET | `/api/pairing/pending` | full | → `[{pairing_id, user_code, client_name, client_kind, ip, created_at}]` |
| POST | `/api/pairing/approve` / `/deny` | full | `{user_code}` → `{ok:true}` |
| GET | `/api/library/summary` | library:read | → `{counts:{movies, shows, episodes, home_videos, photos, other}, total_bytes, needs_review, recently_added: Item[] /* up to 12 newest per category, photos included, newest first */, show_posters: {[show_name]: item_id} /* poster of each show with an episode in recently_added */, continue_watching: Item[]}` |
| GET | `/api/items` | library:read | `?category&q&sort=added|title|taken|size&order=asc|desc&favorite&year&status&cursor&limit` → page of `Item`. `status` defaults to `ready`; `status=trashed` is refused (400, the trash has its own route) |
| GET | `/api/items/:id` | library:read | → `ItemDetail` |
| PATCH | `/api/items/:id` | full | `{title?, year?, favorite?, show_name?, season?, episode?, episode_title?}` → `ItemDetail` (metadata only, no move) |
| POST | `/api/items/:id/organize` | full | `{category, title?, year?, show_name?, season?, episode?}` → `ItemDetail` (moves the file; explicit action) |
| DELETE | `/api/items/:id` | owner | → 204 (moves to `.popcornstash/Trash`) |
| GET | `/api/items/:id/thumb?size=sm|lg` | library:read | → `image/webp` (404 when not ready) |
| GET | `/api/items/:id/file?download=1` | library:read | → original bytes, Range supported |
| GET | `/api/items/:id/view` | library:read | → browser-friendly photo (original or rendition) |
| POST | `/api/items/:id/playback` | library:read | `{video_codecs: string[], audio_codecs: string[], hdr: bool, max_height?: number, start_s?: number, audio_index?: number}` → `{mode:"direct", url}` or `{mode:"hls", url, session_id}` |
| GET | `/api/stream/:session/index.m3u8`, `/api/stream/:session/:segment` | library:read | HLS playlist/segments |
| DELETE | `/api/stream/:session` | library:read | → 204 |
| PUT | `/api/items/:id/progress` | library:read | `{position_s, duration_s}` → 204 |
| GET | `/api/shows` | library:read | → `[{show_name, item_count, season_count, poster_item_id, last_added_at}]` |
| GET | `/api/shows/:name` | library:read | → `{show_name, poster_item_id|null, seasons:[{season, episodes: Item[]}]}` |
| GET | `/api/photos/timeline` | library:read | `?cursor&limit` → `{months:[{month:"2024-06", count, items: Item[]}], next_cursor}` |
| GET | `/api/inbox` | library:read | → `{items: ItemDetail[]}` (status needs_review) |
| POST | `/api/inbox/:id` | full | `{action:"accept"|"ignore", category?, title?, year?, show_name?, season?, episode?}` → `ItemDetail` |
| POST | `/api/library/rescan` | full | → 202 `{job_id}` |
| GET | `/api/trash` / POST `/api/trash/:id/restore` / DELETE `/api/trash` | owner | list / restore / empty |
| GET | `/api/downloads` | downloads:read | `?state=active|finished|all&limit` → `{items: Download[]}` |
| POST | `/api/downloads` | downloads:create | `{url, filename?, headers?: {cookie?, referer?, user_agent?}}` (http(s) or magnet) → `Download` |
| POST | `/api/downloads/torrent` | downloads:create | multipart `file` (.torrent ≤ 10 MB) → `Download` |
| POST | `/api/downloads/:id/pause` / `resume` | full | → `Download` |
| DELETE | `/api/downloads/:id?delete_files=bool` | full | → 204 |
| POST | `/api/uploads` | full | `{filename, size, mime?, last_modified?}` → `{id, chunk_size: 16777216, received: 0}` |
| PUT | `/api/uploads/:id?offset=N` | full | raw bytes (≤ 64 MiB) → `{received}`; 409 with `{received}` on offset mismatch |
| GET | `/api/uploads/:id` | full | → `{id, filename, size, received, state}` |
| POST | `/api/uploads/:id/complete` | full | → `Download` (kind upload, goes to `Completed/` then sorted). The file's modification time becomes `last_modified` when that is between 1990-01-01 and now (otherwise it keeps the upload time), so a photo without EXIF is dated by it |
| DELETE | `/api/uploads/:id` | full | → 204 |
| GET | `/api/vpn/status` | any | → `VpnStatus` |
| GET | `/api/vpn/nordvpn/countries` | owner | → `[{id, name, code}]` |
| POST | `/api/vpn/nordvpn` | owner | `{access_token, country_id?, protocol:"wireguard"|"openvpn"}` → `VpnStatus` |
| POST | `/api/vpn/wireguard` | owner | multipart `file` or JSON `{config}` → `VpnStatus` |
| POST | `/api/vpn/openvpn` | owner | multipart `file`, `username`, `password` → `VpnStatus` |
| POST | `/api/vpn/reconnect` | owner | → `VpnStatus` |
| DELETE | `/api/vpn` | owner | → `VpnStatus` |
| GET/PUT | `/api/settings` | owner | the settings object (partial PUT merges) |
| GET | `/api/system` | owner | → `{version, source, stash_root, config_dir, disk:{total, free}, ffmpeg_version, hwaccel:{configured, available: string[], set_by_env}, torrentd:{reachable}, jobs:{queued, running, failed}, public_url, passkeys_ready}`. `source` as in `/api/health`; `configured` is the acceleration in effect; `set_by_env` is true when `STASH_HWACCEL` sets it |
| GET | `/api/users` / PATCH `/api/users/:id` `{role}` / DELETE `/api/users/:id` | owner | |
| GET/POST/DELETE | `/api/invites[/:id]` | owner | create `{role, expires_in_hours}` → `{id, url, code, role, expires_at}` |
| GET | `/api/audit?limit&cursor` | owner | → page of `{id, at, user, event, detail, ip}` |
| GET | `/api/events` | any | SSE: `download` (Download), `library` `{type:"added"|"updated"|"removed", item_id}`, `vpn` (VpnStatus), `inbox` `{count}`, `job` `{kind, state, item_id}` |

Non-API routes: `/` and all client routes serve the SPA (`index.html`, `Cache-Control: no-cache`); `/assets/*` immutable; `/manifest.webmanifest`; `/sw.js`; `/share` (PWA share target: Android posts `multipart/form-data` with `title`, `text`, `url` and `files` to `/share`. The service worker (`sw.js`) answers it. Files are kept in Cache Storage on the device (`popcornstash-shared-files-v1`, deleted once uploaded or cancelled, or after 7 days) and the page opens `/share?files=<id>`. A share without files opens `/share?title&text&url`, and a failure opens `/share?failed=files`. The server serves only GET; a POST that reaches it gets 405.); `GET /third-party-licenses.txt` (the web app's npm notices, image builds); `/.well-known/assetlinks.json` (from `STASH_ANDROID_ASSETLINKS`, future Android passkeys).

## 9. Internal engine API (127.0.0.1:9091)

Bearer internal token on every route except `/livez`. The engine binds to 127.0.0.1 only.

| Method | Path | Body → Response |
|---|---|---|
| GET | `/livez` | No auth. → `ok` (Docker healthcheck) |
| GET | `/health` | → `{vpn: TunnelStatus, engine: {state:"running"|"blocked", reason|null, allow_no_vpn}, public_ip|null, country|null, protected|null, checked_at|null}` |
| GET | `/torrents` | → `[{id, info_hash, name, state:"initializing"|"downloading"|"paused"|"seeding"|"error", progress, total_bytes, done_bytes, uploaded_bytes, down_bps, up_bps, peers, eta_s, error, output_dir}]` |
| POST | `/torrents` | `{magnet?: string, torrent_base64?: string}` → `{id, info_hash, name}` (409 when blocked by VPN state) |
| POST | `/torrents/:id/pause` / `resume` | → 204 |
| DELETE | `/torrents/:id?delete_files=bool` | → 204 |
| PUT | `/limits` | `{down_kbps, up_kbps}` → 204 |
| POST | `/vpn/reload` | → 202, then the engine exits 75 and torrentd re-applies the VPN |
| CONNECT | `127.0.0.1:9092` | HTTP CONNECT proxy, `Proxy-Authorization: Basic base64("stash:<internal token>")`, only to ports 80/443 |

## 10. Clients

- **Web UI** (`web/`): React 19, Vite, TypeScript, Astryx (`@astryxdesign/core` + StyleX via `@astryxdesign/build/vite`) with a custom Popcorn theme. Built into `web/dist` and embedded in the binary (rust-embed). PWA manifest with a `share_target` (POST, multipart, files of any type) so Android Chrome can share links, photos, videos and other files to PopcornStash. Shared files are uploaded through the resumable upload queue after the person confirms. The service worker answers nothing else, and static routes send `/api/*` and `/assets/*` straight to the network.
- **Chrome extension** (`extension/`): Manifest V3. Device pairing (section 8), context menu "Send to PopcornStash" for links/media/magnets, optional "send all browser downloads" with optional cookie forwarding (optional host permissions), popup with recent downloads.
- **Android app:** later. Uses `login/finish` with `want_token` (a bearer session, section 7), device pairing with `client_kind: "android"`, the same API, and `/.well-known/assetlinks.json`. Until then, Android phones use the web app, installed from Chrome, and its share target.

## 11. Behaviour details

Decisions made during implementation that clients rely on.

- **Auth:** `POST /api/auth/login/start` accepts `{conditional?: bool}`; without it the options suit a plain "Sign in" button. The pairing request response adds `interval` (seconds between polls, 6). `PATCH /api/users/:id` also accepts `display_name`. `POST /api/tokens` accepts optional `user_id` (owner only) and `client_kind`. Logging out with a device token revokes that token. Responses carrying secrets send `Cache-Control: no-store`. An approved pairing token nobody collects is revoked 10 minutes after the request expires.
- **Lists:** `GET /api/invites` returns every invite as `{id, role, created_by, created_by_name, created_at, expires_at, used_at, used_by, used_by_name, status: "active"|"used"|"expired"}` (no code or link; only the create response carries those). `GET /api/audit` entries carry `user: {id, display_name|null} | null`. `GET /api/trash` returns `[{item: Item, original_rel_path, trashed_at, trashed_by}]`; restore returns `ItemDetail`; empty returns `{deleted}`.
- **Photo timeline:** `limit` is a photo budget (default 200, at most 500). A busy month can continue on the next page; `count` is always the month's full total.
- **Files:** library files are served with a sandboxed CSP; HTML, SVG, XML and JS are always sent as attachments so a downloaded page cannot run as the signed-in user. Thumbnail URLs with `v=` are immutable; without it they revalidate with ETags.
- **Streams:** session ids are 32 lowercase hex characters. Segment timestamps start at 10 + 6n seconds so segments from restarted ffmpeg runs line up. Videos longer than 72 hours are refused. NVENC/QSV force IDR keyframes; a hardware encoder that fails falls back to libx264 for that encoder until restart.
- **Events:** SSE re-checks the session or token every 60 s and closes the stream once it is gone. `job` events carry `kind` (`scan`, `probe`, `thumb`, `sort`) and `state` (`running`, `done`, `queued` for a retry, `failed`).
- **Library:** rescans refuse to mark anything missing when the stash folder has no files but the database has items (an unmounted share), and they apply `trash_retention_days`. Items marked Ignore in the Inbox are never moved automatically. Media in the owner's own folders carries a suggestion so Organize can be prefilled. Episode items use `title` "Show S01E02" with `episode_title` separate. A show's poster is its first episode in season and episode order that has a thumbnail, else its first episode; `/api/shows`, `/api/shows/:name` and the summary's `show_posters` all use that choice, so a show has one picture everywhere.
- **Media:** rotation is stored clockwise. `taken_at` comes from EXIF or QuickTime capture dates (never the encode date of movies and episodes). The thumbnail job also prepares the browser rendition of HEIC/RAW/TIFF photos.
- **Downloads:** a stored `source` drops credentials, credential-like query keys and fragments; when anything private was removed, or the download depended on forwarded browser cookies, it ends in `#redacted` and cannot be fetched again after a restart ("Send the link again"). Magnet sources keep only the hash and name. Queued torrents carry why they wait in `error`: "Waiting for the VPN to connect." or "Waiting for a free slot (N at once)." Torrents waiting for the VPN are kept in `<config>/pending-torrents/` (folder 0700, files 0600) across restarts. A torrent's files are taken out of the engine only as one entry directly inside `In Progress` (never deeper, never a `.part` file, whatever the engine reports as its folder). A finished torrent can sit in `completed` while its move to `Completed/` is retried. `DELETE /api/downloads/:id` removes the row (after a final `cancelled` event), answers 409 while a sort is pending, and never deletes files once they are sorted. With `http_via_vpn: when_available`, links on ports other than 80/443 go direct; `always` refuses them. Upload 409/400 bodies carry `received` both inside `error` and at the top level.
- **VPN:** `/api/vpn/nordvpn` defaults `protocol` to `wireguard`. Upload routes accept an optional `filename` for the label; `/api/vpn/openvpn` also accepts JSON `{config, username, password}`. An uploaded `.ovpn` pointing at `*.nordvpn.com` is saved as provider `nordvpn`. The OpenVPN validator mirrors OpenVPN's own line parser and refuses code-running directives, `setenv` outside a small allowlist, and lines OpenVPN would read differently. When the engine does not answer but the supervisor's status file is fresh, the status reports the tunnel's real state with `reachable: false`. A saved VPN that the supervisor has not applied yet (right after a setup, while the engine restarts) reports `connecting`, never `not_configured` with `configured: true` (except with `STASH_ALLOW_NO_VPN`, where it is always `not_configured`). With torrents off (no `STASH_TORRENTD_URL`) the status is `unreachable` with `torrents_off: true` and `error` set to the supervisor's reason (`STASH_TORRENTS_OFF_REASON`) or "Torrents are turned off on this server."; nothing waits for the VPN then, and adding a torrent answers `vpn_unavailable`. NordVPN answering 400, 401 or 403 to the token exchange means the token was not accepted. Settings changes are audit-logged as `settings_changed` (key names only).

- **Engine:** torrent ids are info hashes. A magnet POST answers at once; metadata resolves in the background (state `initializing`, or `error` if it fails) and pending magnets survive restarts. While blocked, the librqbit session is stopped (its sockets are tied to `psw0`); `GET /torrents` then answers from saved state. Speed limits are KiB/s; a nonzero limit below 16 KiB/s counts as 16 KiB/s (librqbit moves 16 KiB blocks and would stall below that), and the `torrent.*_limit_kbps` settings are stored that way. Landlock runs on the engine's own thread and runtime. On kernels without IPv6, sockets fall back to IPv4 (vendored `librqbit-dualstack-sockets` patch). A peer whose connection failed (a seeder that paused or went offline) is reconnected as soon as a tracker, the DHT or PEX lists it again, no sooner than 10 s after it failed, instead of after librqbit's reconnect backoff, which grows to an hour (vendored `librqbit` patch, `vendor/librqbit/PATCHED.md`).
- **Ownership:** the adopt walk only re-owns entries owned by the engine user; `/config` preparation skips files with more than one hard link and refuses symlinked `torrent`, `vpn` or `engine` folders; `vpn/` is forced to 0700.

## 12. Packaging

- Dockerfile: Node stage builds `web/` → Rust stages build `popcornstash` (release, embeds `web/dist`) and `boringtun-cli` → runtime `debian:trixie-slim` with `ffmpeg`, `libvips-tools`, `wireguard-tools`, `openvpn`, `nftables`, `iproute2`, `ca-certificates`, `tini`. `ENTRYPOINT ["tini", "--", "popcornstash"]`, `CMD ["run"]`. Multi-arch amd64/arm64. The Node and Rust stages run on `$BUILDPLATFORM`; when `TARGETARCH` differs from `BUILDARCH` the Rust stages cross-compile (arm64: `aarch64-unknown-linux-gnu` with `gcc-aarch64-linux-gnu`, `libc6-dev-arm64-cross` and multiarch `libssl-dev:arm64`, with the target's `CC`/`CXX`/`AR`, linker and pkg-config path for openssl-sys, ring, aws-lc-sys and the bundled SQLite; amd64 on an arm64 machine the same way round, not yet tested), and each binary's ELF machine is checked. A native build compiles exactly as before. Only the runtime stage runs for the target platform, so an arm64 build on amd64 needs QEMU (binfmt_misc) for its `RUN` steps (docs/deployment.md, "Building the image yourself"). The base registry is a build argument (default Docker Hub; restricted environments use `mirror.gcr.io/library`, and a TLS-intercepting proxy's CA goes in as the `build_ca` secret).
- `compose.yaml`: one service `popcornstash`: ports 8788, `cap_drop: [ALL]` plus `cap_add: [NET_ADMIN, CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID, KILL]` (each verified necessary; NET_RAW is not), `devices: [/dev/net/tun]`, `read_only: true` with tmpfs `/tmp` and `/run` (`/run:exec,mode=755`), `no-new-privileges`, the `net.ipv4.conf.all.rp_filter=2` sysctl (docs/vpn.md), volumes appdata → `/config` and stash → `/stash`, env `PUID/PGID/STASH_PUBLIC_URL` plus the optional settings from `.env` (`STASH_HWACCEL` is empty by default, so Settings decides). The container is named `popcornstash`. The image patches boringtun for IPv4-only hosts, ships the VA drivers for Intel/AMD GPUs, and replaces Debian's open ImageMagick policy with one that allows only the camera RAW coders (section 5, Media). `popcornstash healthcheck` uses HTTPS when `STASH_TLS_CERT` is set. The image carries `LICENSE`, `NOTICE`, `THIRD_PARTY_LICENSES`, `THIRD_PARTY_LICENSES-web` and `debian-packages.txt` (every Debian package with its source package and version, for snapshot.debian.org) in `/usr/share/doc/popcornstash`; `scripts/notices.sh` regenerates the two third-party files.
- `unraid/popcornstash.xml`: one template that names the container `popcornstash`, like compose (`--cap-add=NET_ADMIN --device=/dev/net/tun`, PUID 99 / PGID 100, `STASH_HWACCEL` empty by default).
- No CI service: checks and releases run on a developer machine. GitHub Actions is not used (the repository's Actions are turned off).
  - `scripts/check.sh`: git ignores the usual secret file names and tracks none of them, every manifest says `AGPL-3.0-or-later`, then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, web `npm ci` (only when `node_modules` is missing or older than `package.json` or `package-lock.json`) + `check` + `build`, extension the same + `zip`, then `scripts/docker-smoke.sh` (builds `IMAGE`, default `popcornstash:dev`). Stops at the first failure and prints PASS/FAIL/SKIP per step. `--quick` skips Docker, `--e2e` adds `tests/e2e`, `--full` adds `tests/e2e`, `scripts/vpn-e2e.sh` and `scripts/fullstack-e2e.sh` (on the same image), `--audit` adds `cargo audit` and `npm audit --omit=dev` for web and extension. `IMAGE`, `SKIP_BUILD`, `REGISTRY`, `DOCKER_HUB_MIRROR` and `BUILD_CA` pass through to the Docker scripts. `.githooks/pre-push` (opt-in, `git config core.hooksPath .githooks`) runs it before a push to `main`.
  - `scripts/release.sh <version>`: preflight, all before any build (semver with each number ≤ 65535; on `main`, clean, equal to `origin/main`; tag `v<version>` new here and on `origin`; a `## [<version>] - <today>` section in `CHANGELOG.md`; Cargo workspace and `Cargo.lock`, the `package.json`/`package-lock.json` of web, extension, tests/e2e and tests/fullstack at `<version>`, and `extension/static/manifest.json` at `<version>` without its pre-release; a build for the non-native architecture runs (QEMU); `GHCR_TOKEN` is a classic token with `write:packages` that ghcr.io accepts; the package is public or does not exist yet and has no `<version>`; `gh` can push to the repository and has no `v<version>` release) → `scripts/check.sh --quick --e2e --audit` (the working tree must be unchanged afterwards) → `scripts/notices.sh --check` (the committed third-party notices match `Cargo.lock`, boringtun-cli and `web/package-lock.json`) → `linux/amd64` and `linux/arm64` images built from `git archive` of the commit with `--build-arg POPCORNSTASH_RELEASE=<version>`, labelled with `org.opencontainers.image.version`/`revision`/`created`, both reporting `popcornstash <version>` and each carrying the committed notice files and a Debian package list in `/usr/share/doc/popcornstash` → `docker-smoke.sh` (with `EXPECT_SOURCE=https://github.com/thekozugroup/PopcornStash/tree/v<version>`), `vpn-e2e.sh` and `fullstack-e2e.sh` on the native image (`SKIP_BUILD=1`), then `docker-smoke.sh` on the other architecture's image with `PLATFORM=linux/<arch>` under QEMU (it skips only the VPN and engine checks and checks that torrents stay off) → the extension zip from `npm ci` + `npm run zip` in the same copy → local annotated tag `v<version>` (`--cleanup=verbatim`, the `CHANGELOG.md` section) → push `ghcr.io/thekozugroup/popcornstash:<version>-<arch>`, `docker buildx imagetools create` → `<version>`, plus `<major>.<minor>` and `latest` when `<version>` is the highest full release of its line and overall (a pre-release gets `<version>` only) → push the tag → `gh release create` with the notes, the zip and `--prerelease` or `--latest` → an anonymous `imagetools inspect` (fails the run if the package is not public). The token goes to `docker login` in a temporary `DOCKER_CONFIG` deleted at exit, and `GHCR_TOKEN`/`GH_TOKEN` are removed from the environment of everything else. A failure before the images are joined deletes the local tag; a later one prints the commands that finish the release. `--dry-run` does everything except push, tag and release (a dirty tree is built as it is); `--bump <version>` sets the version numbers and stops. The extension's `manifest.json` `version` is always `MAJOR.MINOR.PATCH` (Chrome accepts only numbers); `extension/scripts/build.mjs` puts a pre-release in `version_name`.
- `cargo audit` reads `.cargo/audit.toml`, which lists the reviewed advisories it may skip and why.

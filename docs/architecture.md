# PopcornStash architecture

This document is the contract between the Rust backend, the web UI, the Chrome extension and the future Android app. Change it in the same commit as any behavior it describes.

## 1. Shape of the system

```
                    HTTPS (reverse proxy, Tailscale, or built-in TLS)
                                     │
┌────────────────────────────────────▼─────────────────────────────────────┐
│ popcornstash  (container 1: unprivileged, cap_drop ALL, read-only rootfs) │
│  popcornstash serve                                                       │
│   ├─ HTTP API + embedded web UI (axum)                                    │
│   ├─ passkey auth, sessions, device tokens, invites                        │
│   ├─ library index (SQLite), watcher, sorter                               │
│   ├─ probe / thumbnails / HLS transcoding (ffmpeg, vips)                   │
│   ├─ HTTP download manager, resumable uploads                              │
│   └─ torrent client ──► internal HTTP API ─┐                               │
└──────────────────── /config (appdata) ─────┼───── /stash (library root) ───┘
                                             │ compose network only
┌────────────────────────────────────────────▼─────────────────────────────┐
│ popcornstash-vpn  (container 2: NET_ADMIN + /dev/net/tun, own netns)       │
│  popcornstash torrentd   (root supervisor: tunnel, routes, kill switch)    │
│   └─ popcornstash torrent-engine  (drops to PUID:PGID)                     │
│       ├─ librqbit session, every socket bound to the tunnel device         │
│       ├─ internal API :9091   (bearer = internal token)                    │
│       └─ HTTP CONNECT proxy :9092 (optional: web downloads via VPN)        │
│  mounts: <appdata>/torrent → /data,  <stash>/Downloads → /stash/Downloads  │
└───────────────────────────────────────────────────────────────────────────┘
```

One image, one binary (`popcornstash`), two roles. The internet-facing web app never holds network privileges. The VPN container can only see `Downloads/`, never the family library.

### Binary subcommands (`crates/popcornstash`)

| Command | Runs in | Purpose |
|---|---|---|
| `serve` | container 1 | Web app, API, library, media, downloads |
| `torrentd` | container 2 (root) | Applies VPN + kill switch, supervises the engine |
| `torrent-engine` | container 2 (PUID) | librqbit + internal API + CONNECT proxy. Spawned by `torrentd` only |
| `admin reset-owner` | `docker exec` | Prints a one-time owner recovery link (lost passkeys) |
| `admin setup-code` | `docker exec` | Prints the current first-run setup code |
| `healthcheck` | Docker HEALTHCHECK | Exit 0 when `/api/health` (or engine `/health`) answers |

### Crates

| Crate | Responsibility |
|---|---|
| `stash-core` | Config, DB pool + migrations, models, library scanner/watcher/classifier/organizer, media probe/thumbnail/transcode, job queue, filename parser |
| `stash-server` | axum router, auth (webauthn-rs), sessions/tokens, security middleware, API handlers, download manager, uploads, torrentd client, NordVPN API client, SSE events, embedded UI |
| `stash-torrentd` | VPN config parsing (wg-quick, ovpn), tunnel bring-up (kernel WireGuard → boringtun fallback, OpenVPN), routes, nftables kill switch, DNS, privilege drop, librqbit engine, internal API, CONNECT proxy |
| `popcornstash` | clap CLI wiring the above |

## 2. Configuration

Environment variables (container 1 unless noted):

| Variable | Default | Meaning |
|---|---|---|
| `STASH_ROOT` | `/stash` | The PopcornStash folder (any directory or Unraid share) |
| `STASH_CONFIG` | `/config` | Appdata: database, caches, secrets, torrent dir |
| `STASH_PUBLIC_URL` | none | `https://stash.example.com`. Required for passkeys: sets the WebAuthn origin and RP ID. `http://localhost:8788` allowed for local testing |
| `STASH_EXTRA_ORIGINS` | none | Comma-separated extra allowed origins (same RP ID) |
| `STASH_BIND` | `0.0.0.0:8788` | Listen address |
| `STASH_TLS_CERT`, `STASH_TLS_KEY` | none | Optional PEM files for built-in HTTPS (rustls) |
| `STASH_TRUST_PROXY` | `false` | Use `X-Forwarded-For` for client IPs (rate limits, audit) |
| `STASH_TORRENTD_URL` | `http://popcornstash-vpn:9091` | Internal engine API. Empty disables torrents |
| `STASH_TORRENTD_PROXY` | `http://popcornstash-vpn:9092` | Internal CONNECT proxy for web downloads via VPN |
| `STASH_HWACCEL` | `none` | `none`, `vaapi`, `qsv`, `nvenc` |
| `STASH_LOG` | `info` | tracing filter |
| `PUID`, `PGID` (container 2) | `99`, `100` | Owner of files the engine writes (Unraid nobody:users) |
| `STASH_ALLOW_NO_VPN` (container 2) | `false` | Only for users whose router already tunnels everything. Engine runs without a tunnel; UI shows a permanent warning |

Persistent settings that the UI edits live in the `settings` table (section 6).

## 3. Folder layout

Created at startup if missing (container 1). Names are fixed.

```
<STASH_ROOT>/
  Movies/                     Movies/Title (Year)/Title (Year).ext
  Shows/                      Shows/Show Name/Season 01/Show Name - S01E02.ext
  Home Videos/                Home Videos/2024/06/original-name.ext
  Photos/                     Photos/2024/06/original-name.ext
  Other/                      Other/original-name.ext
  Inbox/                      Drop zone: anything placed here is sorted
  Downloads/
    In Progress/              Torrents, web downloads and uploads while incoming
      .uploads/               Partial browser uploads (<id>.part)
    Completed/                Finished, waiting for the sorter
  .popcornstash/
    Trash/                    Soft-deleted items (30-day retention)
    stash.id                  Marker file (UUID) so a wrong mount is detected
<STASH_CONFIG>/
  popcornstash.db             SQLite (WAL)
  secrets/session.key         Random key material (0600)
  cache/thumbs/ab/<item>-sm.webp, -lg.webp
  cache/renditions/<item>.webp     Browser-friendly photo renditions (HEIC/RAW/TIFF)
  cache/transcode/<session>/       Live HLS segments (deleted on session end)
  torrent/                    Shared with container 2 (mounted there as /data)
    internal.token            Created by container 1 (0640), read by torrentd
    vpn/vpn.json              { provider, protocol, label, server, country, created_at }
    vpn/wireguard.conf        wg-quick format (0600)
    vpn/openvpn.ovpn          (0600)
    vpn/openvpn.auth          username\npassword (0600)
    engine/                   librqbit persistence and fastresume
    status.json               Written by torrentd supervisor: tunnel state
```

### Movement rules (safety)
- Only files under `Inbox/` and `Downloads/Completed/` are moved automatically.
- Everything else is indexed in place. Category comes from the top-level folder (`Movies/` → movie, `Shows/` → episode, `Home Videos/` → home_video, `Photos/` → photo, `Other/` → other). Files in the owner's own folders are classified heuristically and never moved without an explicit "Organize" action.
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
- audio: mp3 m4a flac wav aac ogg opus (category other)
- everything else: other

Rules, first match wins:
1. **Photo** → `Photos/YYYY/MM/`. Date from EXIF DateTimeOriginal, else filename date (`20240613`, `2024-06-13`, `IMG_20240613_...`), else mtime. Confidence High.
2. **Episode**: `S01E02`, `s1e2`, `1x02`, `Season 1 Episode 2`, `S01E02E03` (first episode). Show name = text before the marker with separators normalized, release tags removed. → `Shows/Show/Season 01/Show - S01E02.ext`. High when show name non-empty.
3. **Home video**: camera metadata (ffprobe tags `com.apple.quicktime.make`, `make`, `model`, `com.android.version`, handler `GoPro`, encoder `DJI`) or camera filenames (`IMG_####.MOV`, `VID_YYYYMMDD_*`, `PXL_*`, `MVI_*`, `GOPR*`, `GH0*`, `GX0*`, `DJI_*`, `DSC*`, `MOV_*`, `Screen Recording*`, `RPReplay*`). Date from `creation_time` tag, filename, mtime. → `Home Videos/YYYY/MM/`. High.
4. **Movie**: title followed by a year 1900–2099 (`Title.2019.1080p...`, `Title (2019)`), release tags stripped (resolution, source, codec, group). → `Movies/Title (Year)/Title (Year).ext`. High when title non-empty and the year is plausible; Low when a year is missing but release tags exist.
5. Anything else → `Other/`. Low.

Samples (`sample` in name and under 150 MB) and extras inside torrent folders are skipped and left in `Completed/`.

Auto-sort (setting `auto_sort`, default on) moves High-confidence results immediately. Low-confidence results become items with `status = needs_review`, stay in place, and appear in Inbox with the suggested destination prefilled. The owner can accept, edit and accept, or ignore (marks it reviewed and leaves it in `Completed/`).

## 5. Processes and pipelines

### Ingest pipeline (container 1)
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
- **Thumbnails:** video frame at 10% (min 3s) via `ffmpeg -ss ... -frames:v 1 -vf scale=...`; photos via `vipsthumbnail --rotate` (HEIC, RAW, TIFF supported). Sizes: `sm` 400px long edge, `lg` 1280px. WebP q80.
- **Photo renditions:** browsers get the original for jpg/png/webp/gif/avif; HEIC/RAW/TIFF get a cached 2560px WebP.
- **Playback decision:** client sends codec support. Direct play when container is mp4/m4v/mov/webm, video codec is supported (h264 always; hevc/av1/vp9 when the client says so), audio is aac/mp3/opus/flac (or absent), and the file is not HDR for an SDR-only client. Otherwise HLS.
- **HLS transcoding:** session per playback. A full VOD playlist with fixed 6s segments is generated up front. ffmpeg runs from the requested segment (`-ss`, `-force_key_frames expr:gte(t,n_forced*6)`, `-start_number`, `-output_ts_offset`), H.264 (libx264 veryfast CRF 23, or `h264_vaapi`/`h264_qsv`/`h264_nvenc`) + AAC stereo 160k, fMP4 or MPEG-TS segments in `cache/transcode/<session>/`. A segment request outside the produced range (behind the start, or more than 3 segments past the newest) restarts ffmpeg at that segment; requests just ahead wait up to 20s for the segment to appear. ffmpeg runs ahead without throttling in v1. Sessions idle for 60s are killed and their folder deleted. HDR sources are tone mapped to SDR (zscale + tonemap hable). Max 2 concurrent transcodes (setting).

### Web downloads (container 1)
reqwest streaming into `In Progress/<name>.part`, resume with `Range`, retry 3× with backoff, concurrency 3. Filename from `Content-Disposition`, else URL path, sanitized. Optional request headers from the extension (`Cookie`, `Referer`, `User-Agent`) are held in memory only and never written to the DB or logs. Setting `http_via_vpn`: `never` | `when_available` (default) | `always` (fails if the VPN is down). Magnet links and `.torrent` URLs are handed to the torrent client.

### Torrents (container 2 engine, container 1 client)
- Container 1 polls `GET /torrents` every 2s while any torrent is active (10s otherwise) and mirrors state into `downloads`.
- Engine downloads into `/stash/Downloads/In Progress/<torrent name>`.
- When a torrent finishes: if `seed_after_complete` is off (default), container 1 calls `DELETE /torrents/:id?delete_files=false`, then moves the folder to `Completed/`. If on, it seeds until `seed_ratio` (default 1.0) or `seed_hours` (default 24), then the same.
- DHT, PEX, uTP enabled. LSD disabled. Upload/download limits from settings.

### VPN (container 2)
`torrentd` (root) loop:
1. Wait for `/data/internal.token`.
2. Read `/data/vpn/vpn.json`. If absent: kill switch with no tunnel, start the engine in "VPN not configured" state (it refuses to add torrents unless `STASH_ALLOW_NO_VPN=true`).
3. Resolve the endpoint(s) using Docker DNS, remember the default gateway, apply the nftables kill switch:
   - table `inet popcornstash`; output policy drop. Allow: `lo`; the tunnel interface; established/related; `eth0` to the container's own subnets; `eth0` to each VPN endpoint IP:port/proto.
   - input policy drop. Allow: `lo`; established/related; from local subnets to tcp 9091/9092; tunnel established.
   - IPv6: drop everything except `lo` (no IPv6 tunnelling in v1).
4. Bring up the tunnel:
   - **WireGuard:** `ip link add psw0 type wireguard`; if the kernel lacks WireGuard, spawn bundled `boringtun-cli psw0 --foreground --disable-drop-privileges` and wait for its UAPI socket. Then `wg setconf psw0 <stripped conf>`, add the address, MTU 1420, link up, route endpoint via the original gateway, `ip route replace default dev psw0`.
   - **OpenVPN:** `openvpn --config openvpn.ovpn --auth-user-pass openvpn.auth --dev psw0 --dev-type tun --auth-nocache --script-security 2 --up/--down <torrentd hook>`; redirect-gateway from the provider config sets routes; the hook reports pushed DNS.
5. Write `/etc/resolv.conf` with the tunnel DNS (WireGuard `DNS=`, OpenVPN pushed DNS, NordVPN 103.86.96.100 / 103.86.99.100 fallback).
6. Spawn `popcornstash torrent-engine` with uid/gid PUID/PGID, env `STASH_TUNNEL_IF=psw0`. librqbit uses `bind_device_name = psw0`, so even a routing mistake cannot reach `eth0`.
7. Monitor: WireGuard latest-handshake < 180s, OpenVPN process alive. Write `/data/status.json` `{state, provider, protocol, server, interface, since, error}` every 5s. On failure: keep the kill switch, tear down, retry with backoff (5s → 5 min).
8. Reload: the engine exits with code 75 when container 1 calls `POST /vpn/reload`; torrentd tears down and restarts from step 2.

Proof of protection: the engine checks its public IP through the tunnel every 10 minutes (NordVPN `https://api.nordvpn.com/v1/helpers/ips/insights` → `{ip, country, protected}`; other providers `https://api.ipify.org?format=json`).

### NordVPN setup (container 1)
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
- **Bearer tokens:** `pst_` + 43 base64url chars. Stored as sha256 with an 8-char display prefix. Scopes: `full`, `downloads:create`, `downloads:read`, `library:read`. Created by device pairing (Chrome → `downloads:create downloads:read`; Android → `full`) or manually in Settings → Devices.
- **CSRF:** cookie-authenticated unsafe methods require `Origin` (or `Referer`) to match an allowed origin and `Content-Type: application/json` or multipart/octet-stream on upload routes. Bearer requests skip the Origin check.
- **Rate limits:** auth start/finish and pairing request/poll: 10/min per IP, burst 20. Failed auth is audit-logged.
- **Headers:** CSP `default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Permissions-Policy: camera=(), microphone=(), geolocation=()`, `Cross-Origin-Opener-Policy: same-origin`, HSTS when https.
- **Paths:** every filesystem path from a request is resolved under its root and rejected if it escapes (canonicalize + prefix check, no symlink escape). Uploaded filenames are sanitized (no separators, no control chars, no leading dots, max 200 bytes).
- **Internal API:** container 2 accepts only `Authorization: Bearer <internal token>` from local subnets; the token is 32 random bytes hex, file mode 0640.
- **Secrets at rest:** VPN files 0600. Device token plaintext is shown once (pairing delivers it once, then the row is wiped).
- **Roles:** owner can do everything. Member can browse, play, upload, add downloads, review Inbox, manage own passkeys/sessions/devices. Owner-only: VPN, settings, people/invites, trash emptying, deleting items, audit log.

## 8. HTTP API (container 1)

JSON, snake_case. Errors: `{"error": {"code": "not_found", "message": "Human readable sentence."}}` with matching HTTP status. Codes: `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`, `rate_limited`, `setup_required`, `passkeys_unavailable`, `vpn_unavailable`, `internal`.

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
ItemDetail = Item & { media: MediaSummary|null, mime, suggestion: Classification|null, source_download_id|null,
  next_episode_id|null, previous_episode_id|null }
Classification { category, title, year, show_name, season, episode, episode_title, taken_at, confidence: "high"|"low", reason, destination: string /* rel path */ }
Download { id, kind: "torrent"|"http"|"upload", name, source, state, progress, total_bytes|null, done_bytes,
  down_bps, up_bps, peers|null, eta_s|null, error|null, created_at, completed_at|null, item_ids: string[] }
VpnStatus { configured: bool, reachable: bool /* torrentd answered */, state: "connected"|"connecting"|"disconnected"|"error"|"not_configured"|"unreachable",
  provider: "nordvpn"|"wireguard"|"openvpn"|null, protocol: "wireguard"|"openvpn"|null, label|null, server|null, country|null,
  public_ip|null, protected: bool|null, since|null, error|null, kill_switch: bool, allow_no_vpn: bool }
```

### Public
| Method | Path | Body → Response |
|---|---|---|
| GET | `/api/health` | → `{status:"ok", version}` |
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
| POST | `/api/pairing/poll` | `{pairing_id, poll_secret}` → 202 `{status:"pending"}` / 200 `{status:"approved", token, scopes, server_name}` / 410 `{status:"denied"|"expired"}` |

### Authenticated (cookie or bearer)
| Method | Path | Scope | Body → Response |
|---|---|---|---|
| POST | `/api/auth/logout` | any | → 204 |
| GET | `/api/me` | any | → `User` |
| GET/POST | `/api/me/passkeys` (+ `/start`, `/finish`) | full | list → `Passkey[]`; add flow like setup |
| PATCH/DELETE | `/api/me/passkeys/:id` | full | `{name}`; delete refuses the last passkey (409) |
| GET/DELETE | `/api/me/sessions[/:id]` | full | → `Session[]` |
| GET/POST/DELETE | `/api/tokens[/:id]` | full | create `{name, scopes}` → `Token & {token}` (once) |
| GET | `/api/pairing/pending` | full | → `[{pairing_id, user_code, client_name, client_kind, ip, created_at}]` |
| POST | `/api/pairing/approve` / `/deny` | full | `{user_code}` → `{ok:true}` |
| GET | `/api/library/summary` | library:read | → `{counts:{movies, shows, episodes, home_videos, photos, other}, total_bytes, needs_review, recently_added: Item[], continue_watching: Item[]}` |
| GET | `/api/items` | library:read | `?category&q&sort=added|title|taken|size&order=asc|desc&favorite&year&status&cursor&limit` → page of `Item` |
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
| GET | `/api/shows/:name` | library:read | → `{show_name, seasons:[{season, episodes: Item[]}]}` |
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
| POST | `/api/uploads/:id/complete` | full | → `Download` (kind upload, goes to `Completed/` then sorted) |
| DELETE | `/api/uploads/:id` | full | → 204 |
| GET | `/api/vpn/status` | any | → `VpnStatus` |
| GET | `/api/vpn/nordvpn/countries` | owner | → `[{id, name, code}]` |
| POST | `/api/vpn/nordvpn` | owner | `{access_token, country_id?, protocol:"wireguard"|"openvpn"}` → `VpnStatus` |
| POST | `/api/vpn/wireguard` | owner | multipart `file` or JSON `{config}` → `VpnStatus` |
| POST | `/api/vpn/openvpn` | owner | multipart `file`, `username`, `password` → `VpnStatus` |
| POST | `/api/vpn/reconnect` | owner | → `VpnStatus` |
| DELETE | `/api/vpn` | owner | → `VpnStatus` |
| GET/PUT | `/api/settings` | owner | the settings object (partial PUT merges) |
| GET | `/api/system` | owner | → `{version, stash_root, config_dir, disk:{total, free}, ffmpeg_version, hwaccel:{configured, available: string[]}, torrentd:{reachable}, jobs:{queued, running, failed}, public_url, passkeys_ready}` |
| GET | `/api/users` / PATCH `/api/users/:id` `{role}` / DELETE `/api/users/:id` | owner | |
| GET/POST/DELETE | `/api/invites[/:id]` | owner | create `{role, expires_in_hours}` → `{id, url, code, role, expires_at}` |
| GET | `/api/audit?limit&cursor` | owner | → page of `{id, at, user, event, detail, ip}` |
| GET | `/api/events` | any | SSE: `download` (Download), `library` `{type:"added"|"updated"|"removed", item_id}`, `vpn` (VpnStatus), `inbox` `{count}`, `job` `{kind, state, item_id}` |

Non-API routes: `/` and all client routes serve the SPA (`index.html`, `Cache-Control: no-cache`); `/assets/*` immutable; `/manifest.webmanifest`; `/sw.js`; `/share` (PWA share target, GET `?url&text&title`, handled client-side); `/.well-known/assetlinks.json` (from `STASH_ANDROID_ASSETLINKS`, future Android passkeys).

## 9. Internal engine API (container 2, port 9091)

Bearer internal token on every route.

| Method | Path | Body → Response |
|---|---|---|
| GET | `/health` | → `{vpn: TunnelStatus, engine: {state:"running"|"blocked", reason|null, allow_no_vpn}, public_ip|null, country|null, protected|null, checked_at|null}` |
| GET | `/torrents` | → `[{id, info_hash, name, state:"initializing"|"downloading"|"paused"|"seeding"|"error", progress, total_bytes, done_bytes, uploaded_bytes, down_bps, up_bps, peers, eta_s, error, output_dir}]` |
| POST | `/torrents` | `{magnet?: string, torrent_base64?: string}` → `{id, info_hash, name}` (409 when blocked by VPN state) |
| POST | `/torrents/:id/pause` / `resume` | → 204 |
| DELETE | `/torrents/:id?delete_files=bool` | → 204 |
| PUT | `/limits` | `{down_kbps, up_kbps}` → 204 |
| POST | `/vpn/reload` | → 202, then the engine exits 75 and torrentd re-applies the VPN |
| CONNECT | `:9092` | HTTP CONNECT proxy, `Proxy-Authorization: Basic base64("stash:<internal token>")`, only to ports 80/443 |

## 10. Clients

- **Web UI** (`web/`): React 19, Vite, TypeScript, Astryx (`@astryxdesign/core` + StyleX via `@astryxdesign/build/vite`) with a custom Popcorn theme. Built into `web/dist` and embedded in the binary (rust-embed). PWA manifest with `share_target` so Android Chrome can share links to PopcornStash.
- **Chrome extension** (`extension/`): Manifest V3. Device pairing (section 8), context menu "Send to PopcornStash" for links/media/magnets, optional "send all browser downloads" with optional cookie forwarding (optional host permissions), popup with recent downloads.
- **Android app:** later. Uses `login/finish` with `want_token`, device pairing with `client_kind: "android"`, the same API, and `/.well-known/assetlinks.json`.

## 11. Packaging

- Dockerfile: Node stage builds `web/` → Rust stage builds `popcornstash` (release, embeds `web/dist`) and `boringtun-cli` → runtime `debian:trixie-slim` with `ffmpeg`, `libvips-tools`, `wireguard-tools`, `openvpn`, `nftables`, `iproute2`, `ca-certificates`, `tini`. Multi-arch amd64/arm64. Images pulled from `mirror.gcr.io` in CI-unfriendly environments.
- `compose.yaml`: services `popcornstash` and `popcornstash-vpn`, shared network, appdata and stash binds, `PUID/PGID`.
- `unraid/popcornstash.xml`, `unraid/popcornstash-vpn.xml`.
- GitHub Actions: `ci.yml` (fmt, clippy, tests, web check/build, extension check, Docker build + smoke), `publish.yml` (GHCR on tags), extension zip artifact.

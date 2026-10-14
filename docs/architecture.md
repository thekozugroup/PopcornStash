# PopcornStash architecture

This document is the contract between the Rust backend, the web UI, the Chrome extension and the Android app (`android/`). Change it in the same commit as any behavior it describes.

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
| `stash-core` | Config, DB pool + migrations, models, libraries (definitions, claims, access predicate, icon list), library scanner/watcher/classifier/organizer, media probe/thumbnail/transcode, job queue, filename parser |
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
| `STASH_ALLOW_NO_VPN` | `false` | Only for users whose router already tunnels everything. The engine runs without a tunnel; every VPN surface shows the permanent warning "No VPN: torrents use this network", even when a VPN is saved (it is not used, its setup forms are hidden, and saving a new one answers 409, section 8) |

Set by the supervisor for its children (not for users): `STASH_TORRENTD_URL=http://127.0.0.1:9091`, `STASH_TORRENTD_PROXY=http://127.0.0.1:9092`, `STASH_INTERNAL_TOKEN` (random per start), `STASH_SUPERVISOR_SOCKET=/run/popcornstash/supervisor.sock`, `STASH_TUNNEL_IF=psw0` (engine only), and in degraded mode `STASH_TORRENTS_OFF_REASON` (the sentence the UI shows). When `serve` runs without the supervisor these are unset and torrents are disabled. Children get `HOME=/tmp` (web server) or the engine state dir; the web server also gets the GPU render device's group as its only supplementary group when `/dev/dri/renderD128` exists (never group 0).

Persistent settings that the UI edits live in the `settings` table (section 6).

## 3. Folder layout

The fixed folders (`Other/`, `Inbox/`, `Downloads/…`, `.popcornstash/…`) are created at startup if missing and their names never change. Each library adds one top-level folder that the owner names (section 3.1); a fresh stash starts with four.

```
<STASH_ROOT>/
  <library folder>/           One per library, named by the owner; a fresh stash has Movies/, Shows/, Home Videos/, Photos/.
                              A sorted file is laid out by what it is, whatever the library:
                                movie         <folder>/Title (Year)/Title (Year).ext
                                TV episode    <folder>/Show Name/Season 01/Show Name - S01E02.ext
                                camera video  <folder>/2024/06/original-name.ext
                                photo         <folder>/2024/06/original-name.ext
  Other/                      Other files (built in, not a library): Other/original-name.ext
  Inbox/                      Drop zone: anything placed here is sorted
  <any other folder>/         The owner's own top-level folder, not in a library: indexed in place, seen by owners only
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
- Everything else is indexed in place, except the rest of `Downloads/` (anything outside `In Progress/` and `Completed/`, such as another download client's folders), which is never moved and never indexed. What an indexed file is (its `category`) comes from where it is: in a library folder, from the library's type and structure (section 4, Files already in a library); in `Other/`, `other`; anywhere else, the classifier's guess, kept as the suggestion that prefills Organize. Nothing outside `Inbox/` and `Downloads/Completed/` moves without an explicit Organize by the owner.
- Creating, renaming, reordering or deleting a library, changing who sees it and changing what it claims never move, rename or delete a file.
- Moves use `rename(2)`; across filesystems, copy + fsync + verify size + remove.
- Name collisions append ` (2)`, ` (3)` before the extension. Nothing is overwritten.
- Sidecar files with the same stem (`.srt`, `.ass`, `.vtt`, `.sub`, `.idx`, `.nfo`, `.xmp`, `.aae`) move with their media file.
- Partial files (`.part`, `.crdownload`, `.!qB`, `.tmp`, `~$*`), hidden files and `.popcornstash/` are ignored.
- A file is stable when size and mtime are unchanged for 10 seconds (copies over SMB take time).

### 3.1 Libraries and their folders

A library is a row in `libraries` (section 6): a name, an icon, a content type (`video` or `photos`), for video a structure (`episodes` or `one_offs`), the kinds of new file it claims (section 4), who sees it (section 7), its place in the order, and exactly one folder.

- **One top-level folder per library.** The folder is a real directory (not a symlink) directly under `STASH_ROOT`. Everything below it belongs to that library. A subfolder cannot be a library of its own, so libraries never nest, and a folder never belongs to two libraries: `libraries.folder` is unique, compared without regard to case. On a case-sensitive file system a `movies/` beside the library folder `Movies/` is a separate folder that is not in a library, and it cannot become one while `Movies` is a library folder.
- **Folder names.** 1 to 100 characters after trimming, no `/`, `\`, control characters, leading dot, trailing dot or trailing space, and not `.` or `..`. Reserved, in any case: `Inbox`, `Downloads`, `Other`, every name starting with a dot, and the names the indexer always skips (`lost+found`, `#recycle`, `$RECYCLE.BIN`, `System Volume Information`, `#snapshot`, `@eaDir`, `@Recycle`, partial-file names such as `*.part`).
- **New or existing.** Creating a library either makes a new folder (refused when anything with that name already exists, in any case) or takes an existing top-level folder that is not a library folder and not reserved. An existing folder's files are indexed in place under the library's rules (section 4) and never moved.
- **Fixed for life.** A library's folder never changes. Renaming a library changes only its name in the app.
- **Created only while empty, never stood in for.** A library's folder is created automatically only while the library holds no items (`is_empty`): on a fresh install, when a library is made with `folder.mode = "new"`, and at startup or when the sorter needs it for a library that is still empty. For a library that has items not marked missing, the folder counts as *missing* (`folder_state: "missing"`, shown to owners) when it is absent, is not a directory (a symlink counts as not a directory), cannot be read, or is completely empty, with nothing in it at all (an unmounted disk or share leaves an empty mount point; a folder that still holds an empty subfolder is not missing). An empty library's absent folder is not missing: it is created when something needs it. A folder that is there but cannot be written is *read-only* (`folder_state: "read_only"`). While a library's folder is missing, PopcornStash does not create it, rescans mark none of its items missing (section 5, Watcher), and nothing is moved into it; while it is missing or read-only, files of the kinds it claims wait in Inbox (`waiting: "folder_unavailable"`, section 8) and accepts or Organize into it answer 409. Neither state stops the server, and each ends at the next rescan after the folder is back. A folder that is gone for good is let go by deleting the library: its items then have no library, and the next rescan marks them missing like any other vanished file.
- **Deleting a library** removes it from the app only. Its folder and files stay where they are, and the folder becomes a folder that is not in a library.
- **Folders not in a library.** Every other top-level folder (not a library folder, not `Inbox`, `Downloads`, `Other` or `.popcornstash`, not hidden, not a symlink) and any loose file directly in the stash root are indexed in place and seen by owners only (section 7). Settings lists these folders with "Make a library" (`GET /api/libraries/folders`).
- **Membership follows the path.** `items.library_id` is the library whose `folder` equals the first component of the item's `rel_path` (exact match), else NULL, and `items.place` says which kind of place holds it (section 6). Both are recomputed whenever a file's path changes (sorting, Organize, a move found by a rescan, restore from Trash) and for every item under the folder when a library is created or deleted, always under the move lock (section 5) and in the same write that records the path, so they are worked out from the final path and the library rows as they are at that moment. Every rescan ends by re-deriving both for every item from `libraries.folder` in one UPDATE under the move lock, so a row that went stale repairs itself. In the database this is done by two triggers (section 6), so no write that records a path can leave them behind. Inside a library folder the index holds only files the library shows (section 4, Files already in a library). A file moved on disk from one library folder to another changes library at the next rescan and keeps its id, favourite and progress.

## 4. Classification and sorting

`stash-core::library::classify(path, probe, exif) -> Classification { category, title, year, show, season, episode, episode_title, taken_at, confidence: High|Low, reason }`

Kinds by extension:
- video: mkv mp4 m4v mov avi webm ts m2ts mts wmv flv 3gp mpg mpeg vob ogv
- photo: jpg jpeg png heic heif webp gif tif tiff avif dng cr2 cr3 nef arw orf rw2 raf
- audio: mp3 m4a flac wav aac ogg opus (category other, sorted to `Other/` with High confidence)
- everything else: other

Rules, first match wins:
1. **Photo** (`photo`, laid out `YYYY/MM/`). Date from EXIF DateTimeOriginal, else filename date (`20240613`, `2024-06-13`, `IMG_20240613_...`), else mtime. Confidence High (pictures that may be artwork excepted, see below).
2. **Episode**: `S01E02`, `s1e2`, `1x02`, `Season 1 Episode 2`, `S01E02E03` (first episode). Show name = text before the marker with separators normalized, release tags removed. Laid out `Show/Season 01/Show - S01E02.ext`. High when show name non-empty.
3. **Home video**: camera metadata (ffprobe tags `com.apple.quicktime.make`, `make`, `model`, `com.android.version`, handler `GoPro`, encoder `DJI`) or camera filenames (`IMG_####.MOV`, `VID_YYYYMMDD_*`, `PXL_*`, `MVI_*`, `GOPR*`, `GH0*`, `GX0*`, `DJI_*`, `DSC*`, `MOV_*`, `Screen Recording*`, `RPReplay*`). Date from `creation_time` tag, filename, mtime. Laid out `YYYY/MM/`. High.
4. **Movie**: title followed by a year 1900–2099 (`Title.2019.1080p...`, `Title (2019)`), release tags stripped (resolution, source, codec, group). Laid out `Title (Year)/Title (Year).ext`. High when title non-empty and the year is plausible; Low when a year is missing but release tags exist.
5. Anything else (`other`) → `Other/`. Low.

Rules 1 to 4 say what a file is and how it is laid out inside a library folder; which library it goes to comes from the claims below.

Samples (`sample` in name and under 150 MB) and extras inside torrent folders are skipped and left in `Completed/`.

What a sort looks at (one top-level entry of `Inbox/` or `Downloads/Completed/`):
- A file that is not a video, photo or song and arrives on its own (an upload, a web download, a document copied into Inbox, or a folder with no media) follows rule 5: it waits in Inbox, suggested for `Other/`. A sidecar whose media file sits next to it is not sorted on its own; it moves with that file.
- In a folder that holds a video, photo or song, the other files stay where they are. Release clutter (`.nfo .sfv .url .md5 .sha1 .sha256 .sha512 .par2 .webloc .lnk`) is never sorted out of a folder.
- In a folder that holds a video or song, or whose name is a release name, pictures that are artwork stay where they are. Artwork means names like `poster`, `cover`, `folder`, `fanart`, `banner`, `thumb`, `screen01`, `<title>-poster`, `www.…`, or anything in `Screens/`, `Screenshots/`, `Artwork/`, `Scans/`, `Covers/` and similar folders.
- Other pictures in a Completed download that holds a video or song wait in Inbox, suggested for `Other/` (Low). Artwork-like pictures in other folders wait for review too.
- A single picture (the extension's "Send image") and a download of only pictures follow rule 1.
- A download folder is judged together with what earlier sorts took out of it (the items linked to its download).

### Where a sorted file goes: claims

Each library claims zero or more kinds of new file, and each kind is claimed by at most one library (`library_claims`, section 6). A claim must fit the library:

| Kind (`category`) | Owner's words | Fits | Layout under the library folder |
|---|---|---|---|
| `movie` | movies | Video, One-offs | `Title (Year)/Title (Year).ext` (`Title/Title.ext` without a year) |
| `episode` | TV episodes | Video, Episodes | `Show Name/Season 01/Show Name - S01E02.ext` (season 1 when unknown) |
| `home_video` | camera videos | Video, One-offs | `YYYY/MM/original-name.ext` |
| `photo` | photos | Photos | `YYYY/MM/original-name.ext` |

`other` (music, documents, anything else) cannot be claimed: it always goes to `Other/`, the built-in Other files place.

For each file it takes, the sorter classifies it (rules above; probing and hashing happen outside the move lock because they take time), then takes the move lock (section 5) and holds it from reading the claims until the item row is written. Under the lock it first checks that neither the file nor its row changed while it was probing, then:
1. `other`: to `Other/` (High for songs; anything else waits in Inbox, Low, as before).
2. A claimable kind that no library claims: waits in Inbox (`needs_review`) whatever its confidence, with no suggested library. A person picks a library that fits (`POST /api/inbox/:id`, section 8).
3. Claimed, but the library's folder is missing or read-only (section 3.1): waits in Inbox.
4. Claimed, High confidence and `auto_sort` on: moved to `<library folder>/<layout>`; `library_id` and `place` are worked out from the final path in the same write that records it.
5. Claimed, Low confidence or `auto_sort` off: waits in Inbox with the suggestion.

Why a file waits is not stored; it is worked out whenever the item is read (`Classification.waiting`, section 8).

**One sort job, one set of claims.** A sort job takes one top-level entry of `Inbox/` or `Downloads/Completed/` (a single file, or a whole season pack) and notes the claims generation when it starts. The generation goes up with every library write that can change where a file goes: a create, a delete, and an edit of claims, type or structure. When the generation has changed by the time the job reaches a file, that file and every later file of the job wait in Inbox with their suggestion instead of moving, so a claim that moves halfway never splits one download between two libraries. Files the job already moved stay where they are.

A stored suggestion (`items.suggestion_json`) holds only the classification: kind, title, year, show, season, episode, episode title, date, confidence and the classifier's reason (a sentence about the file itself, such as "Not sure this is a movie."). Where it would go and why it waits are worked out every time it is read, from the claims, the library folders and the reader's access at that moment (`Classification.place`, `library_id`, `destination` and `waiting`, section 8). So moving a claim to another library, deleting the library that held it, or a library folder coming back re-points or clears every waiting suggestion at once, and nothing stored goes stale. Claims decide where new files go; they never move files that are already in a library.

Auto-sort (setting `auto_sort`, default on) moves High-confidence results immediately. Waiting results become items with `status = needs_review`, stay in place, and appear in Inbox with the suggestion prefilled. A person who can see the item (section 7) accepts it, edits and accepts, or ignores it (marks it reviewed and leaves the file where it is). Where they may send it depends on who they are (section 8, Inbox accept and Organize): the person who added the file and full owners may choose any library they see that fits it, or Other files; anyone else, who sees the file only because a library they see claims its kind, may accept it only into that claiming library.

### Files already in a library (indexed in place)

A file that is in a library folder without having been sorted there (it was in the folder when the library was made, or someone copied it in) is indexed in place. What the library indexes and what each file is follow from its type and structure:

| Library | Indexed | `category` | Metadata from the path |
|---|---|---|---|
| Video, Episodes | Video files, except samples and anything inside an extras folder below the show folder (`Extras`, `Featurettes`, `Behind The Scenes`, `Deleted Scenes`, `Interviews` and the rest of the indexer's list, any case) | `episode` | Show from the first folder below the library folder, else the file's title, else the file name; season from a `Season NN` folder or the file name; episode and episode title from the file name |
| Video, One-offs | Depends on the claims, below | `movie` or `home_video` | Movies: title and year from the `Title (Year)` folder or the file name. Home videos: date from the file name, else the file time |
| Photos | Photo and video files (phone clips live with the camera roll) | `photo` | Date from the file name, else the file time; EXIF or QuickTime dates replace it after the probe |

For a One-offs library:
- Claims camera videos and not movies: video and photo files are indexed, all as `home_video`.
- Claims movies and not camera videos: video files only (samples and anything in an extras folder below a title folder excepted), as `movie`.
- Claims both or neither: video files are `home_video` when they have camera evidence (rule 3) or sit under a `YYYY/MM/` folder, `movie` otherwise; photo files (artwork and anything inside a `Title (Year)` folder excepted) are `home_video`.

A file keeps the category it was given when it was first indexed; changing claims later affects only files indexed after the change. Files a library does not index (a PDF in a Video library) are left alone on disk and are not in the index, so they show nowhere, not even to owners.

When a library is made from an existing folder, the folder's items are sorted out in the same transaction that makes the library, under the move lock: items the library indexes join it (`library_id` set, `place = 'library'`), with category and metadata read again from the path and their ids, favourites and progress kept; items it does not index leave the index in that same transaction, and their favourite and progress go with them (the files stay). So nothing the library will not show is ever visible through it, not even for a moment. Before saving, the editor says how many of the folder's files the chosen type will not show (from `UnassignedFolder`'s counts). A scan job then indexes whatever in the folder was not indexed yet.

The four starting libraries reproduce the old fixed folders exactly: Movies claims movies (so: videos only, `movie`), Shows is Episodes (`episode`), Home Videos claims camera videos (videos and photos, `home_video`), Photos is Photos (`photo`).

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

### Move lock
One lock in `stash-core` (`Library::moves`, a single async mutex) serialises every step that moves a file or decides an item's path, library or place: the sorter (from reading the claims until the item row is written, section 4), Inbox accept, Organize, trash, restore, emptying the trash, watcher and rescan indexing, marking items missing and purging them, the rescan's closing re-derivation of `library_id` and `place` (section 3.1), and the library writes that change where files belong: create (with the sort-out of a taken folder, section 4), delete, and edits of claims, type or structure. Those library writes hold it for their whole transaction, so the sorter and the indexer never act on library rows that changed under them: a file is never moved into the folder of a library that has just been deleted, and an item is never recorded as `folder` inside a folder that has just become a library. Slow work (probing, hashing) runs outside the lock, and whatever it read is checked again under the lock before anything moves. A library write waits for a move already under way to finish; renames, icon, order and access changes do not take the lock.

### Watcher
`notify` (inotify) on `STASH_ROOT` plus a full rescan every 30 minutes and on startup (inotify is unreliable on some network shares). Rescan reconciles: new files → items; vanished files → `status = missing` (kept 7 days, then removed); moved files are matched by `(size, quick_hash)` so favorites and progress survive. `quick_hash` = xxh3-64 of size + first 1 MiB + last 1 MiB. A rescan marks nothing missing when the stash folder has no files but the database has items, nor inside a library whose folder is missing (section 3.1): an unmounted disk or share never empties a library.

### Media
- **Probe:** `ffprobe -v error -print_format json -show_format -show_streams`. Stored summary in `items.media_json` (section 6).
- **Thumbnails:** video frame at 10% (min 3s) via `ffmpeg -ss ... -frames:v 1 -vf scale=...`; photos via `vipsthumbnail --rotate` (HEIC, RAW, TIFF supported). Sizes: `sm` 400px long edge, `lg` 1280px. WebP q80. libvips runs with `VIPS_BLOCK_UNTRUSTED=1` (only its JPEG, PNG, WebP, GIF, TIFF and HEIF loaders) except for a file with a camera RAW name (`dng cr2 cr3 nef arw orf rw2 raf`) that also starts like a RAW format only ImageMagick reads (ORF, RW2, RAF, CR3); TIFF-based RAW files (DNG, CR2, NEF, ARW) and anything else under a RAW name get the trusted loaders only. The image's ImageMagick policy allows only the RAW coders (plus JPEG and TIFF), no delegates, filters or `@file` reads. ffmpeg always reads `file:<path>` (its default protocol whitelist then stays `file,crypto,data`, so a playlist inside a hostile file cannot make it fetch URLs). Each image is written to a hidden `.<name>.<16 hex>.tmp.webp` next to its place and renamed when complete; a cancelled conversion (a photo view the browser closes) removes it, and leftovers in `cache/thumbs` and `cache/renditions` are removed on start.
- **Photo renditions:** browsers get the original for jpg/png/webp/gif/avif; HEIC/RAW/TIFF get a cached 2560px WebP.
- **Playback decision:** client sends codec support. Direct play when container is mp4/m4v/mov/webm, video codec is supported (h264 always; hevc/av1/vp9 when the client says so), audio is aac/mp3/opus/flac (or absent), and the file is not HDR for an SDR-only client. Otherwise HLS. The web player plays HLS with hls.js wherever Media Source Extensions work and uses the browser's own HLS only where they do not, so a resumed stream starts at the resume segment.
- **HLS transcoding:** session per playback. A full VOD playlist with fixed 6s segments is generated up front. ffmpeg runs from the requested segment (`-ss`, `-force_key_frames expr:gte(t,n_forced*6)`, `-start_number`, `-output_ts_offset`), H.264 (libx264 veryfast CRF 23, or `h264_vaapi`/`h264_qsv`/`h264_nvenc`) + AAC stereo 160k, fMP4 or MPEG-TS segments in `cache/transcode/<session>/`. A segment request outside the produced range (behind the start, or more than 3 segments past the newest) restarts ffmpeg at that segment; requests just ahead wait up to 20s for the segment to appear. ffmpeg runs ahead without throttling in v1. Sessions idle for 60s are killed and their folder deleted. HDR sources are tone mapped to SDR (zscale + tonemap hable). Max 2 concurrent transcodes (setting). There is no remux path yet: every HLS stream re-encodes the video, also when only the container or the audio needed converting (an H.264 MKV, an MP4 with DTS audio).

### Web downloads
reqwest streaming into `In Progress/<name>.part` (0600), resume with `Range`, retry 3× with backoff, concurrency 3. Filename from `Content-Disposition`, else URL path, sanitized. Optional request headers from the extension (`Cookie`, `Referer`, `User-Agent`) are held in memory only and never written to the DB or logs. The forwarded `Cookie` never follows a redirect to another origin (scheme, host and port): reqwest drops it for direct downloads, and redirects followed by hand through the VPN proxy (downloads and `.torrent` links) drop it the same way for the rest of the chain. Setting `http_via_vpn`: `never` | `when_available` (default) | `always` (fails if the VPN is down). With `STASH_ALLOW_NO_VPN` there is no VPN, so the setting has no effect: web downloads go direct whatever it says (the status reports `allow_no_vpn`). Magnet links and `.torrent` URLs are handed to the torrent client.

### Torrents (engine process, web server client)
- Adding a torrent while the VPN is down creates the download in `queued` with the message "Waiting for the VPN to connect" ("Blocked: VPN not protecting" while the engine is blocked for the public address, section 11); the poller adds it to the engine once the engine is `running`. With torrents disabled (`STASH_TORRENTD_URL` unset) the API answers `vpn_unavailable`.
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

Proof of protection: the engine checks its public IP through the tunnel every 10 minutes (NordVPN `https://api.nordvpn.com/v1/helpers/ips/insights` → `{ip, country, protected}`; other providers `https://api.ipify.org?format=json`). The service addresses are fixed in production and injectable in tests. When NordVPN answers `protected: false`, the engine stops torrents (engine `blocked`, reason "The VPN is connected, but the public address is not a VPN address. Torrents stay stopped until a later check passes.") and checks again every 60 s until a check no longer says `protected: false`. The answer outlasts a tunnel drop and an engine restart (Reconnect, a saved VPN, a container restart; it is kept in `/config/torrent/engine/not-protected.json`): torrents start only after a check passes, the address is checked again as soon as the tunnel is back, and meanwhile the status keeps `protected: false` and the last address. The web server also reports `protected: false` whenever the engine is blocked with that reason.

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
      favorite INTEGER DEFAULT 0, source_download_id NULL, missing_since NULL, added_at, updated_at,
      /* added by 0002 with ALTER TABLE ADD COLUMN, so they come last: */
      library_id NULL → libraries /* no ON DELETE action: a library row is deleted only after its items left it (section 8, Delete) */,
      place NOT NULL DEFAULT 'folder' CHECK(place IN ('library','other_files','incoming','folder','trash')
                                            AND (place = 'library') = (library_id IS NOT NULL)),
      added_by NULL → users ON DELETE SET NULL)
libraries(id PK, name /* 1-40 chars, unique ignoring case */, icon /* a name from the icon list, section 8 */,
          content_type CHECK(content_type IN ('video','photos')),
          structure NULL CHECK(structure IN ('episodes','one_offs')) /* set for video, NULL for photos */,
          folder UNIQUE COLLATE NOCASE /* one path component under STASH_ROOT, never changes */,
          visibility CHECK(visibility IN ('everyone','selected')), position INTEGER /* 0..n-1: sidebar and Home order */,
          created_by NULL → users ON DELETE SET NULL, created_at, updated_at,
          CHECK((content_type = 'video') = (structure IS NOT NULL)))
library_claims(category PK CHECK(category IN ('movie','episode','home_video','photo')), library_id → libraries ON DELETE CASCADE)
library_members(library_id → libraries ON DELETE CASCADE, user_id → users ON DELETE CASCADE, PRIMARY KEY(library_id, user_id))
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

Library columns and tables (migration `0002_libraries.sql`):
- `items.library_id` and `items.place` are worked out by two triggers that 0002 creates, `items_place_after_insert` (after any insert) and `items_place_after_update` (after an update of `rel_path` or `status`), from the final path and the library rows at that moment: the library whose `folder` equals the first path component exactly (`COLLATE BINARY`, whatever the column's collation) and only when the path has a component below it; `trash` for `status = 'trashed'`. Library create, delete and the end of every rescan run the same expressions over many rows (`library::defs`).
- `items.place`: `library` (under a library folder; `library_id` set), `other_files` (under `Other/`), `incoming` (under `Inbox/` or `Downloads/Completed/`, waiting or ignored), `folder` (in a top-level folder that is not a library folder, or loose in the stash root), `trash` (in `.popcornstash/Trash`). `library_id` and `place` follow the path (section 3.1).
- `items.added_by`: the person whose download, link, upload or share brought the file in (`downloads.created_by` of the download the sorter took it from), kept across moves and renames. NULL for files that appeared in a folder on their own (copied into `Inbox/` or onto the share). It decides who sees Other files and incoming items (section 7).
- `library_claims`: the primary key on `category` is what makes "each kind claimed by at most one library" hold. Fit (movie and camera video to Video One-offs, TV episode to Video Episodes, photo to Photos) is checked by the server on every write.
- `library_members`: the members who see a library whose `visibility` is `selected`. Only members are listed; owners see every library (section 7). Rows of a person who becomes an owner are kept and apply again if they return to the member role.
- Indexes: `items(library_id, status, added_at DESC)`, `items(library_id, category, status)`, `items(place, status)`, `items(added_by)`, `library_members(user_id)`, and a unique index on `libraries(name COLLATE NOCASE)`. `position` is not unique in the schema; every write that changes order rewrites all positions as 0..n-1 in one transaction.
- Every library write (create, edit, claims, order, access, delete) runs in one transaction together with the `library_id`/`place` updates of the items it affects; create, delete and edits of claims, type or structure also hold the move lock for that whole transaction (section 5).
- `items.library_id` has no `ON DELETE` action on purpose: `SET NULL` would leave `place = 'library'` with no library and fail the CHECK, so deleting a library depends on statement order. Delete sets the items to `library_id = NULL, place = 'folder'` first, then deletes the claims, the people and the library row (section 8, Delete); a library row that still has items cannot be deleted.

### Migration of existing data

`0002_libraries.sql` runs once, on an existing install and on every fresh install right after `0001_init.sql`, so every stash starts with the same four libraries. It changes nothing on disk, and it rebuilds no table.

Why no rebuild: the pool opens every connection with foreign keys on (`db.rs`), sqlx runs each migration inside one transaction, and its SQLite driver (sqlx 0.8.6) ignores the `-- no-transaction` marker, so `PRAGMA foreign_keys=OFF` cannot take effect inside a migration. SQLite's copy-and-rename rebuild of `items` would then delete every `items` row on `DROP TABLE`, and `ON DELETE CASCADE` would empty `item_progress`, `download_items` and `trash` (reproduced with SQLite 3.45: no progress row survives). So 0002 only adds tables and columns and fills them, all inside the one transaction sqlx gives it:

1. Creates `libraries`, `library_claims` and `library_members`.
2. Adds `items.library_id`, `items.place` and `items.added_by` with `ALTER TABLE items ADD COLUMN`. The pairing of `place` and `library_id` is a CHECK on the `place` column (SQLite lets a column CHECK name another column, and checks it on every later write); `place` defaults to `folder`, which satisfies it for every existing row until step 4.
3. Inserts the four libraries, visible to everyone, with fixed ids, and their claims:

| id | name | icon | content_type | structure | claims | folder | position |
|---|---|---|---|---|---|---|---|
| `0192a000-0000-7000-8000-000000000001` | Movies | `film` | `video` | `one_offs` | `movie` | `Movies` | 0 |
| `0192a000-0000-7000-8000-000000000002` | Shows | `tv` | `video` | `episodes` | `episode` | `Shows` | 1 |
| `0192a000-0000-7000-8000-000000000003` | Home Videos | `clapperboard` | `video` | `one_offs` | `home_video` | `Home Videos` | 2 |
| `0192a000-0000-7000-8000-000000000004` | Photos | `image` | `photos` | NULL | `photo` | `Photos` | 3 |

4. Assigns every item by folder, setting `place` and `library_id` together in one UPDATE: `trash` when `status = 'trashed'`; otherwise `library` with the matching `library_id` when the first path component is one of the four folders, `other_files` for `Other/`, `incoming` for `Inbox/` and `Downloads/Completed/`, `folder` for everything else. `category`, metadata, favourites and progress stay as they are.
5. Fills `added_by` from `downloads.created_by`, through `items.source_download_id`, else through `download_items` (whose rows are untouched).
6. Creates the indexes.
7. Creates the two triggers that keep `library_id` and `place` with the path from then on (above).

**Required test** (`stash-core`): build a database at 0001 holding items in each kind of place, `item_progress`, `trash`, `download_items` and favourite rows, run 0002 through the real pool (foreign keys on), and check that every one of those rows is still there, that `place`, `library_id` and `added_by` are filled as above, and that `PRAGMA foreign_key_check` returns nothing. A later migration that really must rebuild a table that other tables reference runs that rebuild outside sqlx's migrator, on its own connection opened with foreign keys off, and is tested the same way.

What changes for existing households: items in other top-level folders (seen by everyone before) are seen by owners only until an owner makes their folder a library; Other files and incoming items are seen by owners and the person who added them; and a download someone else added is listed for a member only once it is sorted and something it brought in is in a library that member sees (section 7). Everything in the four starting libraries looks and sorts exactly as before.

## 7. Security model

- **Passkeys only.** No passwords. webauthn-rs, user verification required, discoverable credentials (passkeys) so sign-in needs no username. RP ID = host of `STASH_PUBLIC_URL`.
- **First run:** on startup with zero users, a setup code (`XXXX-XXXX-XXXX`, 60 bits, valid 24h, regenerated on restart until used) is printed to the log. Registering the owner requires it, so nobody else on the network can claim a fresh install.
- **Recovery:** `docker exec popcornstash popcornstash admin reset-owner` prints a one-time link valid 30 minutes that lets the owner register a new passkey.
- **Invites:** owner creates invite links (role member or owner, default expiry 72h, single use).
- **Sessions:** 256-bit random token in cookie `__Host-stash` (`Secure` when the public URL is https, `HttpOnly`, `SameSite=Strict`, `Path=/`). Stored as sha256. 30-day sliding expiry, absolute max 180 days. Cookie name falls back to `stash` (no `__Host-` prefix) on http://localhost.
- **Bearer tokens:** `pst_` + 43 base64url chars. Stored as sha256 with an 8-char display prefix. Scopes: `full`, `downloads:create`, `downloads:read`, `library:read`. Created by device pairing (Chrome → `downloads:create downloads:read`; Android → `full`; `client_kind: "other"` → `downloads:create` only) or manually in Settings → Devices.
- **Bearer sessions:** `POST /api/auth/login/finish` with `want_token: true` returns a bearer session, not a device token: a `sessions` row of kind `bearer` with the user's full access, the same 30-day sliding expiry and 180-day absolute limit as the cookie, listed under `/api/me/sessions` (not `/api/tokens`) and ended by logging out with it or deleting the session. The token is sent as `Authorization: Bearer`, like a device token.
- **CSRF:** unsafe methods (POST/PUT/PATCH/DELETE) under `/api` without a bearer header require `Origin` (or `Referer`) to match an allowed origin, and a body, when present, must be `application/json`, `multipart/form-data` or `application/octet-stream`. Bearer requests skip the check. `POST /api/pairing/request` and `/api/pairing/poll` are exempt (the Chrome extension sends a `chrome-extension://` origin and native apps send none); they never read the cookie. `POST /api/auth/login/start` and `/api/auth/login/finish` skip the Origin check only when the request carries neither `Origin` nor `Referer` (the Android app's passkey sign-in; a browser always sends `Origin` on a cross-site POST, and a cross-site JSON body needs a preflight the server never grants); their body must then be `application/json`. Setup, invites, recovery and adding a passkey always need the origin.
- **Rate limits:** auth start/finish, pairing request/poll and invite lookups: 10/min per client, burst 20. IPv6 clients are grouped by /64. With `STASH_TRUST_PROXY`, the client address is the last `X-Forwarded-For` entry (the one the trusted proxy appended). Failed auth is audit-logged.
- **Headers:** CSP `default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Permissions-Policy: camera=(), microphone=(), geolocation=()`, `Cross-Origin-Opener-Policy: same-origin`, HSTS when https.
- **Paths:** every filesystem path from a request is resolved under its root and rejected if it escapes (canonicalize + prefix check, no symlink escape). The check fails closed: a link whose target is missing, or a path that cannot be resolved, is refused too. Uploaded filenames are sanitized (no separators, no control chars, no leading dots, max 200 bytes).
- **Internal API:** the engine listens on 127.0.0.1 only and requires `Authorization: Bearer <internal token>`; the token is 32 random bytes hex, generated by the supervisor at each start and passed to both children in the environment, never written to disk.
- **Secrets at rest:** VPN files 0600; the database 0600; `cache/`, `secrets/`, `pending-torrents/` and `.popcornstash/uploads/` 0700. Device token plaintext is shown once (pairing delivers it once, then the row is wiped).
- **Logs:** request spans (debug level) carry the method and the path only; invite and recovery codes in paths are replaced with `<redacted>` and query strings are never logged.
- **Engine answers:** the web server reads at most 8 MiB from the engine and passes on at most 300 characters (one line) of any engine message, so a compromised engine cannot exhaust its memory or put a long text in front of people.
- **Roles:** owners manage everything; members browse and play the libraries they can see, upload, add downloads, review the Inbox items they can see, and manage their own passkeys/sessions/devices. What each caller sees is in Library access below. Owner-only, and only with the `full` scope (`AuthUser::is_owner`): libraries (create, edit, claims, access, order, delete), folders not in a library, Organize, VPN, settings, people/invites, trash, deleting items, audit log.
- **Android passkeys:** when `STASH_ANDROID_ASSETLINKS` lists the app (`com.thekozugroup.popcornstash` with `delegate_permission/common.get_login_creds`), the server also accepts the WebAuthn origin `android:apk-key-hash:<base64url of the SHA-256 certificate fingerprint>` for each `sha256_cert_fingerprints` entry of that statement, with the same RP ID. No other `android:` origin is accepted. A statement for another package, without that relation, or a fingerprint that is not 32 hex bytes adds nothing.

### Library access

One rule decides who sees an item, and every route that returns, counts or touches items applies it. It lives once in `stash-core` (`library::access`: a SQL predicate `visible_items(user)` used inside every item query, and `can_see(user, item)` for single items), so no handler writes its own check.

**Who is asking.** Access depends on the caller's role and on the scopes of the credential in use, never on the role alone:
- A *full owner* is role `owner` with the `full` scope (an owner's browser session, bearer session or Android app). A full owner sees every item, every library and every download.
- Any other owner credential (a `library:read` token on a shared living-room TV, the Chrome extension's `downloads:create downloads:read`) sees every library, and otherwise only what a member would: its own Other files and incoming items, and downloads under the member rule below. A limited token never opens folders not in a library, other people's Other files, uploads or waiting files, or the trash.
- A member sees what the table says.

A library is visible to the caller when the caller's role is `owner` (whatever the scopes), or its `visibility` is `everyone`, or the caller is listed in `library_members` for it; otherwise it does not exist for them.

| `items.place` | Visible to a member or a limited owner credential when |
|---|---|
| `library` | the library is visible to them |
| `other_files` | `added_by` is them |
| `incoming` | `added_by` is them, or the item waits for review (`needs_review`), its kind is claimed, and the claiming library is visible to them |
| `folder`, `trash` | never |

Enforcement, route by route (an invisible item or library is "not found": the same 404 `not_found` and message as for one that never existed, "That item no longer exists." or "That library no longer exists.", never 403):
- `GET/PATCH /api/items/:id`, `/thumb`, `/file`, `/view`, `/playback`, `/progress`, `POST /api/inbox/:id`, `/organize` (full owner only anyway): `can_see` before anything else, on every request. So an Inbox accept on an item that someone just sorted into a library the caller cannot see answers 404; the 409 "That file was already sorted." is only for an item the caller still sees.
- Every `/api/libraries/:id` route (`GET /api/libraries/:id`, `/shows`, `/shows/:name`, `/timeline`) answers 404 for a library the caller cannot see before it looks at the library's type, the path or the query, so the existence or the type of a hidden library cannot be probed (the starting libraries' ids are published in section 6).
- `GET /api/items`, `/api/libraries/:id/shows`, `/shows/:name`, `/timeline`, `/api/home`, `/api/inbox`: the predicate is part of the query, so lists, counts, cursors, `next_cursor`, search (`q`), favourites (`favorite=true`), years and totals only ever cover visible items. A list filtered by an invisible `library_id` answers 404.
- Shows: grouped per library. A show's seasons, episode count, poster and `next_episode_id` / `previous_episode_id` come only from episodes in the same library, so an episode in another library (visible or not) is never offered as next and never pictures the show.
- Continue watching and progress: only visible items are returned. Progress rows of an item a person can no longer see are kept and come back if access returns; `PUT /progress` on an invisible item answers 404 and saves nothing.
- Favourites (`items.favorite`, one flag the household shares) can be read and set only on visible items.
- HLS: a session belongs to the person who started it; another person's requests for it answer 404. Every playlist, segment and `DELETE /api/stream/:session` request checks that the session's person still sees the item; if not, it answers 404 and the server stops the session (ffmpeg killed, folder deleted).
- Downloads (`GET /api/downloads`, the `download` event, pause, resume, delete): a download is seen by the person who created it and by full owners, in every state. Anyone else with `downloads:read` sees it only while it is `sorted` and at least one item linked to it (`download_items`) is visible to them. So a download that is queued, downloading, paused, seeding, completed, sorting, failed or cancelled, a sorted one whose files all went where they cannot look, and a sorted one whose linked items are all gone (trash emptied, purged as missing, which deletes the `download_items` rows) is not listed for them, its events are not sent to them, and pause, resume and delete answer 404. Uploads and shares (`kind: upload`, whose `name` is the person's file name) are therefore private to their sender and full owners until their file lands somewhere others can see. `item_ids` and `destinations` list only items visible to the caller, for the creator too (a sorted download whose files the creator cannot see shows no destination). When a download leaves someone's view, their connections get `download` `{id, removed: true}` (Live events, below).
- Trash: full owners only (unchanged), so it needs no filter.
- Library definitions: everyone but full owners gets only visible libraries, with `position` counted among them (0 to k-1 in order, so a hidden library leaves no gap), and never `folder`, `visibility`, `member_ids`, `folder_state` or `is_empty` (null in `Library`).
- Live events: see below.

**While watching.** Access is checked on every request, not once per playback: after a library is hidden from someone, their next HLS playlist or segment request, next direct-play range request and next progress save answer 404, and the HLS session is stopped. Clients then say "This video is no longer available." and go back. Seconds already buffered on the device may still play.

**Caching.** Thumbnails, photo views and files are served `private`. What a device already loaded stays in its own cache until it is cleared; losing access stops every new request, not what a browser already holds. The service worker caches no `/api/*` response.

**Outside the app.** Library access is enforced by PopcornStash only. Anyone who can open the stash share (SMB, NFS) or the server's disk reads every folder, hidden libraries, Other files and folders not in a library included. Family members get no share access, or write access to `Inbox/` only (PRODUCT.md, Honest boundaries; docs/security.md and docs/deployment.md say the same once they describe libraries).

**Live events.** Every published event carries, server side only, what decides who may see it: the item's `library_id`, `place`, `added_by`, `status` and kind for `library` and `job` events; the creator, state and linked item ids for `download` events. Each SSE connection keeps its caller's user id, role, scopes, visible library ids and the claims map (kind to library), and reloads them whenever that person's access may have changed: after any library write (create, edit, order, people, delete), a role change (`PATCH /api/users/:id` with `role`), and at the 60 s re-check. The server tells the affected streams at once through an internal per-person "access changed" signal, so a demoted owner stops receiving owner-level events immediately; deleting a person closes their streams. Then:
- Scopes first, as today: `library`, `job`, `inbox` and `libraries` events need `library:read`; `download` events need `downloads:read`; `vpn` goes to every connection.
- `library` and `job` events about an item go only to connections that can see the item; `job` events without an item go to every connection with `library:read` only for a scan; any other (a sort job) goes to full owners only, because its timing would tell a member that files bound for a library they cannot see arrived or were sorted. A change that takes an item out of someone's view (Organize into another library, a claim or access change, a move found by a rescan, the trash) reaches them as `library` `{type:"removed", item_id, library_id: null}`; one that brings it into view reaches them as `added`. A `removed` event never names a library, so it cannot hand out the id of one the person cannot see; clients drop the item wherever they show it.
- `download` events are rewritten per connection exactly as `GET /api/downloads` would answer that person. When a download leaves someone's view (its last visible item moves out of view, access changes, its linked items are deleted), each of their connections that was sent it gets `download` `{id, removed: true}` and nothing more about it.
- `inbox` `{count}` is computed per connection: the number of waiting items that person can see. It is sent only when that number differs from the last one the connection was sent (or counted when it opened), so a file that starts waiting for a library someone cannot see sends them nothing.
- A change that moves more than 2000 items in or out of a connection's view at once (a large library hidden, deleted or made from a folder) sends no item events for it; the `libraries` event that comes with it makes clients reload what they show.
- The downloads a connection can be told were removed are those listed for its caller when the stream opened (up to 500, as `GET /api/downloads?state=all` answers) and those sent on it since.
- `libraries` `{}` goes to a connection only when its own answer to `GET /api/libraries` changed, counts aside: a library appeared for it or disappeared, or one it sees changed its name, icon, type, structure, claims, layout or position (for full owners also its folder state, visibility or people). A change to a library someone cannot see sends them nothing. It carries nothing; clients refetch `GET /api/libraries` (and the page they show).

**Required tests** (besides the migration test in section 6): a member with a hidden library in the middle of the order gets positions 0 to k-1 with no gap; an owner `library:read` token sees every library but not folders not in a library, another person's Other files or waiting upload, or the trash; an owner Chrome token (`downloads:create downloads:read`) does not see another person's upload or in-progress download; a member's SSE stream receives no `library`, `job` or `inbox` event without `library:read`, no `job` or `inbox` event when a file claimed by a library they cannot see is sorted or starts waiting, no `libraries` event for a change to a library they cannot see, and `{id, removed: true}` when a download leaves their view; a role change takes effect on an open stream without waiting for the 60 s re-check.

## 8. HTTP API

JSON, snake_case. Errors: `{"error": {"code": "not_found", "message": "Human readable sentence.", "field": "name"}}` with matching HTTP status. Codes: `bad_request`, `unauthorized`, `forbidden`, `not_found`, `method_not_allowed` (405), `conflict`, `payload_too_large` (413), `rate_limited`, `setup_required`, `passkeys_unavailable`, `vpn_unavailable`, `internal`. `field` is present only when one request field is at fault; it names it as the request spells it (`name`, `icon`, `content_type`, `structure`, `claims`, `folder`, `folder.mode`, `folder.name`, `visibility`, `member_ids`, `ids`, `place`, `library_id`, `title`, `show_name`, `episode`), so a form can put the message under that field. Messages are complete sentences meant for people and may be shown as they are.

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
  thumb: string|null /* URL */, favorite: bool, added_at, progress: {position_s, duration_s, finished}|null,
  library_id: string|null, place: "library"|"other_files"|"incoming"|"folder"|"trash"
}
ItemDetail = Item & { media: MediaSummary|null, mime|null, suggestion: Classification|null, source_download_id|null,
  next_episode_id|null, previous_episode_id|null /* same library only */ }
Classification { category, title, year, show_name, season, episode, episode_title, taken_at, confidence: "high"|"low",
  reason /* the classifier's sentence about the file ("Not sure this is a movie."); stored, never about libraries */,
  place: "library"|"other_files"|null /* null: no library the reader can see claims this kind */,
  library_id: string|null /* the claiming library, worked out when read; null when unclaimed or not visible to the reader */,
  destination: string|null /* rel path the file would get; null when place is null */,
  waiting: "choose_library"|"folder_unavailable"|"low_confidence"|"confirm"|null /* why a needs_review item waits, worked out when read, first match wins:
    choose_library: a claimable kind and no library the reader can see claims it;
    folder_unavailable: the claiming library's folder is missing or read-only;
    low_confidence: the classification is Low (show `reason`);
    confirm: anything else (auto-sort was off, or the sort job stopped because the claims changed); the suggestion only needs accepting.
    null for items that are not needs_review */ }
Download { id, kind: "torrent"|"http"|"upload", name, source, state, progress, total_bytes|null, done_bytes,
  down_bps, up_bps, peers|null, eta_s|null, error|null, created_at, completed_at|null, item_ids: string[] /* visible only */,
  destinations: [{place: "library"|"other_files"|"incoming", library_id: string|null, count: number}] /* where its visible items are now, grouped, in first-seen order */ }
DownloadRemoved { id, removed: true } /* `download` event only: the download left the caller's view (section 7) */
LibraryIcon = string /* one of the names in the icon list below */
Library {
  id, name, icon: LibraryIcon, content_type: "video"|"photos", structure: "episodes"|"one_offs"|null /* null for photos */,
  claims: ("movie"|"episode"|"home_video"|"photo")[],
  layout: "posters"|"frames"|"series"|"timeline" /* how clients draw it, below */,
  position: number /* index in the caller's own list, 0 to k-1; a hidden library leaves no gap */,
  item_count: number /* ready items */, show_count: number|null /* episodes structure only */,
  created_at, updated_at,
  /* full owners only; null for everyone else: */
  folder: string|null, visibility: "everyone"|"selected"|null, member_ids: string[]|null,
  folder_state: "ok"|"missing"|"read_only"|null /* section 3.1 */, is_empty: boolean|null /* type and structure can change only while true */
}
UnassignedFolder { name, item_count /* video_count + photo_count + other_count */, video_count, photo_count, other_count,
  suggested: {content_type: "video"|"photos", structure: "episodes"|"one_offs"|null} }
HomeLibrary { library: Library, recent: Item[] /* up to 12 newest ready items, newest first */,
  show_posters: {[show_name]: item_id} /* for each show with an episode in recent, from this library */ }
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
| GET | `/api/home` | library:read | → `{continue_watching: Item[] /* up to 12, visible items in any library */, libraries: HomeLibrary[] /* every library the caller sees, in order */, other_files_count, inbox_count /* waiting items the caller sees */, total_bytes /* of the caller's visible ready items */}` |
| GET | `/api/libraries` | library:read | → `{libraries: Library[]}`: the libraries the caller sees, in order |
| GET | `/api/libraries/:id` | library:read | → `Library` (404 when not visible) |
| POST | `/api/libraries` | owner | create (Libraries, below) → 201 `Library` |
| PATCH | `/api/libraries/:id` | owner | edit (below) → `Library` |
| PUT | `/api/libraries/order` | owner | `{ids: string[]}` (every library exactly once) → `{libraries: Library[]}` |
| DELETE | `/api/libraries/:id` | owner | → 204 (files untouched, below) |
| GET | `/api/libraries/folders` | owner | → `{folders: UnassignedFolder[]}` (top-level folders not in a library, by name) |
| GET | `/api/libraries/icons` | library:read | → `{icons: LibraryIcon[]}` in picker order |
| GET | `/api/libraries/:id/shows` | library:read | Episodes libraries → `[{show_name, item_count, season_count, poster_item_id, last_added_at}]`; other libraries 400 |
| GET | `/api/libraries/:id/shows/:name` | library:read | → `{show_name, poster_item_id|null, seasons:[{season, episodes: Item[]}]}` (this library's episodes of that show, a stored name with stray spaces included; 404 "That show is not in this library." when it has none) |
| GET | `/api/libraries/:id/timeline` | library:read | Photos libraries, `?cursor&limit` → `{months:[{month:"2024-06", count, items: Item[]}], next_cursor}`; other libraries 400 |
| GET | `/api/items` | library:read | `?library_id&place&category&q&sort=added|title|taken|size&order=asc|desc&favorite&year&status&cursor&limit` → page of visible `Item`. `status` defaults to `ready`; `status=trashed` is refused (400, the trash has its own route). `library_id` of a library the caller cannot see → 404. `place=other_files` lists Other files; `place=folder` lists folders not in a library (full owners; empty for everyone else) |
| GET | `/api/items/:id` | library:read | → `ItemDetail` |
| PATCH | `/api/items/:id` | full | `{title?, year?, favorite?, show_name?, season?, episode?, episode_title?}` → `ItemDetail` (metadata only, no move) |
| POST | `/api/items/:id/organize` | owner | `{place?: "library"|"other_files", library_id?, category?, title?, year?, show_name?, season?, episode?}` → `ItemDetail` (moves the file; explicit action; target rules as for Inbox accept, below) |
| DELETE | `/api/items/:id` | owner | → 204 (moves to `.popcornstash/Trash`) |
| GET | `/api/items/:id/thumb?size=sm|lg` | library:read | → `image/webp` (404 when not ready) |
| GET | `/api/items/:id/file?download=1` | library:read | → original bytes, Range supported |
| GET | `/api/items/:id/view` | library:read | → browser-friendly photo (original or rendition) |
| POST | `/api/items/:id/playback` | library:read | `{video_codecs: string[], audio_codecs: string[], hdr: bool, max_height?: number, start_s?: number, audio_index?: number}` → `{mode:"direct", url}` or `{mode:"hls", url, session_id}` |
| GET | `/api/stream/:session/index.m3u8`, `/api/stream/:session/:segment` | library:read | HLS playlist/segments |
| DELETE | `/api/stream/:session` | library:read | → 204 |
| PUT | `/api/items/:id/progress` | library:read | `{position_s, duration_s}` → 204 |
| GET | `/api/inbox` | library:read | → `{items: ItemDetail[] /* waiting (needs_review), oldest first */, ignored: ItemDetail[] /* ignored, still in Inbox/ or Downloads/Completed/ */}`, visible items only |
| POST | `/api/inbox/:id` | full | `{action:"accept"|"ignore", place?: "library"|"other_files", library_id?, category?, title?, year?, show_name?, season?, episode?}` → `ItemDetail` (accept rules below; accepting an ignored item works the same; web and Android always send the `place` and `library_id` the row showed) |
| POST | `/api/library/rescan` | full | → 202 `{job_id}` |
| GET | `/api/trash` / POST `/api/trash/:id/restore` / DELETE `/api/trash` | owner | list / restore / empty (restore into a library that would not show the file answers 409, section 11) |
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
| POST | `/api/vpn/nordvpn` | owner | `{access_token, country_id?, protocol:"wireguard"|"openvpn"}` → `VpnStatus`; 409 `conflict` while torrents run without a VPN (below the table) |
| POST | `/api/vpn/wireguard` | owner | multipart `file` or JSON `{config}` → `VpnStatus`; 409 `conflict` while torrents run without a VPN (below the table) |
| POST | `/api/vpn/openvpn` | owner | multipart `file`, `username`, `password` → `VpnStatus`; 409 `conflict` while torrents run without a VPN (below the table) |
| POST | `/api/vpn/reconnect` | owner | → `VpnStatus` |
| DELETE | `/api/vpn` | owner | → `VpnStatus` |
| GET/PUT | `/api/settings` | owner | the settings object (partial PUT merges) |
| GET | `/api/system` | owner | → `{version, source, stash_root, config_dir, disk:{total, free}, ffmpeg_version, hwaccel:{configured, available: string[], set_by_env}, torrentd:{reachable}, jobs:{queued, running, failed}, public_url, passkeys_ready}`. `source` as in `/api/health`; `configured` is the acceleration chosen (by `STASH_HWACCEL` or the saved setting); when it is not in `available`, videos are converted on the processor (libx264) and Settings says so; `set_by_env` is true when `STASH_HWACCEL` sets it |
| GET | `/api/users` / PATCH `/api/users/:id` `{role}` / DELETE `/api/users/:id` | owner | |
| GET/POST/DELETE | `/api/invites[/:id]` | owner | create `{role, expires_in_hours}` → `{id, url, code, role, expires_at}` |
| GET | `/api/audit?limit&cursor` | owner | → page of `{id, at, user, event, detail, ip}` |
| GET | `/api/events` | any | SSE, filtered per connection (section 7, Live events): `download` (Download, or DownloadRemoved when it leaves the caller's view), `library` `{type:"added"|"updated"|"removed", item_id, library_id|null}`, `libraries` `{}`, `vpn` (VpnStatus), `inbox` `{count}`, `job` `{kind, state, item_id}` |

The VPN setup routes (`POST /api/vpn/nordvpn`, `/api/vpn/wireguard` and `/api/vpn/openvpn`) answer 409 `conflict` while torrents run without a VPN (`STASH_ALLOW_NO_VPN`: the status reports `allow_no_vpn: true` with `torrents_off: false`), before reading the body or contacting NordVPN, with the message "Torrents run without a VPN on this server, so a VPN saved here would not be used. To use one, remove STASH_ALLOW_NO_VPN from the container settings and recreate the container." `DELETE /api/vpn` still removes a saved VPN then.

### Libraries

These replace the fixed-category routes, which are removed and answer 404: `GET /api/library/summary` (now `GET /api/home`), `GET /api/shows` and `/api/shows/:name` (now per library), `GET /api/photos/timeline` (now per library). `GET /api/items?category=` still filters by kind, across the libraries the caller sees.

**Create** `POST /api/libraries` (owner):
```json
{"name": "Kids", "icon": "baby", "content_type": "video", "structure": "episodes",
 "claims": [], "folder": {"mode": "new", "name": "Kids"},
 "visibility": "selected", "member_ids": ["0192..."]}
```
- `name` (required): trimmed, 1 to 40 characters, no control characters; unique among libraries ignoring case; not one of the fixed places (`Home`, `Other files`, `Inbox`, `Downloads`, `Settings`, ignoring case).
- `icon` (required): exactly one name from the icon list (case-sensitive).
- `content_type` (required): `video` or `photos`. `structure`: required (`episodes` or `one_offs`) for video, absent or null for photos.
- `claims` (default `[]`): kinds that fit (section 4). A kind another library claims moves to this one in the same transaction; the other library loses it. Clients show the current holder beside each choice before saving ("Sorted into Films now").
- `folder` (required): `{"mode": "new", "name"}` makes the folder; `{"mode": "existing", "name"}` takes a folder from `GET /api/libraries/folders`. Rules in section 3.1. Web and Android prefill a new folder's name with the library name.
- `visibility`: `everyone` or `selected`. Required with `folder.mode = "existing"` (400, field `visibility`, "Choose who sees this library."): that folder's files were seen by owners only, so making it a library never shares them by default. With `folder.mode = "new"` it defaults to `everyone` (the folder starts empty). `member_ids` (default `[]`): members who see a `selected` library; ignored for `everyone` and kept for a later switch back. Owners always see every library and cannot be listed. `selected` with no members reads "Only owners".
- `position` (optional): where to insert; default last. At most 24 libraries.
- Effects: the folder is created or taken and the answer comes back at once. For a taken folder, the same transaction (under the move lock) lets the items the library indexes join it and takes the rest out of the index (section 4, Files already in a library); a scan job then indexes whatever was not indexed yet, so `item_count` grows and `library` events report items as it runs. `libraries` events are sent (section 7) and an audit entry `library_created` is written.

**Edit** `PATCH /api/libraries/:id` (owner): any of `name`, `icon`, `content_type`, `structure`, `claims` (the full new set), `visibility`, `member_ids` (the full new set). `folder` is refused. Changing `content_type` or `structure` is allowed only while the library holds no items (`is_empty`: no item with this `library_id` in any status, so missing files count); the folder is then indexed again under the new rules. Claims are checked against the type and structure after the change, so one request may change both; a listed kind that another library holds moves here, as in create, and a kind left out is no longer claimed by anyone. A type or structure change without `claims` keeps the current claims and is refused (400, field `claims`) when one of them no longer fits. Visibility and people apply at once to every route and open stream (section 7). Audit entry `library_changed` with the names of the fields changed.

**Order** `PUT /api/libraries/order` (owner): `{ids}` must list every library exactly once; positions become 0..n-1 in that order. Audit entry `libraries_reordered`.

**Delete** `DELETE /api/libraries/:id` (owner): in one transaction, under the move lock and in this order, sets its items to `library_id = NULL, place = 'folder'` (seen by owners only), keeping their ids, favourites and progress; then deletes its claims, its people and the library row (section 6). The folder and every file stay. Its claimed kinds become unclaimed, so new files of those kinds wait in Inbox, and every waiting suggestion that pointed at it now has no library. Open HLS sessions of members who no longer see the item stop at their next request. Making a library from the same folder later brings back, with their history, the items that library indexes; the owner chooses again who sees it (Create, `visibility`). Audit entry `library_deleted` with the name and folder.

**Folders** `GET /api/libraries/folders` (owner): every top-level folder that is not a library folder or reserved, with counts of the items indexed in it by kind (videos, photos, everything else) and a suggested type: `photos` when most of its items are photos; `video` + `episodes` when most of its videos are episodes; otherwise `video` + `one_offs`. "Make a library" sends a create with `folder.mode = "existing"`.

**Icons** `GET /api/libraries/icons`: the curated list below, in picker order. The list is a constant in `stash-core` (`library::LIBRARY_ICONS`), checked on every create and edit. Each name is a Lucide icon name (kebab case) from the Lucide release the web app pins (`lucide-react` 1.55.0); the web draws it with lucide-react and Android draws the same glyphs, generated from that lucide-react package into Kotlin path data (`android/README.md`, Icons). The fixed places' icons (`house`, `file-text`, `inbox`, `arrow-down-to-line`, `settings`) are not in the list, so a library never looks like Home or Inbox, and neither is `folder`: it marks a folder that is not in a library (Settings, Folders not in a library) and is what a client draws for a name it does not know (a newer server), so it never stands for an icon the owner chose. Two libraries may share an icon. Adding or removing a name is a contract change for the server, web and Android together.

Each icon has one plain name in sentence case, the same on the web (`web/src/lib/libraryIcons.ts`) and Android (`ui/icons/LibraryIcons.kt`): the picker's tooltip and the name screen readers announce. Android's unit tests compare both clients' tables and the server's list (Gradle declares the two files as test inputs).

| Name | Plain name | Name | Plain name | Name | Plain name |
|---|---|---|---|---|---|
| `film` | Film | `users` | People | `sun` | Sun |
| `clapperboard` | Clapperboard | `baby` | Baby | `snowflake` | Snowflake |
| `tv` | TV | `heart` | Heart | `gamepad-2` | Game controller |
| `popcorn` | Popcorn | `cake` | Cake | `music` | Music |
| `projector` | Projector | `party-popper` | Party popper | `book-open` | Book |
| `video` | Video camera | `graduation-cap` | Graduation cap | `trophy` | Trophy |
| `drama` | Theatre masks | `gift` | Gift | `paw-print` | Paw print |
| `image` | Picture | `plane` | Plane | `bike` | Bike |
| `images` | Pictures | `tree-palm` | Palm tree | `star` | Star |
| `camera` | Camera | `mountain-snow` | Mountain | | |
| `aperture` | Aperture | `tent` | Tent | | |

```
film, clapperboard, tv, popcorn, projector, video, drama,
image, images, camera, aperture,
users, baby, heart, cake, party-popper, graduation-cap, gift,
plane, tree-palm, mountain-snow, tent, sun, snowflake,
gamepad-2, music, book-open, trophy, paw-print, bike,
star
```

An edit (`PATCH`) checks `icon` only when it is sent; both clients send it only when the owner changed it, so a library that has an icon no longer in the list keeps it until the owner picks another.

**Layout** (`Library.layout`, worked out by the server, so web and Android agree): `timeline` for Photos; `series` for Video Episodes; for Video One-offs, `frames` when it claims camera videos and not movies, `posters` when it claims movies, and otherwise (claims neither) `posters` when at least half of its ready videos are movies, else `frames`.

**Errors** for library writes:

| Situation | Status | `code` | `field` |
|---|---|---|---|
| Not an owner | 403 | `forbidden` | |
| Library missing or not visible | 404 | `not_found` | |
| Name empty, longer than 40 characters, or with control characters | 400 | `bad_request` | `name` |
| Name used by another library or a fixed place | 409 | `conflict` | `name` |
| Icon not in the list | 400 | `bad_request` | `icon` |
| Unknown `content_type`, or `structure` missing for video or set for photos | 400 | `bad_request` | `content_type` or `structure` |
| Type or structure change while the library holds items: "Change the type only while the library is empty. Move its files out first, or make a new library." | 409 | `conflict` | `content_type` or `structure` |
| A claim that does not fit the type and structure, or an unknown kind | 400 | `bad_request` | `claims` |
| `folder` in an edit: "A library's folder cannot change." | 400 | `bad_request` | `folder` |
| Folder name invalid or reserved | 400 | `bad_request` | `folder.name` |
| New folder: something with that name exists | 409 | `conflict` | `folder.name` |
| Existing folder: not found, not a directory, or a symlink | 404 | `not_found` | `folder.name` |
| Existing folder: already a library's folder | 409 | `conflict` | `folder.name` |
| The folder cannot be created or read (permissions, read-only share): the layout sentence that names the folder and PUID:PGID | 409 | `conflict` | `folder.name` |
| `visibility` missing when taking an existing folder: "Choose who sees this library.", or not `everyone` or `selected` | 400 | `bad_request` | `visibility` |
| `member_ids` with an unknown id or an owner | 400 | `bad_request` | `member_ids` |
| A 25th library | 409 | `conflict` | |
| Order that does not list every library exactly once | 400 | `bad_request` | `ids` |

**Inbox accept and Organize: the target.** `POST /api/inbox/:id` with `action: "accept"` and `POST /api/items/:id/organize` take the same target fields. Checks run in this order: the item (404 when the caller cannot see it, section 7), then the target, then the item's state.
- Web and Android always send the `place`, and for a library the `library_id`, that the row showed, also for "Accept all suggestions", so a file never moves to a library the person did not see on screen. A target that changed meanwhile fails visibly (404 for a library that was deleted or hidden, 409 below) instead of moving the file somewhere else.
- `place` omitted (other API callers): `library_id` alone names a library; otherwise the suggestion decides as it reads at that moment (`other_files` for `other`; otherwise the library in `suggestion.library_id`, after a `category` in the body when one is sent). When the suggestion has no library the reader can see: 400 `bad_request`, field `library_id`, "Choose a library for this file." For Organize of an item without a suggestion (one already in a library or in Other files), the item's own library or Other files.
- Who may choose what (Inbox accept): the person who added the file (`added_by`) and full owners may choose any library they see that fits, or Other files. Anyone else sees the item only because a library they see claims its kind (section 7), and may accept it only into that claiming library, as it is now: Other files, or a library that does not fit the file's kind, answers 400, field `library_id`, "Only the person who added this file can send it somewhere else."; a library that fits the kind but no longer claims it (the row was shown before the claim moved) answers 409, field `library_id`, "Where this file goes changed. Check it and accept again." The server keeps no history of claims, so a fitting library a person picked by hand is treated as a stale row and also gets the 409; clients never offer other libraries to these people. Ignore stays open to anyone who sees the item. Organize is for full owners and works on any item, wherever it is.
- `place: "library"` with `library_id` (or the suggested one): the caller must see the library (else 404, field `library_id`), and its folder must be available (section 3.1; else 409, field `library_id`, "This library's folder is missing or cannot be written, so nothing can move there now."). The file must fit it:
  - Photos: a photo or video file; becomes `photo`, laid out `YYYY/MM/`.
  - Video, Episodes: a video file; becomes `episode`; needs a show name and an episode number (from the body, else the suggestion; season 1 when none): missing ones are 400 with field `show_name` or `episode`.
  - Video, One-offs: a video file; becomes `category` from the body when that is `movie` or `home_video`, else the suggestion's kind when it is one of those, else the library's in-place kind (section 4); a movie needs a title.
  - Anything else is 400, field `library_id`: "Photos go in a photos library." for a photo into a Video library, "Only videos and photos go in a library. This file goes in Other files." for songs and documents.
- `place: "other_files"`: any file; becomes `other`, moved to `Other/<name>`.
- Inbox accept on an item that is no longer waiting or ignored (accepted or moved by someone else a moment ago) is 409 `conflict` "That file was already sorted.", but only while the caller still sees the item; one that went where they cannot look is 404 like any invisible item. Accept needs `full`.

"Accept all suggestions" (web Inbox, Android Downloads) accepts, one request per item, every waiting item whose suggestion has a `place`, oldest first, each request naming the `place` and `library_id` its row shows, and leaves the rest with "Choose a library for N files." Items whose request fails stay listed with their error.

Non-API routes: `/` and all client routes serve the SPA (`index.html`, `Cache-Control: no-cache`); `/assets/*` immutable; `/manifest.webmanifest`; `/sw.js`; `/share` (PWA share target: Android posts `multipart/form-data` with `title`, `text`, `url` and `files` to `/share`. The service worker (`sw.js`) answers it. Files are kept in Cache Storage on the device (`popcornstash-shared-files-v1`, deleted once uploaded or cancelled, after 7 days, or when anyone signs out on the device) and the page opens `/share?files=<id>`. The web app remembers interrupted uploads in localStorage (`popcornstash.uploads.v1`) with the user id of the person who started each one, and continues them only for that person; signing out cancels that person's partial uploads (`DELETE /api/uploads/:id`) and forgets them. A share without files opens `/share?title&text&url`, and a failure opens `/share?failed=files`. The server serves only GET; a POST that reaches it gets 405.); `GET /third-party-licenses.txt` (the web app's npm notices, image builds); `/.well-known/assetlinks.json` (from `STASH_ANDROID_ASSETLINKS`, Android passkeys, section 7).

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
  - Sidebar (and the phone menu): Home, then every library the person sees (`GET /api/libraries`, in order, each with its icon and name), then Other files, Inbox, Downloads, Settings. The phone quick links are Home, the first three libraries the person sees, and Downloads.
  - Routes: `/libraries/:id` (the library page for its `layout`), `/libraries/:id/shows/:name`, `/other` (Other files: `GET /api/items?place=other_files`), `/item/:id`, `/inbox`, `/downloads`, `/settings/libraries` (owner: the list, folders not in a library, sorting, rescan, trash), `/settings/libraries/new`, `/settings/libraries/:id`. `/settings/library` redirects to `/settings/libraries`.
  - Old addresses redirect (replace, no history entry) to the library that claims the kind, when the person sees it, else to Home: `/movies` (movie), `/shows` and `/shows/:name` (episode, to `/libraries/:id/shows/:name`), `/home-videos` (home_video), `/photos` (photo).
  - A `libraries` event refetches the list; a library page whose library disappears shows the not-found page with "Go to Home".
  - Inbox accepts, one at a time or "Accept all suggestions", always send the `place` and `library_id` the row shows (section 8). A `download` event with `removed: true` drops that row from Downloads.
  - Who may pick where a waiting file goes: `ItemDetail` does not say who added a file, so the web decides from the row. Owners get every library they see that fits the file, and Other files. A member whose row's suggestion names a library (`suggestion.place: "library"` with a `library_id`) may be looking at someone else's file, so the Selector shows that one library and nothing else; with no claiming library, or for Other files, the member can only be seeing the file because they added it (section 7), and gets the whole choice. A member who added a file of a claimed kind can therefore only accept it into the claiming library or ignore it on the web, although the server would let them choose; owners can still move it.
  - Organize (`POST /api/items/:id/organize`) is offered to owners only, on any item that is not in the trash ("Organize into library", or "Move to another library" for an item already in one), with the same Library selector, details and preview as Inbox.
  - `/settings/libraries/new?folder=<name>` opens the editor with that folder taken (`folder.mode = "existing"`), the name and type from the folder's `suggested`, and "Who sees it" on "Only some people" with nobody chosen. Saving a create or an edit that takes a kind another library claims asks first ("Sort TV episodes into Kids?"). An edit sends only the fields that changed, and `claims` whenever the type or structure changes.
  - A sorted download names where it went from `destinations`: each library the person sees ("In Kids", a link), Other files, or how many files wait in Inbox; nothing named for a library the person cannot see, and "Sorted" alone when no destination is left to show.
- **Chrome extension** (`extension/`): Manifest V3. Device pairing (section 8), context menu "Send to PopcornStash" for links/media/magnets, optional "send all browser downloads" with optional cookie forwarding (optional host permissions), popup with recent downloads. It never reads library routes.
- **Android app** (`android/`): Kotlin, Jetpack Compose, Material 3 themed with the Popcorn tokens (`android/DESIGN.md`), package `com.thekozugroup.popcornstash`, minSdk 28, latest stable compile and target SDK, edge to edge, light and dark from the system. Gradle Kotlin DSL with a version catalog; the wrapper is committed. It uses only the API in section 8, with `Authorization: Bearer` on every request, SSE included.
  - **Server address:** `https://` only; `http://` is accepted for `localhost`, `127.0.0.1` and `10.0.2.2` in debug builds only. The address is reduced to scheme, host and port, then checked with `GET /api/health` (must answer `status: "ok"`) and `GET /api/auth/state`.
  - **Pairing:** `POST /api/pairing/request` `{client_name: "<device model>", client_kind: "android"}`, show `user_code` large, open `verify_url` (`<server>/pair`) in a Custom Tab, poll `POST /api/pairing/poll` every `interval` seconds until 200 (store `token`; scopes `full`), 410 (say "Expired" or "Not allowed" with "Start again") or 404 (start again). A poll that does not get through (no connection, 429, a 5xx) is tried again at the next interval until `expires_at` has passed.
  - **Passkeys:** offered only when `GET /.well-known/assetlinks.json` lists `com.thekozugroup.popcornstash` with `delegate_permission/common.get_login_creds`. `POST /api/auth/login/start` `{}` → Credential Manager `GetPublicKeyCredentialOption` with the `options.publicKey` JSON → `POST /api/auth/login/finish` `{flow_id, credential, want_token: true, device_name}` → `{user, token}` (a bearer session, section 7). Both requests carry no `Origin` (section 7, CSRF). The server accepts the app's `android:apk-key-hash:` origin (section 7).
  - **Token storage:** encrypted with AES-GCM under a non-exportable Android Keystore key, kept in app-private storage (`noBackupFilesDir`), never logged (no HTTP body or header logging in release builds; `Authorization` redacted in debug). The token never leaves the app's process: no other app, intent or system service (DownloadManager included) is ever given it.
  - **No backups:** `android:allowBackup="false"`, and `dataExtractionRules` (Android 12 and later) and `fullBackupContent` (earlier) exclude every app data domain from cloud backup and device transfer, not only the token, because caches and lists hold the family's pictures and names.
  - **Leaving a person's data behind:** sign out, a 401 on any request, and signing in as a different person all clear everything the app kept for the person before: the token and key, Coil's memory and disk caches, the OkHttp cache, any Media3 cache, the private copies of opened files (below), every tab's view models and back stack, and upload notifications. Sign out also calls `POST /api/auth/logout` and cancels running uploads (`DELETE /api/uploads/:id`); a 401 then returns to sign-in.
  - **Tabs:** Downloads (left), Home (centre, the start tab), Settings (right) in a floating toolbar; design in `android/DESIGN.md`. In a right-to-left language the toolbar mirrors (Settings on the left) and so do the slides: a page always comes in from the side of the tapped square, and a page opened inside a tab from the end side.
  - **Home:** `GET /api/home`. The carousel is `continue_watching`; below it every `HomeLibrary` with its icon, name and `recent` (a series library's episodes grouped by show, pictured by `show_posters`). A library opens with `GET /api/libraries/:id`, then `GET /api/items?library_id=&sort=&order=&limit=60` paged by `cursor` (one-offs, `layout` posters or frames; posters sort by `added`, `title` or `size`, frames by `taken` first), `GET /api/libraries/:id/shows` and `/shows/:name` (series), or `/timeline?limit=200` paged by `cursor` (photos, full-screen viewer with `/api/items/:id/view`). Other files (when `other_files_count > 0`) opens `GET /api/items?place=other_files`. Pictures are `Item.thumb` (`size=lg` for posters, frames and the item page) and a show's `/api/items/:poster_item_id/thumb`, loaded by Coil with the session's client; its memory cache and its disk cache (`cacheDir/images`) end with the session. Live updates: a `library` event that names an open library, or removes an item a page shows, loads that page again (as many items as were loaded, after a 400 ms pause that gathers a burst; past the server's largest page, 200 items or 500 photos, it follows `next_cursor` until it has them all, so nothing the person scrolled through is dropped; Other files does the same); a `libraries` event refetches `GET /api/libraries`, and pages of a library no longer listed (or answering 404) close with "Kids is not available any more."; every open page loads again when the events stream comes back after a gap. The photo viewer's "Save the original" asks the system's "Save to" picker (`ACTION_CREATE_DOCUMENT`) for a place and streams `GET /api/items/:id/file?download=1` into it with the app's own client: no DownloadManager, and the token stays in the app.
  - **Opening an Other file:** the app fetches `GET /api/items/:id/file` itself with OkHttp into a private folder under `cacheDir` (`opened/`), then starts `ACTION_VIEW` with a FileProvider `content://` URI and a one-time read grant (`FLAG_GRANT_READ_URI_PERMISSION`, no write grant). Never DownloadManager, shared storage or the phone's Downloads, so the file and the token stay out of other apps' reach and do not outlive the person's session. The copy is deleted when the person comes back to the app, at the next start, and on sign out. When no app can open the type: "No app on this phone opens this kind of file." The viewer app that was given the file can still keep its own copy; that is outside PopcornStash's control.
  - **Item and player:** `GET /api/items/:id` (and `next_episode_id`'s item for the "Next" row); the favourite is `PATCH /api/items/:id` `{favorite}`, shown at once and put back if the server refuses it. Media3 ExoPlayer. `POST /api/items/:id/playback` with this device's real decoder capabilities from `MediaCodecList` (`video_codecs` from `h264`, `hevc`, `av1`, `vp9`; `audio_codecs` from `aac`, `mp3`, `opus`, `flac`; `hdr` from the display's HDR capabilities and an HDR-capable decoder profile; `max_height` from the decoder and display), `start_s` from `progress.position_s` when resuming, `audio_index` when chosen. `direct`: play `url` with the bearer header (Range requests). `hls`: play `url` (the playlist; segments with the bearer header), `DELETE /api/stream/:session` when leaving. Progress `PUT /api/items/:id/progress` every 10 s while playing, on pause, on leaving and when the video ends. At the end, `next_episode_id` offers "Play next episode" (no automatic start). Picture-in-picture on leaving the app while playing (auto-enter on Android 12 and later). A 404 during playback (access removed, file gone) stops with "This video is no longer available."
    - The details, as built (`android/app/.../feature/player/`): the player is its own activity (`openPlayer(itemId)`), landscape, or upright for a video taller than wide. `start_s` is sent in whole seconds and only from the first second; `max_height` is the smaller of the device's and the quality the person picked ("Original", then the web's steps below the picture's short side and the device's limit); `audio_index` only for a track other than the file's first. A progress save is skipped when the position moved less than a second, never sent before there is a position and a length, rounded to a tenth, and sent as the full length at the end. `DELETE /api/stream/:session` also goes out when the person switches audio or quality, moves to the next episode or another item, or the playback fails, and for an `hls` answer that arrives after the person moved on or left (the request is never abandoned half way). A 404 or 410 on the playlist, a segment or the file (the server ended a conversion idle for a minute, or the video went away) asks `POST /api/items/:id/playback` again at the same position, once; a 404 to that, to `GET /api/items/:id` or to a progress save is the "no longer available" stop. A direct file this phone fails to decode is asked for once more with `video_codecs: ["h264"]`, `audio_codecs: ["aac"]`, `hdr: false`, as the web player does. Nothing played is cached on disk, so there is no Media3 cache to clear.
  - **Downloads:** `GET /api/downloads?state=all` and `GET /api/vpn/status`, then `download`, `vpn` and `inbox` events. The VPN line uses the web's words; while the server does not answer (the page failed to load, or the events stream is cut off) and no VPN status was ever read, it is left out, because the page already says so and "Web links and uploads still work." would not be true (the web's Downloads page does the same while its list cannot be loaded) (everything is read again when the events stream reconnects, and on pull to refresh); pause, resume and remove (`DELETE /api/downloads/:id` with `delete_files=true` while it is in progress or failed, `false` once it finished, so sorted files are never deleted); try again on a failed link or magnet (`POST /api/downloads/:id/resume`; when that answers 400, 404 or 409, the same `source` again with `POST /api/downloads` and the old row removed with `delete_files=false`); add a link or magnet (`POST /api/downloads`), prefilled from the clipboard when it holds one (a clip marked sensitive is never read). Imports: `GET /api/inbox` (waiting and ignored), `POST /api/inbox/:id` to accept (always with the `place` and `library_id` the row shows, and an edited `title`), ignore, or accept all (section 8); each row's reason comes from `suggestion.waiting`. A finished download names where it went from `destinations`, and a `download` event with `removed: true` drops its row.
  - **Share sheet:** `ACTION_SEND` and `ACTION_SEND_MULTIPLE`. Text with an `http(s)` link or a magnet goes to Downloads after a confirm (`POST /api/downloads`). Photos, videos and other files are accepted only as `content://` URIs from another app's provider (never `file://`, never one of the app's own providers, so a share cannot make the app upload its own files); after the signed-in person confirms (the sheet names them and who uploads where), a WorkManager job uploads each through `POST /api/uploads` (with `last_modified` in unix seconds from the provider's last-modified column when it has one), `PUT /api/uploads/:id?offset=` in the server's chunk size (16 MiB) and `/complete`, with a foreground notification showing progress and a Stop action. The batch (who confirmed it, the server, the files and each file's upload id) is kept in `noBackupFilesDir`, and the app keeps its own read grant for each file until the job ends. After an interruption it asks `GET /api/uploads/:id` for `received` and continues there; a 409 with `received` also continues from the server's count. If the read grant is gone (the app was killed and the grant with it), the job deletes the partial upload and the notification says "Share the file again to finish uploading." A job runs only while the person who confirmed it is signed in on the same server, and never continues under another person: signing out stops every job and deletes their partial uploads (`DELETE /api/uploads/:id`) before `POST /api/auth/logout`; a 401 or another person signing in stops them without that step (the server deletes partial uploads after a week).
  - **Events:** `GET /api/events` over OkHttp's SSE client with the bearer header, open only while the app is in the foreground, reconnecting after 1, 2, 5, 10 then every 30 seconds.
  - **Settings:** `GET /api/me`, the server address and `/api/health` version, sign out. Owners: native library management with the same fields and rules as the web (`/api/libraries`, `/folders`, `/icons`, `/order`, `GET /api/users` for the people picker), the list reloaded on `libraries` events. The rest of the web's Libraries page (sorting, a scan, the trash) opens as "Sorting and trash" (`<server>/settings/libraries`), and VPN, people, devices, security and system open `<server>/settings/<section>`, in a Custom Tab; the browser has its own sign-in, so the person signs in there with their passkey the first time. Members see Devices and Security there.
  - **Tests:** JVM unit tests, Robolectric Compose UI tests and Roborazzi screenshots (no emulator on the build machine), and the `*ServerIntegrationTest` classes against a real `popcornstash serve` on a `scripts/dev-fixture.sh` stash when `-PpopcornstashServer` names the binary. The test task declares the files outside `android/` it reads (`crates/stash-core/src/library/defs.rs`, `web/src/lib/libraryIcons.ts`, `scripts/dev-fixture.sh` and the server binary) as inputs, and a run against a real server is never taken from Gradle's build cache or skipped as up to date. Building, installing and signing in: `docs/android.md`.

## 11. Behaviour details

Decisions made during implementation that clients rely on.

- **Auth:** `POST /api/auth/login/start` accepts `{conditional?: bool}`; without it the options suit a plain "Sign in" button. The pairing request response adds `interval` (seconds between polls, 6). `PATCH /api/users/:id` also accepts `display_name`. `POST /api/tokens` accepts optional `user_id` (owner only) and `client_kind`. Logging out with a device token revokes that token. Responses carrying secrets send `Cache-Control: no-store`. An approved pairing token nobody collects is revoked 10 minutes after the request expires.
- **Lists:** `GET /api/invites` returns every invite as `{id, role, created_by, created_by_name, created_at, expires_at, used_at, used_by, used_by_name, status: "active"|"used"|"expired"}` (no code or link; only the create response carries those). `GET /api/audit` entries carry `user: {id, display_name|null} | null`. `GET /api/trash` returns `[{item: Item, original_rel_path, trashed_at, trashed_by, restore_refusal: string|null /* the 409 sentence a restore would get now (section 11, Restore from Trash), else null */}]`; restore returns `ItemDetail`; empty returns `{deleted}`.
- **Photo timeline:** `limit` is a photo budget (default 200, at most 500). A busy month can continue on the next page; `count` is always the month's full total.
- **Files:** library files are served with a sandboxed CSP; HTML, SVG, XML and JS are always sent as attachments so a downloaded page cannot run as the signed-in user. Thumbnail URLs with `v=` are immutable; without it they revalidate with ETags.
- **Streams:** session ids are 32 lowercase hex characters. Segment timestamps start at 10 + 6n seconds so segments from restarted ffmpeg runs line up. Videos longer than 72 hours are refused. NVENC/QSV force IDR keyframes; a hardware encoder that fails falls back to libx264 for that encoder until restart.
- **Events:** SSE re-checks the session or token every 60 s and closes the stream once it is gone. `job` events carry `kind` (`scan`, `probe`, `thumb`, `sort`) and `state` (`running`, `done`, `queued` for a retry, `failed`).
- **Library:** rescans refuse to mark anything missing when the stash folder has no files but the database has items (an unmounted share), and mark nothing missing in a library whose folder is missing (section 3.1); they apply `trash_retention_days`. Items marked Ignore in the Inbox are never moved automatically. Media in the owner's own folders carries a suggestion so Organize can be prefilled. Episode items use `title` "Show S01E02" with `episode_title` separate. A show's poster is its first episode in season and episode order that has a thumbnail, else its first episode, counting only that library's episodes; `/api/libraries/:id/shows`, `/shows/:name` and `HomeLibrary.show_posters` all use that choice, so a show has one picture everywhere in a library.
- **Libraries, edge cases:**
  - *Two libraries and one folder:* impossible. `folder` is unique ignoring case, a folder already used by a library is refused (409, `folder.name`), and a symlinked folder is refused, so two names cannot reach one directory.
  - *Nested folders:* only top-level folders can be libraries. A subfolder of a library folder belongs to that library; a subfolder of a folder that is not a library is part of that folder and cannot become a library by itself (the owner moves it to the top level on disk first).
  - *A claim moving to another library:* the claim moves in one transaction (create or edit with that kind). Files sorted before stay where they are. Items waiting in Inbox now suggest the new library to the people who can see it, and stop showing a library to people who cannot (for them they are no longer visible unless they added them). A sort job that started before the change moves no further files: the rest of its entry waits in Inbox with suggestions (section 4, One sort job, one set of claims).
  - *Changing type or structure when not empty:* refused with 409. The owner moves the items out (Organize) or deletes the library and makes a new one from the same folder with the other type: the files stay, the items the new library indexes keep their ids and history, the rest leave the index (section 4), and the folder is indexed under the new rules. Because the folder already exists, the owner chooses again who sees the new library; the editor starts on "Only some people" with nobody chosen, so a library that was private never becomes public by being remade.
  - *Deleting a library with items in Inbox pointing at it:* nothing stored points at a library, so those items simply show no suggested library ("Choose a library") until a person picks one; a person who could see them only through that library no longer sees them. An accept that names the deleted library answers 404 with field `library_id`.
  - *Deleting or hiding a library while someone watches:* their next segment, range or progress request answers 404 and the player says "This video is no longer available." (section 7, While watching).
  - *An episode whose show spans libraries:* each library shows its own part of the show (its seasons and episodes, its poster, its counts). Next and previous episode stay inside the library, so playback never leads into another library, visible or not. Continue watching shows the episode in whichever library it is in.
  - *A file moved on disk between library folders:* matched by size and quick hash as before; it changes library and keeps its id, favourite and progress. If it moved into a library the person cannot see, it disappears for them.
  - *Restore from Trash:* the file goes back to its original path and gets the library (or place) that path has now, indexed under that library's rules: it keeps its category when that still fits the library, otherwise its category and metadata are read again from the path. When the library there now would not show the file (section 4, Files already in a library: a photo where a Video library now is), the restore is refused before anything moves: 409 `conflict` "Kids does not show this kind of file any more, so it stays in the trash." The Trash list says the same beside the item. The file stays in `.popcornstash/Trash/` until the retention ends, and the owner can copy it out on the share. When the library's folder is missing or read-only, restore answers the 409 of section 8 ("This library's folder is missing...").
  - *Loose files at the root and folders not in a library:* full owners only; Organize moves them into a library or Other files.
  - *Member removed or demoted:* their `library_members` rows go with the user (`ON DELETE CASCADE`); a demoted owner sees `selected` libraries only where listed, from their next request and on their open streams at once (section 7, Live events).
  - *A library's folder disappears* (an unplugged disk, a share that did not mount): the folder is not re-created, nothing in it is marked missing, nothing is sorted into it, and new files of its kinds wait in Inbox until it is back (section 3.1). Owners see "Folder missing" in Settings.
  - *A file you add goes to a library you cannot see:* the owners decide where each kind goes, so it is sorted there like any other file and leaves your view; your download stays listed for you as sorted with no destination. PRODUCT.md says so as an honest boundary.
  - *Libraries the person cannot see* never appear in counts, names, icons, suggestions, download destinations or error messages; a request that names one is answered exactly as for an id that never existed.
- **Media:** rotation is stored clockwise. `taken_at` comes from EXIF or QuickTime capture dates (never the encode date of movies and episodes). The thumbnail job also prepares the browser rendition of HEIC/RAW/TIFF photos.
- **Downloads:** a stored `source` drops credentials, credential-like query keys and fragments; when anything private was removed, or the download depended on forwarded browser cookies, it ends in `#redacted` and cannot be fetched again after a restart ("Send the link again"). Magnet sources keep only the hash and name. Queued torrents carry why they wait in `error`: "Waiting for the VPN to connect.", "Blocked: VPN not protecting. The public address is not a VPN address." (the engine is blocked because the public address check failed, the status's `protected: false`; it follows the engine's health on every poll), or "Waiting for a free slot (N at once)." Torrents waiting for the VPN are kept in `<config>/pending-torrents/` (folder 0700, files 0600) across restarts. A torrent's files are taken out of the engine only as one entry directly inside `In Progress` (never deeper, never a `.part` file, whatever the engine reports as its folder). A finished torrent can sit in `completed` while its move to `Completed/` is retried. `DELETE /api/downloads/:id` removes the row (after a final `cancelled` event), answers 409 while a sort is pending, and never deletes files once they are sorted. With `http_via_vpn: when_available`, links on ports other than 80/443 go direct; `always` refuses them. Upload 409/400 bodies carry `received` both inside `error` and at the top level.
- **VPN:** `/api/vpn/nordvpn` defaults `protocol` to `wireguard`. Upload routes accept an optional `filename` for the label; `/api/vpn/openvpn` also accepts JSON `{config, username, password}`. An uploaded `.ovpn` pointing at `*.nordvpn.com` is saved as provider `nordvpn`. The OpenVPN validator mirrors OpenVPN's own line parser and refuses code-running directives, `setenv` outside a small allowlist, and lines OpenVPN would read differently. When the engine does not answer but the supervisor's status file is fresh, the status reports the tunnel's real state with `reachable: false`. A saved VPN that the supervisor has not applied yet (right after a setup, while the engine restarts) reports `connecting`, never `not_configured` with `configured: true` (except with `STASH_ALLOW_NO_VPN`, where it is always `not_configured`). With torrents off (no `STASH_TORRENTD_URL`) the status is `unreachable` with `torrents_off: true` and `error` set to the supervisor's reason (`STASH_TORRENTS_OFF_REASON`) or "Torrents are turned off on this server."; nothing waits for the VPN then, and adding a torrent answers `vpn_unavailable`. NordVPN answering 400, 401 or 403 to the token exchange means the token was not accepted. Settings changes are audit-logged as `settings_changed` (key names only).

- **Engine:** torrent ids are info hashes. A magnet POST answers at once; metadata resolves in the background (state `initializing`, or `error` if it fails) and pending magnets survive restarts. While blocked, the librqbit session is stopped (its sockets are tied to `psw0`); `GET /torrents` then answers from saved state. Speed limits are KiB/s; a nonzero limit below 16 KiB/s counts as 16 KiB/s (librqbit moves 16 KiB blocks and would stall below that), and the `torrent.*_limit_kbps` settings are stored that way. Landlock runs on the engine's own thread and runtime. On kernels without IPv6, sockets fall back to IPv4 (vendored `librqbit-dualstack-sockets` patch). A peer whose connection failed (a seeder that paused or went offline) is reconnected as soon as a tracker, the DHT or PEX lists it again, no sooner than 10 s after it failed, instead of after librqbit's reconnect backoff, which grows to an hour (vendored `librqbit` patch, `vendor/librqbit/PATCHED.md`).
- **Ownership:** the adopt walk only re-owns entries owned by the engine user; `/config` preparation skips files with more than one hard link and refuses symlinked `torrent`, `vpn` or `engine` folders; `vpn/` is forced to 0700.

## 12. Packaging

- Dockerfile: Node stage builds `web/` → Rust stages build `popcornstash` (release, embeds `web/dist`) and `boringtun-cli` → runtime `debian:trixie-slim` with `ffmpeg`, `libvips-tools`, `wireguard-tools`, `openvpn`, `nftables`, `iproute2`, `ca-certificates`, `tini`. `ENTRYPOINT ["tini", "--", "popcornstash"]`, `CMD ["run"]`. Multi-arch amd64/arm64. The Node and Rust stages run on `$BUILDPLATFORM`; when `TARGETARCH` differs from `BUILDARCH` the Rust stages cross-compile (arm64: `aarch64-unknown-linux-gnu` with `gcc-aarch64-linux-gnu`, `libc6-dev-arm64-cross` and multiarch `libssl-dev:arm64`, with the target's `CC`/`CXX`/`AR`, linker and pkg-config path for openssl-sys, ring, aws-lc-sys and the bundled SQLite; amd64 on an arm64 machine the same way round, not yet tested), and each binary's ELF machine is checked. A native build compiles exactly as before. Only the runtime stage runs for the target platform, so an arm64 build on amd64 needs QEMU (binfmt_misc) for its `RUN` steps (docs/deployment.md, "Building the image yourself"). The base registry is a build argument (default Docker Hub; restricted environments use `mirror.gcr.io/library`, and a TLS-intercepting proxy's CA goes in as the `build_ca` secret).
- `compose.yaml`: one service `popcornstash`: ports 8788, `cap_drop: [ALL]` plus `cap_add: [NET_ADMIN, CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID, KILL]` (each verified necessary; NET_RAW is not), `devices: [/dev/net/tun]`, `read_only: true` with tmpfs `/tmp` and `/run` (`/run:exec,mode=755`), `no-new-privileges`, the `net.ipv4.conf.all.rp_filter=2` sysctl (docs/vpn.md), volumes appdata → `/config` and stash → `/stash`, env `PUID/PGID/STASH_PUBLIC_URL` plus the optional settings from `.env` (`STASH_HWACCEL` is empty by default, so Settings decides). The container is named `popcornstash`. The image patches boringtun for IPv4-only hosts, ships the VA drivers for Intel/AMD GPUs, and replaces Debian's open ImageMagick policy with one that allows only the camera RAW coders (section 5, Media). `popcornstash healthcheck` uses HTTPS when `STASH_TLS_CERT` is set. The image carries `LICENSE`, `NOTICE`, `THIRD_PARTY_LICENSES`, `THIRD_PARTY_LICENSES-web` and `debian-packages.txt` (every Debian package with its source package and version, for snapshot.debian.org) in `/usr/share/doc/popcornstash`; `scripts/notices.sh` regenerates the two third-party files.
- `unraid/popcornstash.xml`: one template that names the container `popcornstash`, like compose (`--cap-add=NET_ADMIN --device=/dev/net/tun`, PUID 99 / PGID 100, `STASH_HWACCEL` empty by default).
- No CI service: checks and releases run on a developer machine. GitHub Actions is not used (the repository's Actions are turned off).
  - `scripts/check.sh`: git ignores the usual secret file names and tracks none of them, every manifest says `AGPL-3.0-or-later`, then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, web `npm ci` (only when `node_modules` is missing or older than `package.json` or `package-lock.json`) + `check` (tsc, eslint, the theme check and the vitest unit tests in `web/src/**/*.test.ts`) + `build`, extension the same + `zip`, the Android app (`./gradlew test lint assembleDebug verifyRoborazziDebug checkLucideGlyphs` in `android/`, skipped with a note when no Android SDK is found through `ANDROID_HOME`, `ANDROID_SDK_ROOT` or `android/local.properties`), then `scripts/docker-smoke.sh` (builds `IMAGE`, default `popcornstash:dev`). Stops at the first failure and prints PASS/FAIL/SKIP per step. `--quick` skips Docker, `--e2e` adds `tests/e2e` and the Android app against a real `popcornstash serve` (`--tests '*ServerIntegrationTest*'` with `-PpopcornstashServer`, always run again, never taken from Gradle's cache; when an Android SDK is found), `--full` adds what `--e2e` adds, `scripts/vpn-e2e.sh` and `scripts/fullstack-e2e.sh` (on the same image), `--audit` adds `cargo audit` and `npm audit --omit=dev` for web and extension. `IMAGE`, `SKIP_BUILD`, `REGISTRY`, `DOCKER_HUB_MIRROR` and `BUILD_CA` pass through to the Docker scripts. `.githooks/pre-push` (opt-in, `git config core.hooksPath .githooks`) runs it before a push to `main`.
  - `scripts/release.sh <version>`: preflight, all before any build (semver with each number ≤ 65535; on `main`, clean, equal to `origin/main`; tag `v<version>` new here and on `origin`; a `## [<version>] - <today>` section in `CHANGELOG.md`; Cargo workspace and `Cargo.lock`, the `package.json`/`package-lock.json` of web, extension, tests/e2e and tests/fullstack at `<version>`, and `extension/static/manifest.json` at `<version>` without its pre-release; a build for the non-native architecture runs (QEMU); `GHCR_TOKEN` is a classic token with `write:packages` that ghcr.io accepts; the package is public or does not exist yet and has no `<version>`; `gh` can push to the repository and has no `v<version>` release) → `scripts/check.sh --quick --e2e --audit` (the working tree must be unchanged afterwards) → `scripts/notices.sh --check` (the committed third-party notices match `Cargo.lock`, boringtun-cli and `web/package-lock.json`) → `linux/amd64` and `linux/arm64` images built from `git archive` of the commit with `--build-arg POPCORNSTASH_RELEASE=<version>`, labelled with `org.opencontainers.image.version`/`revision`/`created`, both reporting `popcornstash <version>` and each carrying the committed notice files and a Debian package list in `/usr/share/doc/popcornstash` → `docker-smoke.sh` (with `EXPECT_SOURCE=https://github.com/thekozugroup/PopcornStash/tree/v<version>`), `vpn-e2e.sh` and `fullstack-e2e.sh` on the native image (`SKIP_BUILD=1`), then `docker-smoke.sh` on the other architecture's image with `PLATFORM=linux/<arch>` under QEMU (it skips only the VPN and engine checks and checks that torrents stay off) → the extension zip from `npm ci` + `npm run zip` in the same copy → local annotated tag `v<version>` (`--cleanup=verbatim`, the `CHANGELOG.md` section) → push `ghcr.io/thekozugroup/popcornstash:<version>-<arch>`, `docker buildx imagetools create` → `<version>`, plus `<major>.<minor>` and `latest` when `<version>` is the highest full release of its line and overall (a pre-release gets `<version>` only) → push the tag → `gh release create` with the notes, the zip and `--prerelease` or `--latest` → an anonymous `imagetools inspect` (fails the run if the package is not public). The token goes to `docker login` in a temporary `DOCKER_CONFIG` deleted at exit, and `GHCR_TOKEN`/`GH_TOKEN` are removed from the environment of everything else. A failure before the images are joined deletes the local tag; a later one prints the commands that finish the release. `--dry-run` does everything except push, tag and release (a dirty tree is built as it is); `--bump <version>` sets the version numbers and stops. The extension's `manifest.json` `version` is always `MAJOR.MINOR.PATCH` (Chrome accepts only numbers); `extension/scripts/build.mjs` puts a pre-release in `version_name`.
- `cargo audit` reads `.cargo/audit.toml`, which lists the reviewed advisories it may skip and why.

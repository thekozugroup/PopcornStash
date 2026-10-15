# PopcornStash: plan for the next round

This plan is for the developer who takes PopcornStash from here, working locally with SSH access to the owner's server. It records nine decisions the owner made on 2026-10-14, says what the code does today with file:line references, and gives the design, tests, docs and live checks for each item, in priority order. It starts with what has never run on real hardware, because the owner's server, phone and browser are the first real test of most of the product. The research behind it ran on 2026-10-14, and the references were checked against main at `cb41634`, before the right-edge bar, the Android updater and PR #1 (magnet pause and resume) landed (main is now `aebd256`); line numbers drift, so check each one before relying on it.

Abbreviation: `<app>` = `android/app/src/main/kotlin/com/thekozugroup/popcornstash`.

## How to work here

Read `AGENTS.md`, `PRODUCT.md` and `docs/architecture.md` first. What matters most:

- **The live server.** The owner's NAS runs PopcornStash from a git clone with `docker compose`. The owner gives you the SSH host and the clone path. Deploy a change with the steps in docs/deployment.md, "Updating a clone of the repository" (lines 329-341):
  ```sh
  cd /path/to/PopcornStash
  docker compose stop            # then back up app data (docs/deployment.md, Backups)
  git pull --ff-only
  docker compose up -d --build
  docker compose logs --tail 50 popcornstash
  ```
  Always back up `/config` before deploying a change that adds a database migration (item 6 adds one, item 3 may). Keep the clone clean so `git pull --ff-only` works: local settings go in `.env` and `compose.override.yaml`, never in tracked files.
- **main is the source of truth.** The NAS deploys from main, so nothing reaches main unchecked. Run `scripts/check.sh` before every push (fmt, clippy, tests, web, extension, Android, Docker smoke; `--quick` skips Docker, `--e2e` adds the Android app against a real server). There is no CI service; do not add GitHub Actions workflows.
- **Commits:** as `thekozugroup <thekozugroup@gmail.com>`, no AI co-author tags, only the intended files. No credentials, VPN files, tokens, signing keys or personal media in commits. Screenshots of the owner's library stay in the git-ignored `evidence/`.
- **Contract:** `docs/architecture.md` is the API and data contract. Change it in the same commit as the behaviour. The library icon list is one contract for server, web and Android.
- **Languages:** Rust for processing and backend, TypeScript only for the web UI and the extension, Kotlin only for Android.
- **Safety rules:** never route torrent traffic outside the VPN tunnel. Never weaken the passkey requirement. Never move, rename or delete files outside `Inbox/` and `Downloads/` without an explicit user action.
- **Design:** use the Taste (`.claude/skills/design-taste-frontend`) and Impeccable (`.claude/skills/impeccable`) skills for every design change and review. Build web UI from Astryx (`npm --prefix web run astryx -- component <Name> --props`); Android follows android/DESIGN.md. Record Roborazzi screenshots and look at each.
- **Evidence:** record scope and evidence in `working.md`, including what was checked on the live server and what was not.
- **Testing on the owner's server without risk:**
  - Make a scratch library on a new folder (for example "Test", folder `Test/`) and use your own test files for every delete, trash and history test. Never trash, delete or move the owner's media while testing.
  - Use legal torrents only (a Linux ISO, Blender's open movies).
  - Change `.env` only with the owner's OK. `STASH_LOG=debug` is fine for a session; set it back to `info` afterwards. Environment changes need `docker compose up -d` (a recreate); `docker compose restart` keeps the old environment.
  - Do not make the server or any path of it public (Tailscale Funnel, a tunnel, port forwarding) without the owner. That is the owner's choice (item 2).

## Never verified on real hardware: verify first

Do this pass before new feature work, and record each result in working.md. Items 8 and 9 are mostly this kind of check.

**Android app on the owner's phone** (docs/android.md:99 says it has never run on a phone or emulator):
- [ ] Pairing with a code (the owner reports it works; record the phone model and Android version).
- [ ] Playback: direct play and HLS, seeking, rotation, audio focus, picture-in-picture.
- [ ] Downloads tab: live progress, pause, resume, remove, VPN line.
- [ ] Share sheet uploads: photos and videos from the gallery, a link, background upload with the notification, Stop, and resume after the app is killed.
- [ ] The vertical navigation bar on the right edge (item 9).
- [ ] Frosted glass tiers: blurred on Android 12 and later, opaque on Android 9 to 11 and with the contrast setting at medium or high. Android 17 QPR2's platform blur has never drawn on any device (docs/android.md, What is not done yet).
- [ ] The in-app updater, once a real release exists (item 8).
- [ ] Passkey sign-in, after item 2.

**VPN and torrents** (docs/vpn.md:97-105, "Not yet tested with real services or hardware"):
- [ ] NordVPN token exchange and a real NordLynx (WireGuard) connection; OpenVPN if the owner uses it.
- [ ] The public address on the VPN page is NordVPN's, and NordVPN says the connection is protected.
- [ ] Kill switch: stop the tunnel and confirm the engine has no route and torrents stop (docs/vpn.md, "Checking that it works").
- [ ] A whole torrent through the tunnel. A torrent IP-check magnet (for example the one ipleak.net offers) must report the VPN address only.
- [ ] Web downloads with "Web downloads use the VPN" set to When available and Always.

**First real GPU encode:** VAAPI on the Ryzen iGPU (item 7). Hardware encoding has never run on a real GPU (docs/deployment.md:278, CHANGELOG.md:140).

**Passkeys in browsers on the owner's real domain:** sign-in, adding a second passkey, an invite for a family member, and the recovery path (`docker exec popcornstash popcornstash admin reset-owner`) explained to the owner before anyone needs it. Do not test recovery in a way that locks the owner out.

**The Chrome extension against real sites:** pairing, the popup's Send, right-click "Send video", the opt-in download takeover with and without cookies, and the plain message for `blob:` videos (extension/src/lib/links.ts:32-37).

## Order and shared code

- Priority is the order of the sections below.
- Items 4, 5 and 6 change the same code (`downloads/manager.rs`, `library/organizer.rs`, settings, the Downloads page, the Android Downloads tab). Do them one after another, not in parallel worktrees.
- Migrations: item 6 adds `library_id` columns, and item 3 may add a download kind. `downloads.kind` has a CHECK constraint (crates/stash-core/migrations/0001_init.sql:148), so a new kind means rebuilding that table. Number the migrations in the order they merge (`0004_…`, `0005_…`) and test each against a copy of the owner's database before deploying.
- Item 3's browser fallback and item 6 both add fields to the upload API. Agree on the `POST /api/uploads` body once.

## 1. Torrents: download, seed at most a little, stop at completion

### Decision (2026-10-14)
The owner wants to leech, seed at most a tiny bit, and stop the moment a download completes. Seeding after completion is already off by default. The owner was advised to set Upload limit to 16 KB/s. Plan the fixes the research found: the failed-completion path that leaves a torrent seeding, stopping the idle engine session, a per-torrent peer limit, the ratio help text, clamping `seed_hours`, and tests. A settings migration only if it is worth it.

### Current behaviour
- Defaults: seeding off, ratio 1.0, 24 hours, 5 at once, no speed limits (crates/stash-core/src/settings.rs:35-45; contract docs/architecture.md:400).
- With seeding off, a finished torrent is removed from the engine on the next poll:
  - The poller treats Seeding or done ≥ total as complete (crates/stash-server/src/downloads/torrents.rs:774-775), reads the settings fresh (:792) and calls `finish()` (:796-797).
  - `finish_completed()` sends `DELETE /torrents/:id?delete_files=false` first (torrents.rs:956-963), then the supervisor adopts the folder (:993), it moves to `Completed/` (:997) and the sort is queued (:1000-1004).
  - The window is 2 s after the previous poll's work, not 2 s flat: a poll runs everything inline (torrents.rs:387-392, 483-525), a move across filesystems copies (crates/stash-core/src/paths.rs:386-393), and each engine call may take its 5 s timeout (crates/stash-server/src/torrentd_client.rs:24, 121). At 16 KB/s that is still only tens of KiB.
- **Bug:** when `finish()` cannot find the files in `In Progress` it marks the row failed and returns without a DELETE (torrents.rs:906-911). `manager.fail()` only updates the database (manager.rs:986-1004), and failed rows are not polled (torrents.rs:398-400). The torrent then seeds until someone removes the download.
- Uploading during a download cannot be turned off:
  - librqbit is built without `disable-upload` (crates/stash-torrentd/Cargo.toml:29).
  - It unchokes every peer (vendor/librqbit/src/peer_connection.rs:334-341).
  - A finished torrent that is still live keeps finding new peers and announcing (vendor/librqbit/src/torrent_state/live/mod.rs:612-616).
  - The limit is one session-wide cap (crates/stash-torrentd/src/engine.rs:730-733, 1346-1358). 0 means unlimited, and the lowest real cap is 16 KiB/s (crates/stash-core/src/internal_api.rs:189-192; engine.rs:402-409; web/src/pages/settings/vpn/TorrentSettings.tsx:63-64, 85). At 16 KiB/s that is about 1.3 GiB a day at most.
- No peer limit is set (engine.rs:883-888, 1191-1194), so each torrent may hold 128 connections (vendor/librqbit/src/torrent_state/live/mod.rs:286-288). `SessionOptions.peer_limit` (vendor/librqbit/src/session.rs:468) is applied to every torrent that does not set its own (session.rs:1357).
- The engine session runs whenever the tunnel is usable, with or without torrents (engine.rs:608-689). It keeps a DHT node on UDP 6882 and a TCP/uTP listener on 6881 (engine.rs:709-727).
- The web form says the ratio is "Uploaded divided by downloaded." (TorrentSettings.tsx:168). The code divides by the torrent's total size (torrents.rs:793-794).
- `normalized()` clamps the ratio but not `seed_hours` (settings.rs:93-111), so the API accepts 0 or about 4 billion hours. The web form allows 1-720 (TorrentSettings.tsx:71-77).
- `docs/vpn.md` and `PRODUCT.md` never mention seeding or the upload limit.

### Design
1. **Failed completion removes the torrent.** In `finish()`'s missing-files branch (torrents.rs:906-911), send `DELETE /torrents/:id?delete_files=false` before `manager.fail()`, treating 404 as done. If the DELETE fails, pause the torrent and retry the removal on later polls, the way `finish_pending` retries (torrents.rs:426-452). A failed torrent must never stay live in the engine.
2. **Stop at the moment of completion (recommended; matches "stop the moment").**
   - Add `seed: bool` to `LimitsRequest` (internal_api.rs:196-200) and push it from `seed_after_complete` in `push_limits` (torrents.rs:537-552).
   - When `seed` is false, the engine awaits `ManagedTorrent::wait_until_completed` (vendor/librqbit/src/torrent_state/mod.rs:591) for each torrent it adds or restores, and pauses the torrent as soon as it completes.
   - The poller already treats done ≥ total as complete, so a paused finished torrent is finished as before.
   - Contract: `PUT /limits` in docs/architecture.md section 9.
3. **Stop the idle session.**
   - In `Engine::supervise` (engine.rs:608-689), stop the librqbit session when it holds no torrents and no pending magnets. Start it again in `Engine::add` (engine.rs:1164), and at engine start when `session.json` holds torrents.
   - `GET /torrents` answers `[]` while the session is stopped.
   - If `/health` gains an `idle` engine state, update section 9 and make sure the VPN page does not show idle as a problem.
   - Cost: the first magnet after an idle period waits for the DHT. `dht.json` persists, so the wait is short. The tunnel itself stays up; that is not part of this item.
4. **Per-torrent peer limit.** Set `peer_limit: Some(N)` in `session_options` (engine.rs:702-735). Start with N = 50, a constant in the engine. It applies to torrents that go live after the change.
5. **Ratio help text:** "Uploaded divided by the torrent's size." (TorrentSettings.tsx:168).
6. **Clamp** `seed_hours` to 1..720 in `normalized()` (settings.rs:93-111).
7. **No settings migration.** Settings are saved as one JSON row with every key (settings.rs:180-196). A stored `up_limit_kbps: 0` cannot be told apart from an owner who chose "no limit", so a migration would override real choices. Set 16 KB/s by hand on the owner's server. Whether new installs should default to 16 is an open question.

### Tests
- `seed_hours` reached: `completed_at` older than `seed_hours` finishes the torrent (in the style of `seeding_policy_is_respected`, crates/stash-server/src/downloads/tests.rs:1472).
- Turning `seed_after_complete` off while torrents seed finishes them on the next poll, paused ones included.
- `seed_after_complete` defaults to false (next to the `seed_ratio` assertion at crates/stash-server/src/api/settings.rs:183).
- `seed_hours` is clamped (with `partial_update_merges`, settings.rs:237).
- Missing files at completion: the fake engine records the DELETE (fake engine helpers at tests.rs:540-559).
- Engine: with `seed: false`, a torrent is paused when it completes. Use two loopback engines with `--allow-no-vpn`, as the reconnect regression test does.
- Engine: the session stops with no torrents, starts on add, and the listing works while it is stopped.
- Engine: `session_options()` sets the peer limit.
- Full stack: after "In Movies", the engine no longer lists the torrent (tests/fullstack/specs/fullstack.spec.ts, after :179).
- Existing tests that must keep passing: tests.rs:1404, :1472, :2372, :2419; engine.rs:1535 and :1563.

### Docs
- docs/architecture.md:296 (finish path, pause at completion, failed completion), section 9 (`PUT /limits`, `/health` idle).
- docs/architecture.md:400 only if a default changes.
- docs/vpn.md, a short paragraph near line 72: seeding is off by default; a finished torrent stops at once; it uploads while it downloads; the Upload limit goes down to 16 KB/s.

### Live validation
- Settings > VPN > Torrents shows seeding off and Upload limit 16 KB/s.
- Add a legal torrent. During the download the row's upload speed stays at or under about 16 KB/s.
- At completion, `docker compose logs popcornstash | grep -E 'torrent (added|finished|removed)'` shows "torrent removed" right after the finish, and the download ends sorted.
- With no torrents left, the log shows "torrent session stopped" (engine.rs:774), and `docker exec popcornstash ss -uanp` no longer lists UDP 6882. Adding a magnet logs "torrent session started" (engine.rs:746).

## 2. Android passkeys

### Decision (2026-10-14)
The likely cause is that Google cannot fetch `https://<rpId>/.well-known/assetlinks.json`, because the server is on a private Tailscale or home-only address. Separately, our docs omit `delegate_permission/common.handle_all_urls`. Pairing with a code works. Plan: fix the docs; give the app one error message per cause and a log tag; build a better pre-check from `rp_id` and the app's own fingerprint; add server startup warnings. Whether to make the file public (Tailscale Funnel on 443 for that one path with the app on :8443, or another public route) is the owner's choice, made on the live server.

### Current behaviour
- The owner's sentence, "The passkey did not work. Try again, or pair with a code.", comes only from the phone side. Any `GetCredentialException` that is not a cancel or `NoCredentialException` maps to it, and the exception is discarded (`<app>/data/auth/CredentialManagerAuthenticator.kt:17-27`). Nothing is logged anywhere in the app.
- So the server never received `login/finish`. A server rejection shows its own sentence (crates/stash-server/src/auth/passkeys.rs:351-357).
- The app's pre-check (`<app>/feature/signin/SignInViewModel.kt:96`; `<app>/data/api/ApiClient.kt:46-53, 345-355`) fetches assetlinks from the typed address, from the phone (inside the tailnet). It never checks:
  - whether the typed host is the `rpId` (`ApiModels.kt:41` reads `rpId` but nothing uses it);
  - the fingerprint;
  - `handle_all_urls`;
  - whether Google can reach the file.
- The `rpId` is the host of `STASH_PUBLIC_URL` (crates/stash-core/src/config.rs:254-260). The file is served from `STASH_ANDROID_ASSETLINKS` as is, with `Cache-Control: public, max-age=3600` (crates/stash-server/src/web.rs:28, 89-101).
- Server parsing (`android_origins`, passkeys.rs:134-179):
  - It requires `get_login_creds`.
  - It skips a fingerprint that is not 32 hex bytes, silently (:161-169).
  - It logs nothing about the origins it derives.
- Docs:
  - docs/android.md:81 says "no separator at all" is fine. Google wants uppercase, colon-separated hex.
  - docs/android.md:86 gives only `get_login_creds`.
  - docs/android.md:95 says a tailnet-only address "may not" work. It cannot work.
  - docs/deployment.md:21-32 recommends Tailscale Serve and says the name need not be public, without this caveat.
  - docs/deployment.md:367 and .env.example:40 should point to the full statement.
- Google's API answers a `*.ts.net` name without Funnel with `ERROR_CODE_FETCH_ERROR` "DNS name not found", and caches that for about 10 minutes.

### Design
1. **Docs.**
   - The statement gives both relations and an uppercase, colon-separated fingerprint:
     `STASH_ANDROID_ASSETLINKS='[{"relation":["delegate_permission/common.handle_all_urls","delegate_permission/common.get_login_creds"],"target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash","sha256_cert_fingerprints":["<FINGERPRINT>"]}}]'`
   - Drop "no separator at all" (docs/android.md:81).
   - Say plainly that a private address (Tailscale without Funnel, home-only DNS, an internal CA) cannot work for app passkeys, and that pairing is the way for a private server (docs/android.md:95; a warning in docs/deployment.md:21-32).
   - Add Google's check as the verification step:
     `https://digitalassetlinks.googleapis.com/v1/assetlinks:check?source.web.site=https://<rp_id>&relation=delegate_permission/common.handle_all_urls&target.android_app.package_name=com.thekozugroup.popcornstash&target.android_app.certificate.sha256_fingerprint=<FINGERPRINT>`. Repeat it with `get_login_creds`. Both must say `"linked": true`.
   - Describe both public options neutrally, with their trade-offs:
     - (a) Funnel on 443 for only `/.well-known/assetlinks.json`, with the app on `tailscale serve --https=8443` and `STASH_PUBLIC_URL=https://<name>.ts.net:8443`. The rpId stays the host, so existing passkeys keep working, but bookmarks, the extension and the app move to :8443. This has never been tried.
     - (b) An own domain or public route. That changes the rpId, so the owner registers a new passkey through `admin reset-owner`.
2. **App, one message per cause, plus a log tag.** In `CredentialManagerAuthenticator.kt:17-27`:
   - Before mapping, log `e.type`, `e.errorMessage` and, for `GetPublicKeyCredentialDomException`, the DOM error type with `Log.w` under the tag `PopcornStash.Passkey`. These values hold no secrets.
   - SecurityError: "Android could not confirm this app belongs to <rpId>. Google must be able to open https://<rpId>/.well-known/assetlinks.json from the internet. Pair with a code instead."
   - NotAllowedError: "The passkey was not used. Unlock with your screen lock and try again, or pair with a code."
   - Provider configuration or unsupported: "This phone's passkey service is not set up. Update Google Play services or turn on a passkey service in Settings."
   - Interrupted: "Sign-in was interrupted. Try again."
   - Anything else: today's sentence, with the error type in small text under it.
   - Write the final wording with the ux-copy rules (short, plain, one action).
3. **App pre-check** (`SignInViewModel.kt:91-96`, `ApiClient.kt:46-53, 345-355`):
   - Read `rp_id` from `/api/auth/state`. When it differs from the typed host, say so.
   - Fetch assetlinks from `https://<rp_id>/`, not from the typed address.
   - Require both relations.
   - Compare the app's own signing certificate SHA-256 (from `PackageManager` signing info) with `sha256_cert_fingerprints`.
   - When a check fails, show why under "Pair with a code" instead of a passkey button that cannot work.
   - The phone cannot prove that Google can reach the file. Optionally, after a SecurityError only, call Google's `assetlinks:check` to name the exact cause. This sends the host name to Google, which Android already does at sign-in. Mention it in docs/android.md.
4. **Server startup warnings** in `android_origins` (passkeys.rs:134-179). Return the problems with the origins, so tests can assert them without capturing logs.
   - WARN for each skipped fingerprint.
   - WARN for a statement without `handle_all_urls`.
   - WARN for a fingerprint that is not uppercase with colons.
   - INFO with the derived `android:apk-key-hash:` origins (public values).
   - WARN when `STASH_ANDROID_ASSETLINKS` is set and the `rp_id` looks private (`*.ts.net`, `.lan`, `.local`, `.home.arpa`, a single label): "the Android app's passkeys need https://<rp_id>/.well-known/assetlinks.json to be reachable by Google; this name is probably private. Pairing still works."
5. **Server: sign-ins that start but never finish.**
   - Log `login/start` at INFO with whether it came from the native app (no Origin or Referer; crates/stash-server/src/security.rs:231-253).
   - When a native flow expires unused (passkeys.rs:24, 232-240), log that Android probably refused it on the phone.
   - Optional, a contract change: `POST /api/auth/login/abort {flow_id, reason}` so the owner sees "Sign-in failed on the phone: SecurityError" in the audit log. Do it only if the log line is not enough.
6. The passkey requirement itself does not change. Nothing here relaxes WebAuthn checks.

### Tests
- Kotlin: the exception-to-message mapping for each DOM error and exception type. The log line is written with the type and no secrets.
- Kotlin: the pre-check fails for an `rp_id` different from the typed host, a missing `handle_all_urls`, and a fingerprint that is not the app's own.
- Rust: `android_origins` reports a skipped fingerprint, a missing relation, a lowercase or colon-less fingerprint, and returns the right origin for `DB:19:CF:…:9A:C0` (`android:apk-key-hash:2xnPX3XbzSy-ERFeZ1G1vLjdGf738pyJ42O-2Z6HmsA`). The existing vector is at passkeys.rs:491-514.
- Rust: private-name detection for each pattern, and no warning for a public name.

### Docs
docs/android.md:81, 86, 95; docs/deployment.md:21-32, 367; the `.env.example` comment above line 40; docs/architecture.md:472, 769, 806 if the served file or the app's checks are described there; working.md.

### Live validation
1. On the NAS:
   ```sh
   curl -s https://<typed host>/api/auth/state        # note rp_id and public_url
   curl -si https://<rp_id>/.well-known/assetlinks.json
   docker exec popcornstash env | grep -E '^STASH_(PUBLIC_URL|ANDROID_ASSETLINKS)='
   ```
2. From any machine: `https://digitalassetlinks.googleapis.com/v1/statements:list?source.web.site=https://<rp_id>&relation=delegate_permission/common.get_login_creds`. A fetch error confirms the cause.
3. With the owner's OK, put the two-relation statement in `.env` with the installed APK's fingerprint, then `docker compose up -d`. Confirm the startup log shows no new warnings except the private-name one.
4. Show the owner the two public options and pairing. Apply what the owner chooses. Then wait about 10 minutes (Google caches the error) and check that `assetlinks:check` says `"linked": true` for both relations.
5. On the phone with the new app build: tap "Sign in with a passkey" and read `adb logcat -s PopcornStash.Passkey`. On success, the audit log shows the sign-in.
6. If the owner keeps the server private: the app explains why passkeys are off, and pairing works.

## 3. Chrome extension media grabber

### Decision (2026-10-14)
"Rust grabber + browser fallback":
- The extension finds the media files and HLS/DASH manifests a page loads: an optional `webRequest` permission, plus an `activeTab` scan of the page.
- It lists them in the popup, and the person sends one.
- The server downloads it in Rust:
  - manifest parsing;
  - segments with resume;
  - AES-128 HLS;
  - every request through the SSRF guard and `choose_proxy`;
  - ffmpeg remuxing from local files only;
  - DRM detected and declined.
- When the server is refused (403, IP-bound or signed URLs), the browser downloads the file in an offscreen document with the person's own cookies and uploads it through the resumable upload API. That needs a narrower uploads scope for the extension token, which is a contract change.
- Not yt-dlp. Not MSE capture.

### Current behaviour
- Manifest V3, with no content scripts (docs/security.md:96). Always granted: `contextMenus`, `storage`, `notifications`. Optional: `downloads`, `cookies`, and host access up to `<all_urls>` (extension/static/manifest.json:30-32). It does not declare `webRequest`, `activeTab`, `scripting` or `offscreen`.
- It only sees what the person right-clicks, and real browser downloads. A `blob:` source is refused with a message (extension/src/lib/links.ts:32-37).
- The server saves an `.m3u8`, `.mpd` or HTML page URL as one small file. That file is classified as other and waits in Inbox (crates/stash-core/src/library/classify.rs:14-41; crates/stash-server/src/downloads/source.rs:151-177). Meanwhile the extension says "Sent to PopcornStash" (extension/src/background.ts:65-71).
- Web downloads:
  - reqwest client: crates/stash-server/src/downloads/http.rs:193-226.
  - Resume with Range: http.rs:674-756.
  - Memory-only secrets fail after a restart: http.rs:554-558; manager.rs:416-419.
  - Cookies are dropped on a cross-origin redirect: http.rs:228-242, 687-700.
  - AddressGuard: crates/stash-server/src/downloads/ssrf.rs:52-154, with its resolver at :212-235.
  - `choose_proxy`: crates/stash-server/src/downloads/manager.rs:865-895.
- ffmpeg reads only `file:<path>` (crates/stash-core/src/media/tools.rs:37-43; docs/security.md:60). Job kinds are Scan, Probe, Thumb and Sort (crates/stash-core/src/jobs.rs:24-33).
- The extension token has `downloads:create` and `downloads:read` (crates/stash-server/src/auth/pairing.rs:38-44). Uploads need `full` (crates/stash-server/src/api/uploads.rs:36). docs/security.md:43 promises the extension "can only add and list downloads".

### Design
**Extension (TypeScript, plain DOM as today):**
- A switch, "Find videos on pages", in Options requests `webRequest` plus all-sites host access, inside the click as Chrome requires. `activeTab`, `scripting` and `offscreen` are optional where Chrome allows that; otherwise they are required only if Chrome shows no warning for them. The docs/extension.md permission table says why each is there.
- The service worker registers a top-level `webRequest.onHeadersReceived` listener for types media, xmlhttprequest and other. It matches by URL (`.m3u8 .mpd .mp4 .webm .m4a .mp3`) or Content-Type (`application/vnd.apple.mpegurl`, `application/x-mpegURL`, `application/dash+xml`, `video/*`, `audio/*`). Segment requests (`.ts`, `.m4s`) count only toward the manifest they belong to. Finds per tab go in `chrome.storage.session`, and are cleared on navigation and when the tab closes.
- Opening the popup with the switch on runs a one-shot, read-only scan of the active tab through `scripting.executeScript`: `video`/`audio` `currentSrc` and `src`, `<source src>`, media links, `og:video`, JSON-LD `contentUrl`. It flags `blob:` sources and `mediaKeys !== null` (copy protection).
- The popup lists the finds in plain words ("Video stream, 1080p", "audio.m4a, 4 MB"), with one action per row. A copy-protected page shows "This video is copy-protected, so PopcornStash can't save it." Design it with Taste and Impeccable.
- URLs found on a page come from a stranger: accept only http(s), never PopcornStash's own hosts, and send only what the person picked. The service worker still answers only the extension's own pages (background.ts:89-94).
- Headers sent are as today: Referer, User-Agent, and cookies only with the cookie option on, only for the link's own host.

**Server (Rust):**
- `POST /api/downloads` accepts an optional `kind: "file"|"hls"|"dash"`. The server also checks the Content-Type of the first answer, so a manifest sent as a plain link is handled too.
- A new stream downloader next to `downloads/http.rs`:
  - Parse HLS with `m3u8-rs`. For DASH, use a parser only (`dash-mpd`'s own fetching and muxing must not be used) or a small parser of our own.
  - Pick the best video variant and its audio rendition.
  - Every manifest, key and segment request goes through the existing client, AddressGuard and `choose_proxy`, so "Always use the VPN" still means always.
  - Cookies follow the existing rule: only to the origin they were sent for, never across a redirect. CDN segment hosts get Referer and User-Agent only. If that fails, the browser fallback covers it.
- Resume by segment:
  - Segments go into a private folder (0700, owned by PUID) under `.popcornstash/`, with an index of the segments already done.
  - A restart continues unless the download needs memory-only secrets, which follows the existing rule (http.rs:554-558).
  - Each segment retries like a web download (http.rs:288-300, 635-657).
- AES-128: decrypt in Rust (the `aes` and `cbc` crates), or hand ffmpeg the local files plus a local key file.
- Decline:
  - HLS: METHOD `SAMPLE-AES`, KEYFORMAT FairPlay/PlayReady/Widevine, `skd://` keys, `#EXT-X-FAXS-CM`.
  - DASH: any `ContentProtection`.
  - Live streams: HLS without `ENDLIST`, a dynamic MPD.
  - Each with a plain sentence. Never wire up any decryption key input.
- Limits: caps on segment count, duration and total size, each with its own sentence.
- Remux: one ffmpeg run on `file:` inputs only, `-c copy` (plus `-bsf:a aac_adtstoasc` for ADTS audio from TS), into one `.mp4` in `Downloads/In Progress`. Then the usual `Completed` and sort path.
- The download kind: either a new `stream` kind (a table rebuild migration, because of the CHECK at 0001_init.sql:148; web, Android and extension types follow), or `http` with a format field. Choose one and document it.
- When the site refuses the server (401, 403, 410, or a signed URL that expired or is IP-bound), fail with a machine-readable `error_code` such as `site_refused`, so the extension can offer the fallback.

**Browser fallback:**
- An offscreen document fetches the file with the browser's own cookies and IP, and streams it to `POST /api/uploads`, `PUT /api/uploads/:id?offset=` (16 MiB chunks, resume from `GET /api/uploads/:id`) and `/complete`. It never buffers a whole file in memory.
- First version: progressive files and single-rendition HLS. The offscreen document decrypts AES-128 with WebCrypto, joins the TS segments (or the fMP4 init segment plus fragments) into one upload, and the server remuxes that one local file. Separate audio renditions and DASH in the fallback come later.
- New scope `uploads:create`:
  - It allows creating, writing, reading, completing and cancelling the caller's own uploads, and nothing else.
  - `scopes_for("chrome-extension")` adds it (pairing.rs:38-44).
  - Tokens paired earlier lack it. The extension then says "Pair again to let this browser send files".
- The fallback traffic leaves through the laptop's own connection, not the VPN. AGENTS.md's VPN rule covers torrents, so this is allowed, but the popup must say it. Whether it starts by itself or after a click is an open question.

### Tests
- Rust, against a local axum server like the existing web-download tests (crates/stash-server/src/downloads/tests.rs:756-1375):
  - TS and fMP4 HLS, with a master playlist and a separate audio rendition;
  - AES-128;
  - a DASH VOD manifest;
  - resume mid-way and after a restart;
  - a segment or key URL pointing at a private address is refused;
  - a redirect to a private address is refused;
  - "Always use the VPN" with the VPN down fails;
  - cookies are never sent to another origin;
  - each DRM marker and each live-stream case is declined with its sentence;
  - the size caps;
  - the remux produces a playable `.mp4` (ffprobe), and ffmpeg's argv contains only `file:` inputs.
- Rust: `uploads:create` can do exactly the upload routes on its own uploads, and is refused everywhere else.
- Extension unit tests (vitest): URL and Content-Type matching, grouping segments under their manifest, parsing the DOM scan, the DRM flags, filtering untrusted URLs, the popup wording.
- Extension e2e (extension/test/e2e/extension.spec.ts with the mock server, test/e2e/mock-server.ts:211-436):
  - a page that loads an HLS stream;
  - sending it from the popup;
  - a file the mock server refuses without the browser's cookie, to exercise the fallback and the upload;
  - an old token without `uploads:create`;
  - light and dark, the axe audit, 44px targets.

### Docs
- docs/architecture.md: section 5 (Web downloads), section 7 (scopes), section 8 (`POST /api/downloads` body, the upload routes' scope, the Download kind), sections 10 and 11.
- docs/extension.md: the permission table, what it finds, what it cannot do (DRM, players that build the video inside the page, live streams, sites that need sign-in on several hosts).
- docs/security.md:43 and :96 (the one-shot scan, untrusted URLs), plus the threat model.
- PRODUCT.md, Journey 4 and Honest boundaries (DRM declined; the fallback uses this computer's connection).
- The third-party notices for new crates (`scripts/notices.sh`).

### Live validation
- With the new extension paired to the owner's server, test against:
  - open HLS and DASH test streams (Apple's sample streams, DASH-IF test vectors, Blender's open movies);
  - one AES-128 stream;
  - a site with signed or cookie-bound URLs, to see the fallback;
  - a DRM demo asset, to see it declined.
- Check that each file arrives sorted and plays, that the log shows no private addresses, and that segments went through the VPN proxy when the setting says so.

## 4. File management: Trash, delete permanently, selection

Confirmed 2026-10-15: restoring Trash stays owner-only. Android keeps the existing web Trash this round; do not build a native Trash screen.

### Decision (2026-10-14)
- **Who may trash:** members may move to the Trash only their own private files: their Other files and their own Inbox files. Owners may trash anything.
- **Move to Trash stays the default everywhere.** Add a separate "Delete permanently…" on the item page, in selections and in the Inbox, with a stronger confirm that names the files and their size.
- **In the Trash:** "Delete now" on each item, and deleting a selection.
- **Deleting a movie, show or season:** delete the listed files with their sidecars, then remove the folders left completely empty, never the library folder. Other files in the folder stay, and the confirm names them.
- **Selection mode** on web and Android. **Delete on Android**, which has none today.

### Current behaviour
- `DELETE /api/items/:id` only moves an item to the Trash, and only for a full owner (crates/stash-server/src/api/items.rs:349-368; `require_owner` at :355). It writes `item_trashed` with the title and path (:358-366).
- `trash_item` (crates/stash-core/src/library/organizer.rs:1726-1764) moves the file and its sidecars into `.popcornstash/Trash/<id>/` under the move lock. On a separate mount the move is a copy (crates/stash-core/src/paths.rs:376-395). It removes emptied folders only in `Inbox/` and `Downloads/Completed/` (organizer.rs:1752-1756; `remove_empty_dirs` at :1111-1146 never acts elsewhere). So `Movies/Heat (1995)/` and `Season 01/` stay on the share.
- Deleting for good only happens through Empty trash, or through retention at a rescan (`empty_trash`, organizer.rs:1965-2024; crates/stash-core/src/library/scanner.rs:1120-1131). Routes: `GET /api/trash`, restore, `DELETE /api/trash` (crates/stash-server/src/api/trash.rs:16-19). There is no per-item delete.
- Web:
  - Only the item page's "…" menu has "Move to Trash" (web/src/pages/ItemPage.tsx:315-324, web/src/components/library/DeleteItemDialog.tsx).
  - Inbox rows have no delete (web/src/pages/InboxPage.tsx).
  - Trash rows only offer Restore (web/src/pages/settings/LibrariesSettingsPage.tsx:503-529; Empty trash at :551-563).
  - Nothing can be selected anywhere.
- Android:
  - Nothing can be deleted. The item screen plays and sets favourites (`<app>/feature/item/ItemScreen.kt:196-214`).
  - Imports rows have Ignore only (`<app>/feature/downloads/ImportRows.kt:119-122`).
  - "Sorting and trash" opens the web page (`<app>/feature/settings/SettingsScreen.kt:76`).
- The API does not tell a client who added a file (working.md: "the API has no `added_by`"), so a client cannot tell which files are the member's own.
- Not audited today: retention deletes, and removing a download with its files (crates/stash-server/src/api/downloads.rs:214-227).

### Design
**Core (`stash-core`):**
- Move `empty_trash`'s per-item step (organizer.rs:1975-2015) into `purge_trashed_item`. That step deletes `trash_dir_for(id)`, the row and the cached media.
- `Library::delete_items(ids, permanent, user)`, per item:
  - Take the move lock and read the row again inside it.
  - Refuse `Zone::Internal` (unless the item is in the Trash), `InProgress` and `OtherDownloads` (paths.rs:24-59).
  - Refuse an item whose library folder is missing or read-only, with the existing 409. Otherwise an unmounted disk would look like a successful delete.
  - Same disk: `trash_item`'s move, then `purge_trashed_item`. The actual unlink then only happens inside the Trash.
  - Different disk (EXDEV): when deleting permanently, unlink in place instead of copying. The path comes from `layout.abs`. `symlink_metadata` must show a regular file or a link, and the size must match the row. `remove_file` removes a link itself, never its target. Sidecars are handled the same way.
  - The row is deleted before the lock is released, so the watcher never marks the item missing.
  - A missing file: only the row and its cached images go.
  - Publish the library "removed" event and the inbox count.
- Extend `remove_empty_dirs` for explicit trash and delete. It removes completely empty folders going up from the file's folder and stops below the top-level folder (the library folder, `Other/`, or a top-level folder). It never removes the library folder (docs/architecture.md:146: an empty library folder reads as missing).
- `TranscodeManager::stop_for_item`, next to `stop` (crates/stash-core/src/media/transcode.rs:263). A permanent delete ends open streams of that item at once.

**API (stash-server), contract changes:**
- `DELETE /api/items/:id` still moves to the Trash. `?permanent=true` deletes for good (owner only).
- `POST /api/items/delete {ids: string[] (1-500, no repeats), permanent: bool}` → `{done: string[], failed: [{id, message}]}`. It is not all-or-nothing, because a filesystem cannot roll back. An id the caller cannot see fails like one that does not exist.
- `POST /api/items/delete/preview {ids}` → `{count, bytes, files: first names, leftovers: [{folder, names (first 5), more}], crosses_disks}`, for the confirms.
- `DELETE /api/trash/:id` (404 when the item is not in the Trash), and `POST /api/trash/delete {ids}`. `DELETE /api/trash` is unchanged.
- No route takes a show, a season or a folder. Clients expand those to item ids from lists they already loaded (`GET /api/libraries/:id/shows/:name`). Only files the person saw are deleted, and nothing becomes a recursive delete of a directory.
- Member rule: Move to Trash is allowed when the item is in Other files or waiting or ignored in the Inbox, and `added_by` is the caller. Library files, restore and delete-for-good stay owner-only. Limited tokens (TV, extension) are refused, as today.
- `ItemDetail` gains `can_trash` and `can_delete` (worked out by the server), so clients show the right actions without being told who added the file.
- New audit events:
  - `item_deleted {item_id, title, rel_path, size, from: "trash"|"library"}`;
  - `trash_expired {deleted}` (no user);
  - `download_files_deleted {download_id, bytes}` (no name).
- Names in the audit log follow item 5's 90-day rule.

**Web (Astryx; Taste and Impeccable):**
- Item page "…" menu: "Move to Trash", then "Delete permanently…".
- A "Select" button on library grids, Other files, show pages (with "Select all" per season), the photo timeline and search results:
  - tiles get checkboxes;
  - a bottom toolbar shows "12 selected · 18.4 GB" with Move to Trash, Delete permanently and Cancel;
  - Shift-click selects a range, and Esc leaves selection.
- Show page: "Delete show…", and "Delete season…" on each season.
- Inbox rows, waiting and ignored: "Move to Trash" and "Delete permanently…".
- Trash: "Delete now" on each row, row selection with "Delete selected", Empty trash as before.
- The owner Move to Trash toast offers Undo, which restores; members receive confirmation without Undo because restore remains owner-only.
- Confirms:
  - Several to the Trash: "Move 12 items to Trash?" with the retention period. When the files cross disks: "Moving these to Trash copies 38 GB to the stash disk first."
  - Permanent: "Delete “Heat (1995)” for good?" with the size and "Everyone's progress on it goes too."
  - A season: list the first 5 episodes, "+5 more", the total size, and "Other files in the folder stay: poster.jpg and 1 more."
  - Destructive buttons say exactly what happens ("Delete 10 episodes").

**Android (Compose, android/DESIGN.md):**
- ApiClient and models for the new routes, plus `GET /api/trash`, restore and empty.
- An owner overflow menu on the item screen (and for members where `can_trash`): Move to Trash, Delete permanently.
- Long press starts selection in grids, season lists, Other files and the photo timeline, with a contextual bar showing the count and both actions.
- Imports rows get Move to Trash and Delete permanently.
- Keep the existing owner web Trash entry this round (owner decision, 2026-10-15); document it in docs/android.md.

### Tests
- Core:
  - a file that is a symlink: the link goes, the target stays;
  - a parent folder that links outside the root is refused;
  - a missing file;
  - a missing library folder answers 409;
  - the library folder is never removed;
  - empty folders are removed up to, but not including, the top-level folder;
  - a folder holding a poster is kept;
  - sidecars go;
  - a race with the watcher leaves nothing marked missing;
  - the EXDEV path (inject the rename failure);
  - an open transcode session stops.
- Server:
  - a member can trash their own Other file and their own Inbox file;
  - a member is refused for library files, other people's files, restore and permanent delete;
  - TV and extension tokens are refused;
  - unseen ids answer exactly like missing ones;
  - partial failure results;
  - the preview lists leftovers;
  - each audit event is written.
- Web: vitest for selection logic and dialog wording; e2e specs for trash, delete, season delete and Undo.
- Android: unit tests for the selection state and action rules, and Roborazzi screenshots of the menus, selection bar, dialogs and Trash.

### Docs
- docs/architecture.md: section 3, Movement rules (explicit delete for good; the empty-folder rule); section 5 (the move lock list); section 8 (routes, `can_trash`, `can_delete`); section 11.
- PRODUCT.md, Honest boundaries: delete for good cannot be undone; extras stay; another disk is copied when moving to the Trash; members may trash their own private files.
- docs/security.md:42 and :137.
- docs/android.md.

### Live validation
- In the scratch library on the owner's server, with your own test files: trash and restore one file; delete one for good; delete a season and confirm on the share that `Season 01/` is gone and the show folder holds only what the confirm named; use "Delete now" in the Trash.
- Sign in as a test member (invite a second account): trash their own Other file, and confirm that library files show no delete.
- Check the audit log entries. Never point any of this at the owner's real media.

## 5. Download history

### Decision (2026-10-14)
- One household switch, "Keep download history", owner-only.
- Turning it off clears the existing finished entries at once, after a confirm. No library files are touched.
- Defaults the owner accepted without being asked (you may revisit them with the owner):
  - With history on, entries stay until a new "Clear history". Owners clear everyone's; members clear their own.
  - With history off, failed downloads stay up to 7 days so they can be retried.
  - Info-level server logs name downloads by id only, and compose gets log rotation.
  - The audit log keeps file names for 90 days, then only ids and counts.
  - Removing an unfinished download asks "Also delete the downloaded files", ticked by default for unfinished ones, the same on web and Android.

### Current behaviour
- Every finished, sorted and failed row stays in `downloads` forever. The only deletes are a person's Remove (crates/stash-server/src/downloads/manager.rs:791) and failed adds (torrents.rs:188, 266-271).
- No setting exists (settings.rs:67-89; docs/architecture.md:397-403).
- Clients reach only the newest 100 rows: web/src/components/downloads/useDownloads.ts:61; the server clamps to 500 (crates/stash-server/src/live.rs:210); there is no cursor.
- Visibility (live.rs:165-176): a member who sees a sorted item can remove that download for everyone (crates/stash-server/src/api/downloads.rs:214-227).
- Sorting marks the row `sorted` and sets `items.source_download_id` (organizer.rs:1188-1219). `download_files_gone` is at :1222.
- Sort jobs are kept 1 day when done and 7 days when failed (crates/stash-core/src/jobs.rs:179-195). Upload rows are kept 7 days (crates/stash-server/src/uploads.rs:504-523).
- Info logs that name downloads or paths:
  - manager.rs:451 "web download queued";
  - torrents.rs:999 "torrent finished";
  - http.rs:938 "web download finished";
  - crates/stash-torrentd/src/adopt.rs:404 "adopted a finished download";
  - organizer.rs:785 "sorted a file";
  - also the organize, trash and restore lines (organizer.rs:1611, 1761, 1957) and a rescan move (scanner.rs:826).
- compose.yaml:26 passes `STASH_LOG` (default `info`), and compose.yaml sets no `logging` options.
- The audit log is never pruned (its only write is crates/stash-server/src/auth/users.rs:128). `item_trashed` and `item_restored` keep titles and paths.
- SQLite runs in WAL mode with no `secure_delete` (crates/stash-core/src/db.rs:46).
- Removing an unfinished download:
  - The web checkbox is off by default (web/src/components/downloads/RemoveDownloadDialog.tsx:37).
  - Android always deletes the files for active and failed downloads (`<app>/feature/downloads/DownloadsController.kt:505`) and hides Remove for completed and sorting downloads (:508-509).
- The extension waits up to 10 s for a sent download to show up. A download sorted and removed within that time would read as "waiting" (extension/src/background.ts:113-128; extension/src/lib/intercept.ts:58-59).
- The torrent engine already forgets finished torrents. Nothing changes there.

### Design
**Setting.** `keep_download_history: bool`, default true, so upgrades keep today's behaviour. It goes in `Settings` and its `Default` (settings.rs:67-89). `#[serde(default)]` covers rows saved earlier. The existing owner-only `PUT /api/settings` audits it as `settings_changed`.

**History on:**
- New `POST /api/downloads/clear {scope: "mine"|"everyone"}` → `{removed}`:
  - `everyone` needs a full owner.
  - It removes only sorted, failed and cancelled rows, never queued, downloading, paused, seeding, completed or sorting ones.
  - It deletes only a failed download's own files under `In Progress/` or `Completed/`, through `is_deletable_download_path` (manager.rs:1149-1163). It never touches sorted files.
  - Each removed row sends `{id, removed: true}` to everyone who saw it, and sets `items.source_download_id` to NULL.
  - It writes `download_history_cleared {removed}` (a count, no names).
- A "Clear history…" button in the Finished header of the Downloads page (web and Android) and in Settings › Downloads. Confirm: "Clear 37 finished downloads?" / "They leave the list for everyone. Sorted files stay in your library."

**History off:**
- When files are sorted, `finish_download` (organizer.rs:1188) still copies `added_by` onto the items, which access control needs (docs/architecture.md:408). In the same transaction it deletes the download row instead of marking it sorted, and does not set `source_download_id`.
- The creator's open screens get one last `download` event, state `sorted` with its destinations (shown as a toast or snackbar: "Heat (1995) was sorted into Movies"), then `{id, removed: true}`. `download_files_gone` (organizer.rs:1222) does the same.
- Failed rows stay up to 7 days for a retry. An hourly sweep then removes the row and its own partial files (`is_deletable_download_path`).
- An upload's file name is cleared from its `uploads` row when it completes or is cancelled, and the row goes at the next hourly sweep (uploads.rs:504-523). Done sort jobs are deleted when they finish (jobs.rs:179-195).
- Rows still exist while a download runs (queued to sorting). The server needs them to run, resume and show progress. Settings and PRODUCT.md say this plainly.
- Switching off: confirm "Turn off download history?" / "The 37 finished downloads in the list are cleared now, for everyone. Downloads still show while they run, and failed ones for up to 7 days. No files are deleted." This clears sorted and cancelled rows. Failed rows follow the 7-day rule. Switching back on keeps downloads from then on; nothing that was cleared comes back.
- `GET /api/downloads` answers `{items, history_kept}`.
- Turn on `PRAGMA secure_delete` in db.rs, and checkpoint the WAL after a clear.

**Logs and audit (both settings):**
- The info lines listed above log ids only; names and paths move to debug. Warn lines with `.part` paths (http.rs:475-880) stay, because they are needed to fix disk problems. Note that in the docs.
- compose.yaml gets log rotation: `logging: {driver: local, options: {max-size: "10m", max-file: "3"}}` or the json-file equivalent. docs/deployment.md tells Unraid users to use Unraid's Docker log rotation.
- An hourly job removes `title` and `rel_path` from `item_trashed`, `item_restored` and `item_deleted` entries older than 90 days. Who did what, and when, stays. Downloads stay unaudited, apart from the clear count.

**Removing a download, web and Android the same.** "Also delete the downloaded files" is ticked by default for queued, downloading, paused and failed downloads, and unticked for seeding or completed ones, whose files are whole. Change web/src/components/downloads/RemoveDownloadDialog.tsx:37, and `<app>/feature/downloads/DownloadsController.kt:500-516` plus `<app>/feature/downloads/DownloadSheets.kt` (add the checkbox).

**Clients:**
- Web:
  - a new Settings section "Downloads" (web/src/pages/SettingsPage.tsx:26-31) with the switch and "Clear download history now";
  - the Torrents group moves there from the VPN page (web/src/pages/settings/vpn/TorrentSettings.tsx);
  - `types.ts` and `SETTING_NAMES` (web/src/components/settings/describe.ts:136-151);
  - when off, "Finished" becomes "Needs attention" (failed only) with "Finished downloads are not kept.", and a toast when your download is sorted.
- Android:
  - the same off-state wording, a snackbar when a download is sorted, and Clear history;
  - the owner switch through a "Downloads" row in the web settings list (`SettingsScreen.kt:74-84`). A native switch needs a `/api/settings` client and can come later.
- Extension:
  - the popup reads `history_kept` and says "Finished downloads are not kept on this server.";
  - `confirmServerStarted` counts a download that disappeared after it was accepted as started (background.ts:113-128; intercept.ts:58-59);
  - with history off, it clears its own "Sent to PopcornStash" notification after showing it (background.ts:174).

### Tests
- Settings: default true, merge, and an old row without the key loads.
- History off:
  - a sorted download's row is deleted after `added_by` is set;
  - the event order is sorted, then removed;
  - `source_download_id` stays NULL;
  - members still see their sorted Other files;
  - a failed row and its partial files go after 7 days, and only paths inside `In Progress/` or `Completed/`.
- Switching off clears sorted and cancelled rows and keeps failed and active ones.
- Clear: a member clears only their own; `everyone` needs a full owner; active rows are never touched.
- An extension token's listing includes `history_kept`.
- Audit pruning strips names after 90 days and keeps the rest.
- The log lines carry ids only (capture with a tracing test subscriber).
- `secure_delete` is on.
- Remove dialog defaults: web vitest, Android unit test.
- Update the e2e specs that call `/api/settings` (tests/e2e/specs/02-library.spec.ts, 07-people-and-devices.spec.ts, 09-system.spec.ts).

### Docs
- docs/architecture.md: section 6 (settings keys, 397-403); section 7 (:500, rows can now disappear); section 8 (`/api/downloads/clear`, `history_kept`); section 10 (clients); section 11 (:848).
- PRODUCT.md, Honest boundaries.
- docs/security.md:40 and :137.
- docs/deployment.md (log rotation, what the logs keep).
- docs/android.md, docs/extension.md, working.md.

### Live validation
- Back up `/config`, deploy, add a small legal download, and watch the row become sorted (history on).
- Turn history off: the list clears, and a new download shows a toast and leaves the list.
- `docker compose logs popcornstash | grep -iF '<the file name>'` finds nothing at info level.
- `docker inspect popcornstash --format '{{json .HostConfig.LogConfig}}'` shows the rotation.
- Turn history back on if the owner wants it on.

## 6. Choose the destination library when adding a download

### Decision (2026-10-14)
- When adding a download (magnet, link, .torrent, upload, Android share, extension), the person can choose the destination library, or leave it to automatic sorting, which is the default.
- Server: an optional `library_id` on `POST /api/downloads`, and on the upload and share paths, validated against the libraries the caller can see.
- The sorter uses it instead of the claims for files of that library's type. The rest fall back to normal sorting.
- This is a contract change in docs/architecture.md.
- Not researched beyond this. Read the sorter (`crates/stash-core/src/library/organizer.rs`) first.

### Current behaviour (starting points)
- `AddDownloadRequest {url, filename, headers}` (crates/stash-server/src/downloads/manager.rs:103-109). The handler is crates/stash-server/src/api/downloads.rs:123-130, and `add_url` is manager.rs:344.
- Upload create: crates/stash-server/src/api/uploads.rs:31-38. `.torrent` upload: `add_torrent` in api/downloads.rs:132+. The Android share and the web share target both use `POST /api/uploads` and `POST /api/downloads` (docs/architecture.md:769, 816).
- The sort job payload is `{rel_path, download_id}` (organizer.rs:166-184). `SortJob` holds `download_id`, `added_by`, `auto_sort` and the claims generation (organizer.rs:613-621).
- The claim lookup is `library_defs.iter().find(|d| d.claims(c.category))` (organizer.rs:746-773), followed by the confidence and `auto_sort` rules and `defs::ready_folder`. `Shape::fits` (crates/stash-core/src/library/defs.rs:119) says which kinds a library shape can hold.
- Today the person who added a file may already accept it from the Inbox into any library they see that fits (docs/architecture.md:232). Choosing up front is the same power, used earlier.
- The extension token cannot list libraries: `GET /api/libraries` needs `library:read` (docs/architecture.md:621).

### Design (destination policy confirmed 2026-10-15)
- Store the choice on the download: a new nullable `downloads.library_id` column (`REFERENCES libraries(id) ON DELETE SET NULL`) and `uploads.library_id`, copied to the download when the upload completes.
- Validation: the library must exist and be visible to the caller. Otherwise answer exactly like a missing library, so hidden libraries are not revealed.
- Sorter: `SortJob` gains `library_id`, read from the download row. For each file:
  - If the file's category fits that library's shape (`Shape::fits`, and Photos for photos), the chosen library replaces the claim lookup at organizer.rs:746-749.
  - Otherwise, the normal claim lookup.
  - A compatible explicit choice bypasses confidence and `auto_sort`; claims generation and unavailable storage still prevent unsafe moves. Unidentified videos can enter a chosen one-off library using its normal movie/camera filing rules.
  - Owner decision: move compatible files into the chosen library immediately, even with Low confidence or `auto_sort` off.
  - If the chosen library was deleted (`library_id` now NULL) or hidden from the person meanwhile, files sort automatically.
- Contract:
  - `library_id?` on `POST /api/downloads`, the `.torrent` multipart (`library_id` field) and `POST /api/uploads`;
  - `Download.library_id` (only when the caller sees it);
  - for the extension, a narrow `GET /api/downloads/destinations` (scope `downloads:create`) → `[{id, name, icon, kinds}]` for the libraries the caller sees, instead of giving the extension `library:read`.
- UI: a "Where it goes" select, default "Sorted automatically":
  - on the web add panel, uploads and the share page;
  - on Android's add-link sheet and share confirm;
  - in the extension popup. The right-click menu stays automatic.
  - Show only libraries that fit something, with their icons. Design with Taste and Impeccable.

### Tests
- A movie sent to a second One-offs library (for example "Kids") lands there. Photos in the same download go to the photo library by the claims.
- An unseen or nonexistent `library_id` is refused the same way.
- The library is deleted before sorting: automatic sorting.
- Claims change mid-job: the existing generation rule still holds.
- Low confidence moves immediately into a compatible chosen library; without a choice it still waits. An unavailable chosen folder still waits with its suggestion.
- An upload carries the choice through `/complete`.
- The `.torrent` multipart field works.
- The extension destinations endpoint lists only visible libraries.
- Web and Android unit tests for the select. An e2e spec.

### Docs
docs/architecture.md section 4 (claims; a chosen library), section 6 (columns, migration), section 8 (request bodies, `Download.library_id`, destinations), section 10; PRODUCT.md Journey 4 and the "Owners decide where each kind of file goes" boundary (PRODUCT.md:30); docs/extension.md; docs/android.md.

### Live validation
In the scratch setup, make a second One-offs library ("Test movies"). Send a legal short film with that destination and check the file lands in `Test movies/Title (Year)/`. Send another with automatic sorting, and check it goes to Movies.

## 7. Hardware acceleration: Ryzen 5 PRO 4650GE now, Intel Arc later

### Decision (2026-10-14)
The owner has a Ryzen 5 PRO 4650GE now and plans an Intel Arc card later. There is no "A330"; the owner probably means an A310 or an A380. Set up VAAPI on the Ryzen now. Then do the repo work in this order:
1. render node choice for two GPUs, and docs for `compose.override.yaml`;
2. diagnostics;
3. VAAPI rate control;
4. later: hardware decode and scaling, a remux path, AV1/HEVC output.

### What the Ryzen gives
- The iGPU (Renoir, VCN 2.2) does only the H.264 encode. Output is always H.264, AAC stereo 160k, 6 s MPEG-TS segments (crates/stash-core/src/media/transcode.rs:920-988).
- Decoding, scaling and HDR tone mapping stay on the CPU. There is no `-hwaccel` (transcode.rs:889-905; filters.rs:7-17). 4K HDR HEVC still loads the six cores.
- No AV1 at all (AV1 decode starts with VCN 3.0).
- MKV never plays directly (crates/stash-core/src/media/playback.rs:30), and every HLS stream re-encodes (docs/architecture.md:287). So most torrent plays transcode, and the GPU helps a lot there.
- VAAPI runs at constant QP (`-rc_mode CQP -qp 23`, transcode.rs:936). AMD's older H.264 encoder has no B-frames, so streams are larger. If remote playback stutters, choose "Smaller streams" under Quality.

### Setup steps on the NAS (from the research guide)
1. **BIOS:** keep the integrated graphics enabled; UMA frame buffer Auto or 512 MB.
2. **Host check:** `lsmod | grep amdgpu`, `ls -l /dev/dri` (look for `renderD128`), `ls -ln /dev/dri` (its group must not be 0), `cat /sys/class/drm/renderD128/device/vendor` (expect `0x1002`).
3. **Host driver:**
   - Debian, OMV or Proxmox: `sudo apt install firmware-amd-graphics` (non-free-firmware), then reboot. Ubuntu already has it.
   - Unraid: recent releases load amdgpu at boot. If `/dev/dri` is missing, install the "Radeon TOP" plugin.
   - TrueNAS SCALE 25.04: check that `/dev/dri` exists.
   - Proxmox with Docker in an LXC: pass `/dev/dri/renderD128` to the LXC.
   - Synology: no driver; stay on software encoding.
4. **Compose, without editing tracked files.** Next to `compose.yaml`, create `compose.override.yaml`:
   ```yaml
   services:
     popcornstash:
       devices:
         - /dev/dri:/dev/dri
   ```
   Then run `echo compose.override.yaml >> .git/info/exclude`. `docker compose config` must list both `/dev/net/tun` and `/dev/dri`. On the Unraid template: add a Device with the value `/dev/dri`.
5. **Groups:** no `group_add`, no `privileged`, no chmod. The supervisor gives the web server renderD128's group (crates/stash-torrentd/src/supervisor.rs:486-495) and replaces all supplementary groups with that one (crates/stash-torrentd/src/children.rs:151-154), so `group_add` would be ignored anyway. A renderD128 owned by group 0 must be fixed on the host (a udev rule).
6. **Recreate:** `docker compose up -d` (add `--build` only after a `git pull`). This also clears the cached detection result (crates/stash-core/src/media/tools.rs:157-162).
7. **Choose the encoder:** leave `STASH_HWACCEL` empty and pick "VAAPI (Intel and AMD)" in Settings > System > Streaming > Hardware acceleration, after the recreate. Keep 1080p and 2 concurrent streams.
8. **Check 1, the app's detection:** the Streaming description reads "Hardware acceleration found on this server: VAAPI (Intel and AMD)", and `docker compose logs popcornstash | grep 'hardware encoders detected'` shows `available=["vaapi"]` (tools.rs:185).
9. **Check 2, the driver:** `docker exec popcornstash ffmpeg -hide_banner -loglevel verbose -vaapi_device /dev/dri/renderD128 -f lavfi -i color=c=black:s=256x256:r=25:d=1 -frames:v 1 -vf format=nv12,hwupload -c:v h264_vaapi -f null -` exits cleanly and prints "VAAPI driver: Mesa Gallium driver … radeonsi, renoir".
10. **Check 3, speed on a real file:** run
    `docker exec popcornstash ffmpeg -hide_banner -vaapi_device /dev/dri/renderD128 -i "/stash/Movies/<film>.mkv" -t 60 -an -vf scale=-2:1080,format=nv12,hwupload -c:v h264_vaapi -rc_mode CQP -qp 23 -f null -`
    and compare it with
    `docker exec popcornstash ffmpeg -hide_banner -i "/stash/Movies/<film>.mkv" -t 60 -an -vf scale=-2:1080,format=yuv420p -c:v libx264 -preset veryfast -crf 23 -f null -`.
    The `speed=` figure is roughly how many such streams the box can run at once. Try a 4K HDR file too.
11. **Check 4, real playback:** play an `.mkv` in a browser. `docker compose logs -f popcornstash | grep -E 'transcode started|libx264|ffmpeg:'` shows `encoder=Vaapi` (transcode.rs:236) and no "using libx264" warning.
12. **Check 5, the GPU is working:** run `sudo grep -H drm-engine-enc /proc/$(pgrep -n -x ffmpeg)/fdinfo/*` twice, a few seconds apart. The number grows. CPU use in `top` is clearly lower than during the libx264 run.
13. **If VAAPI is not found:** `docker exec popcornstash ls -ln /dev/dri`; set `STASH_LOG=debug`, recreate, open Settings > System, and grep for "hardware encoder unavailable" (tools.rs:231). Put `STASH_LOG` back afterwards. An encoder that fails mid-stream stays off until restart (transcode.rs:432-449).

Record the results (detection line, driver line, both speed figures, the encoder in the log) in working.md and CHANGELOG.md. They are the first real-GPU evidence.

### Prompt for an agent running on the NAS (from the research guide, verbatim)
```
Set up hardware video encoding for PopcornStash on this server using the AMD Ryzen 5 PRO 4650GE's built-in Radeon graphics (VA-API).

Rules:
- Ask me before any BIOS change, reboot, kernel or package install, or anything that restarts other containers.
- Do not edit compose.yaml or any other tracked file in the PopcornStash clone.
- Do not use privileged mode, group_add or chmod 777, and do not touch media files.
- Recreating the popcornstash container is fine.

1. Find out the OS (Unraid, TrueNAS, Debian/Ubuntu/OMV, Proxmox or Synology) and how PopcornStash runs: compose from a git clone (the folder with compose.yaml and .env) or the Unraid template. On Synology, stop and tell me.
2. On the host, run `lsmod | grep amdgpu`, `ls -ln /dev/dri`, `ls -l /dev/dri/by-path/` and `cat /sys/class/drm/renderD128/device/vendor` (expect 0x1002). If renderD128 is missing, propose the fix and wait for my OK: firmware-amd-graphics on Debian, the Radeon TOP plugin on Unraid, then a reboot. Report renderD128's group number. If it is 0, stop and ask.
3. Next to compose.yaml, create compose.override.yaml:
   services:
     popcornstash:
       devices:
         - /dev/dri:/dev/dri
   Add it to .git/info/exclude. Run `docker compose config` and confirm the devices list has both /dev/net/tun and /dev/dri. (On the Unraid template instead: add a Device with value /dev/dri.) Then run `docker compose up -d`.
4. Leave STASH_HWACCEL empty in .env. Ask me to choose "VAAPI (Intel and AMD)" in Settings > System > Streaming > Hardware acceleration, and wait until I confirm.
5. Run these checks and report the output of each:
   a) `docker compose logs popcornstash | grep "hardware encoders detected"` (expect vaapi).
   b) `docker exec popcornstash ls -ln /dev/dri`.
   c) `docker exec popcornstash ffmpeg -hide_banner -loglevel verbose -vaapi_device /dev/dri/renderD128 -f lavfi -i color=c=black:s=256x256:r=25:d=1 -frames:v 1 -vf format=nv12,hwupload -c:v h264_vaapi -f null -` (report the "VAAPI driver" line and the exit status).
   d) Pick one .mkv under /stash and report speed= for both of these:
      `docker exec popcornstash ffmpeg -hide_banner -vaapi_device /dev/dri/renderD128 -i "<file>" -t 60 -an -vf scale=-2:1080,format=nv12,hwupload -c:v h264_vaapi -rc_mode CQP -qp 23 -f null -`
      `docker exec popcornstash ffmpeg -hide_banner -i "<file>" -t 60 -an -vf scale=-2:1080,format=yuv420p -c:v libx264 -preset veryfast -crf 23 -f null -`
   e) Ask me to play an .mkv in the browser. Then grep the log for "transcode started" (expect encoder=Vaapi), "using libx264" and "ffmpeg:". While it plays, run `sudo grep -H drm-engine-enc /proc/$(pgrep -n -x ffmpeg)/fdinfo/*` twice, 5 s apart (the number should grow), and note processor use from top.
6. If vaapi is not found: set STASH_LOG=debug in .env, run `docker compose up -d`, ask me to open Settings > System, grep the log for "hardware encoder unavailable" and report it. Then set STASH_LOG back to info and recreate the container.

End with a short report: OS and kernel, driver loaded, renderD128's owner and group, pass or fail for each check with its key line, both speed figures, and every file or setting you changed.
```

### The Arc, later (for the owner)
- **Before buying:**
  - Confirm the model: the A310 (often low-profile and powered by the slot) or the A380. Avoid Battlemage (B570/B580): those cards need Resizable BAR for video, kernel 6.12+, and 150-190 W.
  - Confirm the box has a free x16 slot. The 4650GE often ships in 1-litre mini PCs (ThinkCentre M75q Gen 2 Tiny, EliteDesk 805 G6 Mini) that have none.
- **Host:**
  - A-series needs kernel 6.2+ with i915 (Unraid 7.0+, Debian 13 and TrueNAS 25.04 are fine; Debian 12's stock kernel is not) and `firmware-intel-graphics`. Do not set `i915.enable_guc`.
  - BIOS: iGPU Enabled or Forced (not Auto) as the primary display, Native ASPM and PCIe ASPM L1 on. Otherwise an A380 idles at about 15-20 W.
- **Container:**
  - Nothing to install: the amd64 image has iHD and `libmfx-gen1.2` (Dockerfile:193-203).
  - Until the render-node setting below exists, map only the Arc as renderD128 in `compose.override.yaml`:
    ```yaml
        devices:
          - source: /dev/dri/by-path/pci-0000:03:00.0-render   # the Arc's address
            target: /dev/dri/renderD128
            permissions: rwm
    ```
  - If `docker compose config` rejects the long form, use `- /dev/dri/renderD129:/dev/dri/renderD128` with the real number, and recheck it after kernel or BIOS updates. Or switch the iGPU off.
- **Then:** try "Intel Quick Sync" first, and "VAAPI" if Quick Sync fails. Verify with `intel_gpu_top`.
- **AV1:** PopcornStash cannot use the Arc's AV1 today. AV1 decode on DG2 also needs `intel-media-va-driver-non-free`.

### Repo work, in priority order
1. **Render node choice and GPU docs** (about a day with tests):
   - `STASH_RENDER_DEVICE` (a path, or a PCI address resolved through `/dev/dri/by-path`), used for:
     - detection (tools.rs:19-20, 168-174, 197-199);
     - `-vaapi_device` (transcode.rs:893-895);
     - an explicit QSV device (today QSV passes none, tools.rs:215, and FFmpeg's vendor scan stops at the first node it cannot open);
     - the supervisor's `render_group` (supervisor.rs:486-495).
   - docs/deployment.md:273 tells clone users to uncomment the line in compose.yaml, which makes `git pull` conflict-prone. Document `compose.override.yaml` instead, plus the Unraid amdgpu note and the two-GPU mapping.
   - Add an optional, empty Device entry to unraid/popcornstash.xml (around :15 and :23). It must be empty by default, because Docker refuses to start when a device path is missing.
   - Update `.env.example` and docs/architecture.md section 2.
   - Tests: the setting parses; detection and the argv use the chosen node; `render_group` has a test (it has none today).
2. **Diagnostics** (small to medium):
   - When the chosen encoder fails detection, log at warn (today it is debug, tools.rs:231, 235).
   - Show ffmpeg's error in Settings.
   - A "Test again" action that clears the cache (tools.rs:157-162) and the broken marks (transcode.rs:432-449).
   - Record the driver name from the test encode, and a per-encoder "fell back" flag, in `GET /api/system`'s `hwaccel` (crates/stash-server/src/api/system.rs:119-141). This is a contract change.
   - Name the GPU in the UI, instead of only "VAAPI (Intel and AMD)" (web/src/pages/settings/SystemSettingsPage.tsx:84-89).
   - Optionally add `vainfo` to the image.
3. **VAAPI rate control:** QVBR or VBR with a maximum bitrate per resolution instead of CQP (transcode.rs:936). Tune it on the Ryzen with the speed test and real playback.
4. Tiny: docs/architecture.md:287 says "fMP4 or MPEG-TS segments", but the code writes MPEG-TS only (transcode.rs:985-986). Fix the doc.
5. **Later:**
   - Hardware decode, scaling and tone mapping per mode, with software decode where the hardware lacks a codec.
   - A remux path that copies the video when it is already fine (owner decision D1: after 0.1.0).
   - AV1/HEVC output: fMP4 HLS, a choice per client, and on Android only hardware AV1 decoders counting (`<app>/data/device/PlaybackCapabilities.kt:25-30`).
   - The non-free Intel driver, only once hardware decode exists (an owner decision about licensing).

## 8. Updates

### Decision (2026-10-14)
- The server has no in-app updater; the owner skipped it. Updates are `git pull --ff-only` and `docker compose up -d --build` on the NAS (docs/deployment.md:329-341).
- The Android in-app updater, fed by GitHub releases built from every change on main, landed in `2b86f58` (Settings, "This app"; `<app>/update/`). No release has been published yet.
- Its first real release needs the signing key and a GitHub token, which the owner will set up. On the local machine the developer can hold them and run `scripts/android-release.sh`. Before the first publish, commit the release key's public fingerprint as `android/release-cert.sha256`; without it the script refuses (README, Releasing).

### Work
- Read docs/android.md (Updates) for the updater's flow and working.md for what its review left open (Android 9 signature reading, the notification wording, no restart after a self-update), then make the first real release with `scripts/android-release.sh`. Keep the keystore, its passwords and the GitHub token outside the repository, and never in commits or logs.
- **Signing key:**
  - An installed APK can only be updated by one signed with the same key. Moving the owner's phone from the debug-signed build to the release key needs one uninstall and reinstall (the app then pairs again). Tell the owner before it happens.
  - The release key's fingerprint must go into `STASH_ANDROID_ASSETLINKS` next to (or instead of) the current one, or passkeys break (item 2).
- **Validate on the phone:** install release N, publish N+1 from a change on main, and check that the app finds it, says what changed, downloads it, asks for "Install unknown apps" once, installs over the old version, and keeps the person signed in.
- **Server updates:** follow docs/deployment.md "Updating a clone of the repository" for every deploy. Back up app data before any deploy that adds a migration. After each deploy, check `docker compose ps` (healthy) and the log tail.

## 9. Android navigation bar on the right edge

### Decision (2026-10-14)
The Android navigation moved to a vertical bar on the right edge in `f75529d`. Validate it on a real phone. Open point from its review: in landscape, rows pass under the bar for most of a scroll (they clear it at the end of each page); a right-side content inset for wide windows is the owner's call.

### Validation on the owner's phone
- Reachable with one hand, in both hands' grips. Labels and the selected state are readable over posters and photos.
- With gesture navigation, a swipe from the right edge is Android's Back gesture. Check that taps on the bar never trigger Back, and that Back still works from the rest of the edge. Android limits gesture exclusion to 200dp per edge, so the bar cannot exclude itself entirely. If taps and Back conflict, report it before changing the layout.
- With 3-button navigation, and in landscape: the bar stays clear of the navigation bar, the cutout and the player. In the player and in picture-in-picture it is hidden.
- Font size at maximum, display size large, TalkBack (order, names, the selected state).
- Frosted glass: blurred on Android 12+, opaque on Android 9-11 and with the contrast setting at medium or high.
- Record screenshots in `evidence/` (git-ignored) and the results in working.md. Fix what fails with Taste and Impeccable, and update android/DESIGN.md and the Roborazzi screenshots.

## Open questions for the owner

1. **Passkeys in the app:** make `assetlinks.json` public (Tailscale Funnel on 443 for that path, with the app moving to :8443; or an own domain or public route, which needs a new passkey), or stay private and use pairing?
2. **Torrents:** should new installs default to Upload limit 16 KB/s? Recommendation: no. Peers favour clients that upload, so downloads would be slower for everyone, and the owner's own server is set by hand.
3. **Trash (answered):** owners only.
4. **Android Trash (answered):** keep the existing web Trash this round.
5. **Download history:** confirm the defaults accepted without being asked: kept until cleared; failed downloads 7 days when history is off; ids-only info logs with rotation; audit names for 90 days; "Also delete the downloaded files" ticked for unfinished downloads.
6. **Destination library (answered):** compatible files go into the chosen library immediately, including Low confidence or automatic sorting off.
7. **Extension fallback:** when the server is refused, should the browser start downloading on its own, or after a click that says it uses this computer's connection, not the VPN (the proposal)?
8. **The Arc:** which card (A310 or A380), and does the box have a free x16 slot? Later: is the non-free Intel driver acceptable in the AGPL image if hardware AV1 decode is ever added?

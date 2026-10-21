# Changelog

Changes to PopcornStash, newest first. Version numbers follow [Semantic Versioning](https://semver.org/). Releases are built, checked and published from a developer machine with `scripts/release.sh`, which takes the release notes from this file (see "Releasing" in the README).

## [Unreleased]

- Torrent review on web and Android exposes type, size and name filters immediately, updates selections while editing, and preserves manual choices until the next filter change. Invalid filters block Start/Save.
- Shared web header rows align with the brand; compact checkbox marks retain comfortable press targets.
- Android requires strong biometrics to open, with no PIN/pattern/password fallback and a fresh Keystore operation per visit.


- Fix Intel Arc/VAAPI AV1 compression presets being ignored; each quality option now controls the AV1 encoder on its native scale. Smaller-size and full integrity checks still protect originals.

- Two bounded conversion workers, GPU input decoding and full-video integrity checks, live verification progress, and Intel Arc AV1 decode support in amd64 images. Originals remain protected by complete-file checks; larger results are discarded before verification.

- Double tap the left/right player picture to seek back/forward 15 seconds on web and Android.
- Search navigation across all accessible files or one library; library subfolder browsing, creation and searched bulk moves that preserve watch history.
- Direct Inbox bulk deletion to recoverable Trash, including ignored images; stale background reads cannot restore deleted rows.

- Owner file inventory and selective cleanup by folder, file type and ignored status, including unindexed companions; recover files and whole folders from Trash.
- Optional confirmed library-folder removal on web and Android; default deletion keeps files.
- Inbox selection by download and file type, with bulk import destinations.
- Browser passkey lock after 15 minutes of inactivity, enforced independently of background requests. Android sessions and server downloads continue.


- Optional public tracker list in VPN settings, using ngosang's maintained best list through the VPN, with daily refresh, last-good fallback and private-torrent exclusion.
- Consistent nested menu and media corners; download percentages aligned with the speed/peer chips on web and Android.

Libraries the owner defines, each with its own icon and its own audience, and the first Android app.

### Libraries

- The fixed Movies, Shows, Home Videos and Photos folders become libraries the owner defines: a name, an icon from a curated list of 31 Lucide icons, Videos or Photos, for videos series (shows, seasons, episodes) or one-offs, which kinds of new file it takes (movies, TV episodes, camera videos, photos; each kind goes to one library at most), a folder, who sees it and its place in the order. Existing stashes keep working: they start with Movies, Shows, Home Videos and Photos as before, seen by everyone, and every item is assigned by its folder.
- A new library gets a new folder in the stash, or an existing top-level folder whose files are indexed where they are. Renaming a library never renames its folder, its type can change only while it is empty, and deleting it takes it out of the app without touching a file.
- New files go to the library that takes their kind, laid out by kind (`Title (Year)/`, `Show/Season 01/`, `2024/06/`). A kind no library takes waits in Inbox, where you pick the library. Music, documents and other files go to Other files as before.
- Who sees what: only owners make, change, reorder and delete libraries, and each library is shown to everyone or only to the people the owner picks. Someone who cannot see a library learns nothing about it anywhere: lists, Home, search, pictures, playback, downloads, the trash and live updates all treat its items as not there, and its files arriving or being sorted send them nothing.
- Other files are now private: each one is seen by the person who added it and the owners. Folders on the share that are not a library are seen by owners only, and Settings offers "Make a library" for each.
- Settings has a Libraries section for owners, with an icon picker; Inbox lets you choose the library; owners can move any item into a library ("Organize into library"). Old addresses (`/movies`, `/shows`, `/home-videos`, `/photos`) go to the library that takes that kind.

### Photos and home videos

- Photos and home videos show the time on the camera's clock where they were taken, the same for everyone in every time zone, and the Photos timeline and the `2024/06/` folders use that clock's month. A camera time with no time zone is no longer read as UTC, so it no longer shifts by a few hours, or into the next or previous month, for people west or east of UTC. Files that only say when they were taken in UTC (a file's own date, the UTC time most Android phones put in videos, Pixel file names) are placed in the server's time zone: set `TZ` (docs/deployment.md, Time zone; Unraid sets it for you).
- The API sends the camera's clock (`taken_local`) and the offset it was written with (`taken_offset`) beside `taken_at`. Upgrading reads every photo and home video's date again once, in the background.
- Home: a shelf for a library of series has a tile for every show (up to 12, the most recently added first) with its count of new episodes, so "3 shows, 18 episodes" shows three shows.
- A photo or clip moved by hand between Home Videos and Photos (or restored from the trash into a library that now reads it as the other kind) keeps the date its camera wrote. It used to take the file's modification time, which put a 2024 clip under the month it was moved.
- Videos from Android and Samsung phones named after the phone's clock (`VID_20240630_223000.mp4`, `20240630_223000.mp4`) show that clock and go to that month's folder whatever the server's time zone, because the name and the video's UTC time together say where it was filmed. Before, a server left on UTC showed 2:30 AM on July 1 for a clip filmed at 10:30 PM on June 30.

### Web app

- The sidebar lists Home, then every library you can see with its own icon, in the owner's order, then Other files, Inbox, Downloads and Settings; the phone menu and quick links follow it. Each library's page fits what it holds: posters or video frames, shows with seasons and episodes, or the photo timeline.
- A library that holds files shows what it holds as a plain fact in its settings ("Holds  Videos, arranged as series") instead of greyed choices.
- Messages say each thing once: "Checking VPN" alone while it checks, "VPN status unavailable" with what still works, no VPN line while the server does not answer at all, and no "Add the episode number" under a form field that already says so.
- The web app has unit tests (vitest) for its library and VPN rules, run by `npm --prefix web run check`.
- Photo months and capture times read the same wherever you are: west of UTC a month no longer showed as the one before it, and a camera's time no longer moved to your own time zone. The tests run the dates in eight time zones.
- On a photo's, video's or file's page the sidebar and quick links show which library it is in (or Other files, or Inbox).
- On phones, short folder names ("Grandma's Slides/") and dates ("Oct 14, 2026") keep to one line, and a shared link wraps after its slashes and hyphens instead of anywhere. A long folder name in Settings, Libraries wraps at its spaces instead of being cut off (a phone shows no tooltip), and on phones the folder's icon stays beside its name, with Make a library underneath.
- Switches are easy to see when they are off, in light and dark (a ring and knob with at least 3:1 contrast). Unchecked checkboxes and radios now have the same edge, so every control that is off or not chosen reads alike and at 3:1 or more. In the library editor the checkboxes line up with the choices above them.
- Settings, Devices starts with the Android app: what it does, where it comes from (for an owner, how to build it; for anyone else, from whoever runs the server), and how to get in: pair with a code, which always works, or a passkey once the server is set up for it. Installing PopcornStash from Chrome stays as the way to share from a phone without the app ("Share without the app").
- The Photos timeline follows a photo whose date changes while it is open (the one-time redating after an upgrade, or a photo whose EXIF is read later): it moves to its month, in order, with both months' counts, without a reload. A grid sorted by date taken keeps its order the same way.
- Everything on the blue is white: button labels and icons, checkbox marks, the knob of a switch that is on, badges. The blue is one shade deeper (Tailwind blue-600) so white reads at 5.3:1, and a hovered or pressed button gets darker instead of lighter (6.8:1 and 8.8:1). A checked box, radio or switch that is on also gets darker on hover, a clearer step in light. Focus rings, progress bars and the watched line on tiles use the same blue. So these thin marks keep 3:1, some fills changed: a download's progress track in dark, the Inbox edit form's fill in dark, the watched line's track, and in dark the tint of a banner (the passkey notice on the sign-in pages, "Try again" on a load error), where a button's focus ring sits. A theme test measures every pairing in light and dark, banners included.

### Chrome extension

- The Send and Connect buttons and the switch that is on are white on the deeper blue, in light and dark, and hover goes darker; the progress track, a notice that holds a button and an error notice with "Try again" keep the blue at 3:1 in dark, and the progress track shows more of the bar's length there. A unit test measures the pairings.

### Android app

- A new app for Android 9 and later (`android/`, docs/android.md), in the Popcorn design, light and dark with the system. Three tabs in a floating bar on the right edge of the screen, low enough for the thumb of the hand holding the phone, icons only: Downloads, Home (where it opens) and Settings, top to bottom. It stays on the right in every language and with the phone on its side. The pages slide up or down the way the selection moves, or fade when the phone's animations are off, and each tab keeps its place. Every page scrolls its last row clear of the bar, and messages appear at the bottom beside it. The photo viewer shows its own messages ("Original saved.") just above its controls, and a tab's message waits until the viewer closes.
- Sign in with your server's address, then pair with a code or use your passkey (`STASH_ANDROID_ASSETLINKS`, now passed through by `compose.yaml`).
- Home: what to carry on watching, then every library you can see; libraries as grids, shows by season, photos by month with a full-screen viewer; item pages and a full-screen player that resumes, saves your place, offers the next episode and keeps playing in a small window.
- Downloads: live progress, the VPN line, pause, resume, remove and add a link; the files waiting to be imported with the library picker, edit, accept, ignore and accept all; finished downloads say where they went. Links, photos, videos and files shared from other apps are added or uploaded.
- Settings: who you are, the server and sign out; owners manage the libraries here too. The other admin pages open the web settings.
- The app updates itself from GitHub: Settings, This app shows the version and build, checks for a newer build and installs it after checking its checksum and signature. Builds are published for changes to the app on main, not only for tagged releases. It asks GitHub once a day unless that is turned off, sends only its name and version, and posts one notification for each new build. A copy built on your own computer cannot update itself and says how to switch to the GitHub builds once (docs/android.md, Updates). No build is published yet: the release key and the GitHub token are not set up. The server (Docker) has no updater; upgrade it as before (docs/deployment.md, Upgrading).
- Photos and home videos show the camera's clock in the app too, whatever time zone the phone is in, and a waiting photo previews the month folder of that clock. A series shelf on Home has a tile for every show.
- Long words wrap where they should on narrow phones and with large text: a server address after its dots and hyphens (the sign-in page no longer squeezes "Version" into a column), a folder name in a sentence stays whole, a long folder name in Settings wraps instead of ending in "...", "Accept all" and photo counts are never split, and dates never break inside. The chosen season or file kind carries a check, not only a faint tint.
- Everything on the blue is white: the words and icons on buttons, the selected tab's icon in the floating bar and the app's launcher icon. The blue is one shade deeper (Tailwind blue-600), so white reads at 5.25:1, and pressing a blue button uses a fainter white ripple so its words stay readable while pressed. So that thin blue marks keep 3:1 against what is around them, a download's progress track in dark is darker, the track under the watched bar on a picture is nearly opaque, and the player's seek thumb has a thin white ring.
- The floating bar is frosted glass on Android 12 and later: the pages scroll under it blurred and tinted, and it follows scrolling and page changes as they happen. The selected tab's blue square has a thin ring so it stays clear over any picture, and while it slides each icon is white exactly where the square is under it. On Android 9 to 11, and when the phone's contrast setting is medium or high, the bar stays solid. On Android 17 QPR2 and later the system blurs what is behind the bar itself; that path has not yet run on a phone.
- Building the app needs the Android SDK Platform 37.2 (Android 17 QPR2): `sdkmanager "platforms;android-37.2"` (docs/android.md).
- Not yet run on a phone or an emulator; docs/android.md lists what is not done.

### Tooling

- `scripts/check.sh --e2e` also runs the Android app against a real server, never from Gradle's cache.
- `scripts/android-release.sh` publishes the Android app from a clean, pushed main as the GitHub pre-release `android-N` (N is the number of commits on main), signed with the release key from the environment and checked against `android/release-cert.sha256`. It skips when nothing under `android/` changed (README, Releasing).

### Fixed

- Moving selected images, subtitles and other files to Trash works on Unraid shfs shares that reject atomic no-replace rename. File restores retain collision protection.

- Pausing and resuming a magnet link that is still fetching its details works, also while torrents are blocked because the VPN is not protecting them, and a paused magnet stays paused after a restart.

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
- A torrent added while torrents were stopped for that reason said "Waiting for the VPN to connect." On the Downloads page and in the extension it now says "Blocked: VPN not protecting" with the reason.
- With `STASH_ALLOW_NO_VPN`, the server still saved a new VPN sent to it directly, although the VPN page hides the setup. It now refuses and says how to turn the VPN back on.
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

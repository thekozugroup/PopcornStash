# PopcornStash

A private home for your family's videos, photos and downloaded media. Part of the Popcorn series of open source video projects.

PopcornStash points at one folder on your NAS, Unraid share or home server and keeps it organized. Family members sign in with passkeys, browse the libraries the owner shares with them (a new stash starts with Movies, Shows, Home Videos and Photos; the owner can rename them, add more with their own icons and choose who sees each), and watch in any browser or in the Android app. New files arrive from torrents, web links, the Chrome extension, links, photos and videos shared from Android, browser uploads, or by copying them into the Inbox folder, and are sorted automatically.

## What it does

- **Passkeys only.** No passwords. The owner invites family members, and each person adds passkeys for their own phone and laptop.
- **Torrents that only use your VPN.** Paste a NordVPN access token (or upload any WireGuard or OpenVPN file). Only the torrent engine's traffic enters the tunnel; a kill switch stops it if the tunnel drops. The rest of the app stays on your normal network.
- **Every way in leads to the same place.** Torrents, links, uploads and drops all go through `Downloads/In Progress` and `Downloads/Completed`, then get sorted into the library that takes that kind of file, or `Other`. Anything uncertain, or a kind no library takes, waits in Inbox for one tap of review.
- **Your folders stay yours.** Files outside `Inbox/` and `Downloads/` are indexed where they are and never moved unless you ask.
- **Plays everywhere.** Videos play directly when the browser can, and are transcoded on the fly (HLS, with optional Intel/AMD/NVIDIA hardware encoding) when it cannot. Photos from phones, including HEIC, show in a timeline.
- **Nothing leaves the server.** No metadata scraping, no accounts, no telemetry. The Android app talks only to your server, apart from one question a day to GitHub about a newer build of the app (its name and version only; it can be turned off) and the download of that build from GitHub when you tap Update.

## Quick start

You need Docker and an HTTPS address for the server (passkeys require HTTPS on a domain name; Tailscale, Caddy, Nginx Proxy Manager or Cloudflare Tunnel all work, see [docs/deployment.md](docs/deployment.md)).

```sh
git clone https://github.com/thekozugroup/PopcornStash
cd PopcornStash
cp .env.example .env        # set STASH_PUBLIC_URL, APPDATA_PATH, STASH_PATH, PUID and PGID
# Create both folders and give them to PUID:PGID (here 1000:1000, the .env default):
sudo mkdir -p /srv/popcornstash/appdata /srv/media/PopcornStash
sudo chown 1000:1000 /srv/popcornstash/appdata /srv/media/PopcornStash
docker compose up -d
docker compose logs popcornstash | grep -A 1 "setup code"
```

Use your own `APPDATA_PATH`, `STASH_PATH`, `PUID` and `PGID` in the two folder commands. If the library folder does not belong to `PUID:PGID`, no setup code appears and the log names the folder and the `chown` that fixes it.

Until the first release is published on ghcr.io, `docker compose up -d` builds the image from this clone, which takes a while the first time. Update such an install with `git pull --ff-only` and `docker compose up -d --build` ([Updating a clone of the repository](docs/deployment.md#updating-a-clone-of-the-repository)).

Open your HTTPS address, enter the setup code and create the owner passkey. Then set up the VPN in Settings to turn on torrents.

On Unraid, use the template in [unraid/popcornstash.xml](unraid/popcornstash.xml) once the first release is published (it pulls the published image); until then, use Compose from a clone as above.

## Documentation

- [Installing and running PopcornStash](docs/deployment.md): HTTPS options, Compose, Unraid, first run, recovery, folders, hardware acceleration, backups.
- [Torrents and your VPN](docs/vpn.md): how torrent traffic is kept inside the tunnel, NordVPN and WireGuard/OpenVPN setup, checking that it works.
- [Chrome extension](docs/extension.md): pairing, sending links and downloads, privacy.
- [Android app](docs/android.md): what it does, building and installing it, signing in by pairing or with a passkey (`STASH_ANDROID_ASSETLINKS`), how it updates itself from GitHub, and what is not done yet.
- [Security](docs/security.md): the threat model and what each layer protects.
- [Architecture](docs/architecture.md): the API, database, folder layout and process model.

## How it is built

- One Docker image and one container. A small root supervisor sets up the VPN and kill switch, then runs the web server as your `PUID:PGID` and the torrent engine as its own user, both without privileges.
- Rust for everything on the server: axum, SQLite, webauthn-rs, librqbit, ffmpeg and libvips for media.
- The web UI is React with Meta's [Astryx](https://astryx.atmeta.com) design system and a Popcorn theme shared with Popcorn SR.
- The Chrome extension is a small Manifest V3 extension in TypeScript.
- The Android app is Kotlin and Jetpack Compose with Material 3 in the Popcorn theme ([android/README.md](android/README.md), [android/DESIGN.md](android/DESIGN.md)).

## Development

You need Rust 1.88 or newer, Node 22, a C compiler, `pkg-config` and the OpenSSL headers (Debian/Ubuntu: `apt-get install build-essential pkg-config libssl-dev`), plus ffmpeg and libvips (`apt-get install ffmpeg libvips-tools`) for the sample library, the media tests and the end-to-end tests. The Docker scripts also need Docker with buildx and compose, `curl` and `jq`.

```sh
npm --prefix web ci && npm --prefix web run build   # the server serves web/dist
cargo test --workspace
scripts/dev-fixture.sh /tmp/stash                     # a small sample library
STASH_ROOT=/tmp/stash STASH_CONFIG=/tmp/stash-config \
  STASH_PUBLIC_URL=http://localhost:8788 cargo run -p popcornstash -- serve
```

`popcornstash serve` alone runs everything except torrents.

Before you commit, run the checks on your own machine; there is no CI service:

```sh
scripts/check.sh            # fmt, clippy, tests, web, extension, Android app, Docker image and smoke test
scripts/check.sh --quick    # the same without Docker
scripts/check.sh --e2e      # also the Playwright suite (tests/e2e) and the Android app against a real server
scripts/check.sh --full     # also the VPN and full-stack tests in Docker
scripts/check.sh --audit    # also cargo audit and npm audit
```

It stops at the first failure and ends with a PASS/FAIL line per step. Behind a TLS-intercepting proxy, add `BUILD_CA=/path/to/ca.pem` (and `DOCKER_HUB_MIRROR=1` if Docker Hub is out of reach). End-to-end tests live in [tests/e2e](tests/e2e), [tests/vpn](tests/vpn) and [tests/fullstack](tests/fullstack).

To have `scripts/check.sh` run before every push to main, turn on the hook in [.githooks](.githooks/pre-push) once per clone with `git config core.hooksPath .githooks`. `git push --no-verify` skips it.

## Releasing

Releases are made from the maintainer's machine with [scripts/release.sh](scripts/release.sh). GitHub Actions is not used.

Set up once:

- Docker with buildx, and QEMU emulation so that it can build the arm64 image ([docs/deployment.md](docs/deployment.md#building-the-image-yourself)).
- The GitHub CLI, logged in with `gh auth login`. It creates the GitHub release and uploads the extension zip. Without it, `scripts/release.sh --skip-github-release VERSION` does everything else and prints the steps to create the release on github.com.
- A GitHub personal access token (classic) with the `write:packages` scope, for pushing images to ghcr.io. Fine-grained tokens do not work with ghcr.io. Keep it for releases only. The script reads it from `GHCR_TOKEN`, checks it before building, uses it for a temporary Docker login that it deletes at the end, and keeps it away from the checks and builds.
- GitHub Actions turned off for the repository (Settings, Actions, General, Disable actions), so the workflow files in old commits can never run again.
- cargo-about 0.9.2 (`cargo install --locked cargo-about --version 0.9.2 --features cli`) for `scripts/notices.sh`.
- Private vulnerability reporting turned on in the repository's Security settings, so the **Report a vulnerability** button that [docs/security.md](docs/security.md#reporting-a-vulnerability) points to exists.

After changing `Cargo.lock`, `web/package-lock.json` or the boringtun version, run `scripts/notices.sh` and commit `THIRD_PARTY_LICENSES` and `THIRD_PARTY_LICENSES-web`; `release.sh` checks they are current.

Each release:

1. `scripts/release.sh --bump 0.2.0` sets the version in Cargo, the web app, the extension and the test packages. Write the `## [0.2.0] - YYYY-MM-DD` section of [CHANGELOG.md](CHANGELOG.md), dated the day you release, then commit and push main.
2. `scripts/release.sh --dry-run 0.2.0` does everything except push, tag and create the release, and lists every problem it finds.
3. `read -rs GHCR_TOKEN && GHCR_TOKEN=$GHCR_TOKEN scripts/release.sh 0.2.0` releases. It runs `scripts/check.sh --quick --e2e --audit` and `scripts/notices.sh --check`, builds the amd64 and arm64 images from the commit (each must carry the license notices), runs the Docker smoke, VPN and full-stack tests on the image it will push, runs the Docker smoke test on the other architecture's image (arm64 on an amd64 machine) under QEMU, without the VPN and engine checks that emulation cannot run, and builds the extension zip from a clean install. Then it pushes the images to `ghcr.io/thekozugroup/popcornstash` as `0.2.0`, `0.2` and `latest`, pushes the tag `v0.2.0`, and creates the GitHub release with the CHANGELOG section and the zip. A pre-release, or a fix for an older line, leaves `latest` where it is. If a step after the first push fails, the script prints the commands that finish the release.

After the first release only: ghcr.io creates the package as private, and the script reports that nobody else can pull it. Open the package from the Packages tab of the GitHub profile, then go to Package settings, Change visibility, Public. A public package cannot be made private again. [docs/deployment.md](docs/deployment.md#images-and-versions) explains the image tags.

### The Android app

The app updates itself from GitHub releases ([docs/android.md](docs/android.md#updates)). [scripts/android-release.sh](scripts/android-release.sh) publishes one: after main is pushed with a change under `android/`, it builds the release APK from that commit, signs it, checks it and creates the GitHub release `android-N`, where N is the number of commits on main (`git rev-list --count HEAD`, so it only grows). The APK's versionCode is N and its versionName the Cargo version with the build (`0.1.0+412`). The release is marked as a pre-release, so GitHub's "latest release" stays the server's, and carries `popcornstash-android-N.apk` and its `.sha256`. Every app checks those releases once a day and offers the newest build.

```sh
scripts/android-release.sh --dry-run   # build, sign and check; print what it would publish
scripts/android-release.sh             # publish build N (skips when nothing under android/ changed)
```

It refuses a tree with uncommitted or untracked files, a branch other than main, or a main that is not pushed. A shallow clone (a cloud session's checkout often is one) is fetched whole first, because the build number counts every commit; it refuses when an `android-*` tag of the same or a higher build is on another commit, or when the tags cannot be fetched. It turns tracing (`bash -x`) off before it reads the secrets. When nothing under `android/` changed since the last `android-*` release it says so and stops; `--force` publishes anyway.

It needs these secrets in the environment. In the cloud environment they live in the environment's settings (secrets), never in the repository:

| Variable | |
| --- | --- |
| `POPCORNSTASH_KEYSTORE_B64` | the release keystore, base64 encoded (`base64 -w0 release.jks`); or `POPCORNSTASH_KEYSTORE` with its path |
| `POPCORNSTASH_KEYSTORE_PASSWORD` | the keystore's password |
| `POPCORNSTASH_KEY_ALIAS` | the key's alias |
| `POPCORNSTASH_KEY_PASSWORD` | the key's password, when it differs from the keystore's |
| `GITHUB_RELEASE_TOKEN` | a fine-grained token for this repository with Contents: read and write. Without it the script sends no Authorization header, for an environment whose network rule adds one |

Make the key once, on any machine with Docker (or with a JDK's `keytool`):

```sh
docker run --rm -it -v "$PWD:/out" -w /out eclipse-temurin:21 \
  keytool -genkeypair -keystore release.jks -alias popcornstash -keyalg RSA -keysize 4096 -validity 36500 \
  -dname "CN=PopcornStash"
docker run --rm -it -v "$PWD:/out" -w /out eclipse-temurin:21 \
  keytool -list -v -keystore release.jks -alias popcornstash | grep 'SHA256:'
```

Put the SHA-256 line's value (colons are fine) in `android/release-cert.sha256` and commit it: it is public, the script refuses to publish an APK signed with any other key, and the app uses it to tell a copy signed with another key that it cannot update itself. Store `release.jks` and its password somewhere safe outside the repository as well as in the environment's secrets. **Losing the key means every phone has to uninstall the app and install it again** (and pair again): Android installs an update only when it is signed with the same key. Leaking it lets someone else sign updates the app would accept.

After pushing to main a change under `android/`, run `scripts/android-release.sh`. Before the first build is published, every copy of the app is a debug build signed with its builder's own key; [docs/android.md](docs/android.md#switching-to-the-builds-on-github) says how to move a phone to the GitHub builds once.

## License

AGPL-3.0-or-later. Copyright (C) 2026 The PopcornStash authors. See [LICENSE](LICENSE) and [NOTICE](NOTICE); third-party licenses are in [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES) and [THIRD_PARTY_LICENSES-web](THIRD_PARTY_LICENSES-web).

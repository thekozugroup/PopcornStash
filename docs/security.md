# Security

PopcornStash keeps a family's videos and photos on their own server. This page says who it defends against, what each layer protects, how to report a problem, and what it does not protect against yet. docs/architecture.md (sections 5, 7 and 11) has the exact rules.

## Threat model

PopcornStash is built to hold up against these attackers:

- **Someone on the internet who can reach the web port.** They can send any request, but they have no passkey and no device token.
- **A malicious torrent peer or a hostile file.** Torrents and web links bring in files made by strangers: odd names, links, huge or broken media, files that pretend to be another format.
- **A malicious web page.** The page may try to drive the Chrome extension, or send requests to PopcornStash from the person's browser.
- **A malicious VPN file uploaded by a compromised owner session.** The supervisor runs the VPN as root, so the file must not be able to run code or read files.
- **A compromised torrent engine.** The engine talks to strangers all day. If it were taken over, it must not reach the library, the database, the VPN keys or the home internet connection.

These are out of scope: someone with a shell on the host or access to Docker, root on the NAS, and a signed-in family member's own device (it acts as that person).

## What each layer protects

### Signing in

- Everyone signs in with a passkey. There are no passwords to guess or leak.
- The first owner needs the setup code from the container log, so nobody else on the network can claim a new install.
- Owner recovery needs `docker exec` on the server.
- Invites are single use and expire.
- Session and device tokens are 256-bit random values. The database stores only their SHA-256. The session cookie is `HttpOnly`, `SameSite=Strict` and `Secure` on HTTPS.
- Sign-in, setup, invite lookups and device pairing are rate limited per address, with IPv6 grouped by /64. Failures are written to the audit log.

### Requests

- Every unsafe request carrying the session cookie must come from PopcornStash's own origin. Its body must be JSON, a multipart form or raw bytes, so a form on another site cannot submit one.
- Responses carry a strict Content-Security-Policy, `frame-ancestors 'none'`, `nosniff`, `no-referrer` and HSTS on HTTPS.
- Library files are served with a sandbox policy. HTML, SVG, XML and scripts are always sent as downloads, so a hostile file cannot run as the signed-in person.
- Responses carrying secrets are never cached.
- Request logs carry only the method and the path. Invite and recovery codes are replaced with `<redacted>`, and query strings are never logged.

### People and devices

- Members can watch, browse, upload, add downloads and review the Inbox.
- Only owners can change the VPN or settings, manage people and invites, empty the trash, delete items and read the audit log.
- Device tokens carry scopes. The Chrome extension can only add and list downloads. An owner can give someone else a device only with limited scopes, never full access.
- Roles are read from the database on every request, so a demotion takes effect at once.

### Files

- Every path is resolved under the stash root and refused if it leaves it. This fails closed: a link to a missing target, or a path that cannot be resolved, is refused too.
- Files are served from the opened file descriptor, so a file swapped for a link after the check cannot escape.
- The indexer and the sorter never follow links. Only `Inbox/` and `Downloads/Completed/` are moved automatically, and nothing is ever overwritten.
- The torrent engine may write only `Downloads/In Progress`, so nothing the web server writes for itself lives there:
  - Partial browser uploads are kept in `.popcornstash/uploads/` (folder 0700, files 0600).
  - A web download's `.part` file is 0600. It is only resumed, written or moved on while it is a regular file the web server owns.
- The database, the caches (thumbnails, photo renditions, live transcodes), `secrets/`, the VPN files and waiting torrents are private to the web server's user. The engine shares its group but cannot open them, even without Landlock.

### Media from strangers

- ffmpeg always reads `file:<path>`, so its protocol whitelist stays `file,crypto,data`. A playlist hidden in a "movie" cannot make it fetch URLs over the home connection.
- libvips reads photos with its untrusted loaders blocked. Only JPEG, PNG, WebP, GIF, TIFF and HEIF are allowed. The one exception is a file with a camera RAW name that also starts like an Olympus, Panasonic, Fujifilm or Canon CR3 RAW photo, because only ImageMagick reads those. A hostile file that is merely named like a RAW photo still gets the trusted loaders only.
- The image's ImageMagick policy allows only the camera RAW coders (plus JPEG and TIFF). It turns off delegate programs, filters and `@file` reads.
- Probes, thumbnails and photo renditions have time limits, and transcodes stop when nobody watches. Renditions and transcodes run a fixed number at a time.

### Web downloads

- A download can only reach the public internet. The check runs on every redirect and inside the DNS resolver, so DNS rebinding does not work. The torrent engine, loopback, private, link-local, CGNAT and other reserved ranges are refused.
- Cookies forwarded by the Chrome extension stay in memory and are never stored or logged. They never follow a redirect to another site, whether the download goes direct or through the VPN.
- Stored download sources drop user names, passwords and token-like query keys. Magnet links keep only the hash and the name.

### Torrents and the VPN

- Three independent layers keep torrent traffic in the tunnel:
  - Policy routing by user: the engine's user has no other route.
  - An nftables kill switch by user.
  - librqbit binds every socket to the tunnel.
- DNS from the engine is forced through the tunnel. When the tunnel is down, the engine has no route at all, and nothing on the VPN side can reach the web server.
- The engine runs as its own user, with no capabilities, `no_new_privs` and Landlock (best effort). Its internal API is bound to loopback and needs a random token created at each start.
- The web server reads at most 8 MiB from the engine. It passes on at most 300 characters of any engine message.
- When a torrent finishes, the root supervisor re-owns only files the engine created. It walks them with `openat` and `O_NOFOLLOW`, stays on one filesystem and checks the requesting user with `SO_PEERCRED`. The web server takes out only a single entry directly inside `In Progress`.

### The root supervisor

The supervisor reads no input from the network except DNS packets on loopback and one-line requests on its owner-only socket.

It validates VPN files again before using them:

- **WireGuard:** script hooks and unknown keys are refused, addresses become /32 or /128, and keys are passed through stdin.
- **OpenVPN:**
  - The file is parsed exactly as OpenVPN 2.6 reads it.
  - Directives that run programs, load code, read or write files, or act as a server are refused, and so is `setenv` outside a short allowlist.
  - OpenVPN reads a root-only copy of the file and never changes the main routing table. Its only hook quotes every value the server pushes.

### The Chrome extension

- The extension has no content scripts and accepts messages only from its own pages, so web pages cannot drive it.
- The device token is kept in `chrome.storage.local` and is only sent to the paired server over HTTPS (or plain HTTP on localhost).
- Cookies are forwarded only when the person turns that on and Chrome grants access for that site. The server's own cookies are never forwarded.
- Pairing needs a signed-in person to approve a code shown on the device.

### Container and supply chain

- The container drops all capabilities except the seven the supervisor needs, uses a read-only root filesystem and sets `no-new-privileges`.
- `Cargo.lock` and the npm lockfiles pin every dependency. `scripts/check.sh` and the image build install with `npm ci --ignore-scripts` and build Rust with `--locked`.
- The boringtun crates are checked against their crates.io checksums.
- There is no CI service and no third-party build actions; GitHub Actions is turned off for the repository. Images are built, tested and pushed from the maintainer's machine by `scripts/release.sh`. It refuses a working tree with uncommitted or untracked files, a commit that is not `origin/main`, a version that is already tagged, released or published, and a package that is not public. It builds the images and the extension from `git archive` of the commit, so later edits to the working tree cannot reach them.
- Published images are only as trustworthy as that machine and the release token. The token is a classic personal access token with `write:packages`, which covers every package the account owns, so it should be used for releases only. `scripts/release.sh` takes it from `GHCR_TOKEN`, checks its scopes, logs in with it in a temporary Docker configuration that is deleted at the end, and removes it (and `GH_TOKEN`) from the environment of the checks and builds, where dependency build scripts and tests run.
- Building the arm64 image needs QEMU emulation. Use the distribution's `qemu-user-static` package rather than a privileged third-party container.
- `cargo audit` and `npm audit --omit=dev` must be clean before each release; `scripts/release.sh` runs both (`scripts/check.sh --audit`). `.cargo/audit.toml` lists the advisories that were reviewed and why they do not apply.

## Reporting a vulnerability

Please report security problems privately, not in a public issue. Use GitHub's private vulnerability reporting: open the repository's **Security** tab and choose **Report a vulnerability**. If that is not available, open an issue that asks for a private contact and leave out the details.

Include:

- The PopcornStash version (`/api/health` shows it).
- How the server is set up: reverse proxy, Unraid or compose, kernel version.
- The steps to reproduce.
- What an attacker gains.

We will confirm receipt, agree on a fix and a release date with you, and credit you unless you prefer otherwise.

## Known limitations

These are known and not fixed yet.

- **Without Landlock, the library relies on file permissions.** The engine shares the web server's group so it can hand over finished torrents. Without Landlock it can read any library file that its group or everyone may read. On shares with open permissions, which Unraid uses by default, it can also change those files.
  - Check the container log for "Landlock sandbox enforced". The kernel must have Landlock enabled.
  - On kernels before Landlock ABI 8, only the engine's own threads are sandboxed. A memory-corruption exploit could still reach the others.
- **Media decoders parse hostile files as the web server's user.** These include ffmpeg, libvips, libheif, librsvg through ffmpeg, and ImageMagick with libraw for Olympus, Panasonic, Fujifilm and Canon CR3 RAW photos. A memory-safety bug in one of them would give an attacker the web server's access.
  - Sandboxing the decoders is planned.
- **The built-in web server has no connection limits.** Someone who opens many slow connections can tire it out. On the internet, put PopcornStash behind a reverse proxy, Tailscale or a tunnel that limits connections.
- **Sign-in ceremonies share one pool of 10,000.** An attacker spread over many addresses can fill it for a few minutes and block sign-in until it drains.
- **Device pairing can be phished.** Approving a code that someone else sent you gives them a device on your account. An Android device gets full access, including adding passkeys. Only approve codes shown on a device in your own hands. The approval page shows the device type and the address it asked from.
- **Members are trusted family.** They see the whole library and every download. They can pause or remove anyone's download, and they can organize files in the owner's own folders.
- **Sessions outlive passkeys.** Removing a passkey, or recovering the owner account, does not end sessions that are already signed in. End them under Signed-in browsers in Settings → Security.
- **`STASH_TRUST_PROXY=true` trusts `X-Forwarded-For`.** Turn it on only when the port can be reached only through your proxy. Otherwise an attacker can pick a fresh rate-limit budget for every request.
- **Web downloads through the VPN pass through the engine.** While the VPN is connected, web downloads go through the engine's proxy by default (`http_via_vpn`). HTTPS is checked end to end, so a compromised engine sees only the host name. It can read and change a plain `http://` download and any cookies the extension forwards with it, as anyone else on that unencrypted path can.
- **The VPN page reports what the engine says.** A compromised engine could show "Protected" and a made-up public address. The root kill switch, not the page, keeps torrent traffic in the tunnel.
- **A compromised owner session can point the tunnel anywhere.** A hostile VPN file can choose the server and the DNS servers, and send WireGuard or OpenVPN handshakes to any address, including the LAN. It cannot run code, read files or route anything but torrent traffic.
- **Older installs keep their `Downloads/` mode.** Versions before this audit created `Downloads/` group-writable (0775). New installs get 0755, and existing folders are left as the owner set them. Run `chmod 755` on it if nothing else needs group write there.
- **Some dependencies have open advisories.**
  - `ringbuf` 0.4 (RUSTSEC-2026-0293) is used by the uTP code only for bytes, which cannot trigger the bug.
  - `rsa` (RUSTSEC-2023-0071) is in `Cargo.lock` but never built.
  - `rustls-pemfile`, used by `axum-server` for built-in TLS, is unmaintained.
  - Base images are pinned by tag, not digest, and Debian packages float within trixie.

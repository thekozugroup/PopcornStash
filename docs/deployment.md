# Installing PopcornStash

PopcornStash is one Docker container. It needs two folders, one HTTPS address, and a few container settings that the provided `compose.yaml` and Unraid template already include.

## What you need

- A computer that runs Docker: a NAS, an Unraid server or a Linux PC. Both Intel/AMD (amd64) and ARM (arm64) machines are supported.
- A folder for your library, for example `/mnt/user/PopcornStash` on Unraid. PopcornStash creates its folders inside it.
- A folder for app data (database, thumbnails, settings, VPN files), for example `/mnt/user/appdata/popcornstash`. Fast storage such as an SSD or the Unraid cache pool works best.
- An HTTPS address with a domain name, such as `https://stash.example.com`. Passkeys do not work without it. The next section explains why and shows four ways to get one.
- For torrents: the `NET_ADMIN` capability and the `/dev/net/tun` device. Both are already set in `compose.yaml` and in the Unraid template. You also need a VPN account; see [Torrents and your VPN](vpn.md).

## Why passkeys need HTTPS on a domain

Everyone signs in to PopcornStash with a passkey: a key stored on your phone, your computer or your password manager, unlocked with your face, fingerprint or screen lock. There are no passwords.

Browsers only create and use passkeys on secure pages: an `https://` address with a domain name, or `http://localhost` on the same computer. An address like `http://192.168.1.20:8788` has neither, so the browser refuses. PopcornStash says so on the sign-in page.

A passkey also belongs to one domain name. `STASH_PUBLIC_URL` must be exactly the address people open, and passkeys made for `stash.example.com` will not work on another name. Choose a name you plan to keep. If you ever change it, the owner recovers access with a new passkey (see [Recovery](#recovery)) and invites everyone again.

The name does not have to be public on the internet. A name that only resolves inside your home network or inside Tailscale works, as long as it has a valid certificate.

### Option 1: Tailscale Serve (simplest for private use)

Tailscale gives every device a private network and can issue certificates for your machine's name.

1. Install Tailscale on the server (on Unraid, use the Tailscale plugin) and on every phone and computer that will use PopcornStash.
2. In the Tailscale admin console, open **DNS**, turn on **MagicDNS**, then turn on **HTTPS Certificates**.
3. On the server, run `tailscale serve --bg 8788`. Tailscale prints an address such as `https://nas.tail1234.ts.net`.
4. Set `STASH_PUBLIC_URL=https://nas.tail1234.ts.net` and `STASH_TRUST_PROXY=true`.

Family members need Tailscale on their devices too, or you can share the server with their Tailscale accounts.

### Option 2: Caddy

Caddy gets and renews certificates by itself.

```caddyfile
stash.example.com {
	reverse_proxy 127.0.0.1:8788
}
```

Point the name's DNS record at the computer running Caddy. For a name that only works at home, use Caddy's DNS challenge with your DNS provider and a local DNS record (for example in your router or Pi-hole).

### Option 3: Nginx Proxy Manager or SWAG

On Unraid these are the most common choices.

1. Add a proxy host for `stash.example.com` that forwards to `http://SERVER-IP:8788`.
2. Request a Let's Encrypt certificate for it and force HTTPS.
3. Add these lines to the host's custom Nginx configuration. Without them, uploads larger than 1 MB fail and live updates stall:

   ```nginx
   client_max_body_size 0;
   proxy_request_buffering off;
   proxy_buffering off;
   proxy_read_timeout 1h;
   ```

With SWAG, copy one of the included subdomain samples, set the upstream app to your server's IP and port 8788, and add the same lines.

### Option 4: Cloudflare Tunnel

A tunnel publishes PopcornStash without opening ports on your router.

1. In Cloudflare Zero Trust, create a tunnel and run its connector (`cloudflared`) on the server, for example as a second container.
2. Add a public hostname, `stash.example.com`, with the service `http://SERVER-IP:8788`.
3. Set `STASH_PUBLIC_URL=https://stash.example.com` and `STASH_TRUST_PROXY=true`.

All video then passes through Cloudflare. Check that your Cloudflare plan allows streaming the amount of video you expect.

### Built-in HTTPS

If you already have a certificate for your name (for example from `acme.sh` or your router), PopcornStash can serve HTTPS itself. Put the PEM files in the app data folder and add this to the `environment` section of `compose.yaml`:

```yaml
      STASH_TLS_CERT: /config/tls/fullchain.pem
      STASH_TLS_KEY: /config/tls/privkey.pem
```

Then set `STASH_PUBLIC_URL=https://stash.example.com:8788`. Restart the container after you renew the certificate.

### Behind a proxy

When a proxy sits in front, set `STASH_TRUST_PROXY=true` so sign-in limits and the activity log see each visitor's real address. Then make sure people can only reach PopcornStash through the proxy. If the proxy runs on the same computer, publish the port on the loopback address only by setting `STASH_PORT=127.0.0.1:8788` in `.env`.

## Quick start with Docker Compose

1. Create the two folders and give them to the user who will own your files. Use that user's numbers as `PUID` and `PGID` (`id -u` and `id -g`):

   ```sh
   sudo mkdir -p /srv/popcornstash/appdata /srv/media/PopcornStash
   sudo chown 1000:1000 /srv/popcornstash/appdata /srv/media/PopcornStash
   ```

2. Download `compose.yaml` and `.env.example` from this repository into an empty folder, and copy `.env.example` to `.env`.
3. Edit `.env`. Set `STASH_PUBLIC_URL`, `APPDATA_PATH`, `STASH_PATH`, `PUID` and `PGID`. Every setting is explained in the file.
4. Start PopcornStash:

   ```sh
   docker compose up -d
   ```

5. Point your HTTPS address at port 8788 (see above), then continue with [First run](#first-run).

To build the image yourself instead of downloading it, run `docker compose build` before `docker compose up -d` (see [Building the image yourself](#building-the-image-yourself)).

### Without Compose

The same settings as a single `docker run` command, for Portainer, Synology Container Manager or the command line:

```sh
docker run -d --name popcornstash --restart unless-stopped \
  -p 8788:8788 \
  -e PUID=1000 -e PGID=1000 \
  -e STASH_PUBLIC_URL=https://stash.example.com \
  -v /srv/popcornstash/appdata:/config \
  -v /srv/media/PopcornStash:/stash \
  --cap-drop=ALL --cap-add=NET_ADMIN --cap-add=CHOWN --cap-add=DAC_OVERRIDE \
  --cap-add=FOWNER --cap-add=SETUID --cap-add=SETGID --cap-add=KILL \
  --device=/dev/net/tun --sysctl net.ipv4.conf.all.rp_filter=2 \
  --read-only --tmpfs /tmp --tmpfs /run:exec,mode=755 \
  --security-opt=no-new-privileges:true \
  --stop-timeout 30 \
  ghcr.io/thekozugroup/popcornstash:latest
```

## Unraid

PopcornStash uses one template: `unraid/popcornstash.xml`.

1. Decide how you will reach PopcornStash over HTTPS: the Tailscale plugin, Nginx Proxy Manager or SWAG, or a Cloudflare Tunnel (see above).
2. Create a share for your library, for example `PopcornStash`. You can also use an existing media share.
3. Open a terminal on the Unraid server (the **>_** icon) and install the template:

   ```sh
   wget -O /boot/config/plugins/dockerMan/templates-user/my-PopcornStash.xml \
     https://raw.githubusercontent.com/thekozugroup/PopcornStash/main/unraid/popcornstash.xml
   ```

4. Go to **Docker**, choose **Add Container**, and pick **PopcornStash** from the **Template** list (under user templates).
5. Fill in **Public address** with your HTTPS address. Check **App data** and **Library**. Keep **PUID** 99 and **PGID** 100 unless your share uses other owners.
6. Choose **Apply**. Unraid downloads the image and starts the container.
7. Point your proxy, Tailscale Serve or tunnel at `http://UNRAID-IP:8788`, then continue with [First run](#first-run).

The template already adds what torrents need in **Extra Parameters** (visible in the advanced view): `NET_ADMIN` and the few other permissions the supervisor uses, the `/dev/net/tun` device, a network setting for the tunnel, a read-only system and no extra privileges. Do not switch the container to **Privileged**; it does not need it.

## First run

1. Find the setup code. On Unraid, click the PopcornStash icon and choose **Logs**. With Compose, run:

   ```sh
   docker compose logs popcornstash | grep -A 1 "setup code"
   ```

   If the log has scrolled away, run `docker exec popcornstash popcornstash admin setup-code`. It prints a new code, and the one in the log stops working.
2. Open your HTTPS address. PopcornStash asks for the setup code and your name.
3. Create the owner passkey when your browser asks. Use your phone, your computer or your password manager.

The setup code is valid for 24 hours and changes every time the container restarts, until the owner is created. Nobody else on your network can claim a new install without it.

## Inviting family

1. Open **Settings**, then **People**, and create an invite. Choose **Member** for family members, or **Owner** for someone who should manage PopcornStash with you.
2. Send the link. It works once and expires after 72 hours.
3. The person opens the link, enters their name and creates a passkey on their device.

Everyone can add more passkeys later (for example a phone and a laptop) under **Settings**, then **Security**. Members can watch, browse, upload, add downloads and review the Inbox. Only owners can change the VPN and settings, manage people, delete items and empty the trash.

## Recovery

If the owner loses every passkey, run this on the server:

```sh
docker exec popcornstash popcornstash admin reset-owner
```

It prints a one-time link, valid for 30 minutes, that lets the owner add a new passkey. On Unraid, run it in the server terminal or use the container's **Console**, leaving out the `docker exec popcornstash` part.

## Your folders

PopcornStash creates these folders inside the library folder on its first start:

| Folder | What goes there |
|---|---|
| `Movies/` | `Movies/Title (Year)/Title (Year).mkv` |
| `Shows/` | `Shows/Show Name/Season 01/Show Name - S01E02.mkv` |
| `Home Videos/` | `Home Videos/2024/06/original-name.mov` |
| `Photos/` | `Photos/2024/06/original-name.heic` |
| `Other/` | Anything that is not a video or photo |
| `Inbox/` | Drop zone: anything you copy here is sorted |
| `Downloads/In Progress/` | Torrents and web downloads while they arrive |
| `Downloads/Completed/` | Finished downloads waiting to be sorted |
| `.popcornstash/` | The trash (kept 30 days), uploads while they arrive (private to `PUID`) and a marker file |

How files move:

- PopcornStash only moves files by itself out of `Inbox/` and `Downloads/Completed/`.
- Everything else is shown where it is. Files you already keep in `Movies/`, `Shows/` and the other folders are never moved or renamed unless you choose **Organize** on that item.
- When PopcornStash is not sure where a file belongs, it leaves the file in place and asks you in the Inbox page.
- Nothing is overwritten. If a name is taken, the new file gets ` (2)`, ` (3)` and so on.
- Subtitles and other files with the same name (`.srt`, `.ass`, `.nfo`, `.xmp` and similar) move together with their video or photo.
- Files still being copied are left alone until their size has not changed for 10 seconds, so slow network copies are safe.
- Deleting an item moves it to the trash folder first. It is removed for good after 30 days, or when an owner empties the trash.

## File ownership

Every file PopcornStash creates in the library and app data belongs to `PUID:PGID`. On Unraid that is 99:100 (nobody:users), the same as your other shares. Pick the numbers of the user that owns your library folder.

Torrents are downloaded by a separate user, the engine user (`STASH_TORRENT_UID`, 6881 by default), so torrent traffic can be kept inside the VPN. That user only ever writes into `Downloads/In Progress/` and shares your `PGID` group there. When a torrent finishes, PopcornStash hands its files to `PUID:PGID` before moving them, so your library never contains files owned by the engine user. Because the engine shares the group, the database, the thumbnail and transcode caches, the VPN files and uploads in progress are kept private to `PUID` (files 0600, folders 0700). Where the kernel supports Landlock the engine cannot read the rest of the library either; check the log for "Landlock sandbox enforced" (see [Security](security.md)). If 6881 is already used by another account on your server, set `STASH_TORRENT_UID` to an unused number other than 0 and `PUID`.

The container starts as root only so it can set up the VPN and the kill switch. The web app and the torrent engine run as their own users without any special permissions.

## Hardware acceleration

Most browsers play most videos directly. PopcornStash only converts a video when the device cannot play it, and by default it uses the processor. A graphics chip can do this faster.

- **Intel or AMD (VAAPI)** and **Intel Quick Sync (QSV)**: add the device line in `compose.yaml` (`- /dev/dri:/dev/dri`) and set `STASH_HWACCEL=vaapi` or `qsv`. On Unraid, add a **Device** to the template with the value `/dev/dri` and set **Hardware encoding**.
- **NVIDIA (NVENC)**: install the NVIDIA Container Toolkit (on Unraid, the Nvidia Driver plugin). In `compose.yaml`, add `runtime: nvidia` and the environment variables `NVIDIA_VISIBLE_DEVICES=all` and `NVIDIA_DRIVER_CAPABILITIES=compute,video,utility`, then set `STASH_HWACCEL=nvenc`. On Unraid, add `--runtime=nvidia` to **Extra Parameters** and the same two variables.

The web app runs as `PUID:PGID`, so that user or group must be allowed to use the graphics device. Check with `ls -ln /dev/dri`: `renderD128` must be readable and writable by your `PGID` or by everyone. **Settings**, then **System**, shows which hardware encoders PopcornStash found.

## Backups

- **App data** holds your people, passkeys, settings, watch progress and VPN files. Back it up regularly. Stop the container first (`docker compose stop`), or copy the database together with its `-wal` and `-shm` files. The `cache` folder inside it can be skipped; thumbnails are rebuilt.
- **Library** is ordinary folders and files. Back it up like any other share.

## Images and versions

The image is published at `ghcr.io/thekozugroup/popcornstash` for Intel/AMD (`linux/amd64`) and ARM (`linux/arm64`) machines under one name; Docker downloads the right one by itself. Each release is built, tested and published from the maintainer's computer with `scripts/release.sh` (there is no CI service), and gets these tags:

| Tag | Points to | Use it to |
|---|---|---|
| `latest` | the newest release | always get the newest release; `compose.yaml` and the Unraid template use it |
| `0.1` (major.minor) | the newest `0.1.x` release | get fixes, but no new minor version |
| `0.1.0` (exact version) | that release, never moved | stay on one version, or go back to it |
| `0.1.0-amd64`, `0.1.0-arm64` | one architecture of that release | pin a single architecture (rarely needed) |

A pre-release such as `0.2.0-rc.1` gets only its own tag; `latest` and `0.2` stay on the last full release. [CHANGELOG.md](../CHANGELOG.md) and the GitHub release of each version say what changed; the release also carries the Chrome extension (`popcornstash-extension.zip`).

To stay on a version, replace `latest` in `compose.yaml` (`image: ghcr.io/thekozugroup/popcornstash:0.1`) or in the Unraid template's **Repository** field.

### Building the image yourself

`docker compose build`, or `docker build -t popcornstash:dev .` in a clone of the repository, builds the image for your own machine. To build for the other architecture, add `--platform`, for example on an amd64 PC:

```sh
docker build --platform linux/arm64 -t popcornstash:arm64 .
```

The Rust programs are cross-compiled at full speed (about 7 minutes for the first arm64 build on a 4-core PC). The last stage installs Debian packages for arm64, though, so Docker must be able to run arm64 programs through QEMU, and that stage is slow under emulation (about 18 minutes the first time; Docker caches it afterwards). If that build stops at its first `RUN` step with `exec /bin/sh: exec format error`, turn on emulation:

```sh
sudo apt-get install qemu-user-static binfmt-support
# Only if /proc/sys/fs/binfmt_misc/qemu-aarch64 does not exist now (machines
# without systemd, such as containers and some VMs; repeat after each reboot):
sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc   # if it is not mounted yet
sudo sh -c 'cat /usr/lib/binfmt.d/qemu-aarch64.conf > /proc/sys/fs/binfmt_misc/register'
```

`docker run --privileged --rm tonistiigi/binfmt --install arm64` does the same in one command. `docker run --rm --platform linux/arm64 debian:trixie-slim uname -m` prints `aarch64` once it works. On Docker Desktop for Mac and Windows emulation is already on. An arm64 image started on an amd64 machine this way runs, but QEMU cannot emulate the netlink calls that the VPN kill switch makes, so torrents stay off there.

## Upgrading

Read the release notes of the new version first ([CHANGELOG.md](../CHANGELOG.md)). Then, with Compose:

```sh
docker compose pull
docker compose up -d
```

On Unraid, use **Check for Updates** on the Docker page, then **Apply Update**. If you pinned a version, change the tag first. Your folders and settings are kept. The database is upgraded automatically on start, so back up app data before upgrading (see [Backups](#backups)).

To go back to an earlier version, set its exact tag (for example `0.1.0`), restore the app data backup you took before the upgrade, and start the container again. An older version refuses to start on a database that a newer one has already upgraded.

Update the Chrome extension from the same release: unzip the new `popcornstash-extension.zip` over the old folder and reload it in `chrome://extensions` (see [Chrome extension](extension.md)).

## Running without torrents

Torrents need `NET_ADMIN` and `/dev/net/tun`. If you remove them, PopcornStash still runs: the library, playback, uploads, web downloads and the Inbox all work, and torrent links are refused with a plain explanation. The log says "Torrents need the container to have NET_ADMIN and /dev/net/tun."

Starting the container as a non-root user (`--user` or `user:` in Compose) has the same effect. In that case the folders must already belong to that user.

## Settings reference

| Variable | Default | Meaning |
|---|---|---|
| `STASH_PUBLIC_URL` | none (required) | The HTTPS address people open. Passkeys need it |
| `PUID`, `PGID` | 99, 100 (compose file: 1000, 1000) | Owner of every file PopcornStash writes |
| `STASH_TRUST_PROXY` | `false` | `true` behind a reverse proxy or tunnel |
| `STASH_HWACCEL` | `none` | `none`, `vaapi`, `qsv` or `nvenc` |
| `STASH_TORRENT_UID` | `6881` | User the torrent engine runs as |
| `STASH_EXTRA_ORIGINS` | none | Extra allowed addresses on the same domain, comma separated |
| `STASH_TLS_CERT`, `STASH_TLS_KEY` | none | Certificate and key for built-in HTTPS |
| `STASH_LOG` | `info` | Log detail |
| `STASH_ALLOW_NO_VPN` | `false` | See [Running without a VPN](vpn.md#running-without-a-vpn) |
| `STASH_ROOT`, `STASH_CONFIG` | `/stash`, `/config` | Folders inside the container. Leave them alone |
| `STASH_BIND` | `0.0.0.0:8788` | Listen address inside the container |

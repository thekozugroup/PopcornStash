# Full-stack end-to-end test

`scripts/fullstack-e2e.sh` proves the main promise of PopcornStash in one run,
in Docker and through the real web UI: **a torrent added on the Downloads page
downloads only through the VPN and lands sorted in the library, owned by the
owner's user, ready to play.** Then the VPN peer goes away and comes back while
the web app keeps working. No internet access is needed; everything runs on one
private Docker network.

```
                      private Docker network 172.31.67.0/24 (SUBNET_PREFIX)
 ┌───────────────────────┬────────────────────┬──────────────────┬─────────────────────┐
 │ main  .30             │ wgserver  .10      │ tracker  .40     │ seeder  .50         │
 │ PopcornStash, run     │ tests/vpn/         │ tracker.py       │ popcornstash        │
 │ like compose.yaml     │  wgserver.sh       │  logs the source │  torrent-engine     │
 │  engine ── psw0 ──────┼─▶ wg0 ─ masquerade ┼─▶ of every       │  --allow-no-vpn     │
 │  (kill switch)        │  10.66.0.1/24      │  announce        │  seeds the film at  │
 │  web UI on            │                    │                  │  16 KiB/s           │
 │  127.0.0.1:8788       │ echo  .20 (tests/vpn/echo.sh): the VPN's DNS server         │
 └───────────────────────┴─────────────────────────────────────────────────────────────┘
        ▲ Playwright (Chromium, virtual passkey authenticator) on http://localhost:8788
```

- **main** is the PopcornStash image started with the settings read from
  `compose.yaml` (`cap_drop: ALL` plus the seven capabilities, `/dev/net/tun`,
  `rp_filter=2`, read-only root, tmpfs `/tmp` and `/run:exec`,
  `no-new-privileges`, the compose environment with `PUID`/`PGID` 1000 and
  `STASH_PUBLIC_URL=http://localhost:8788`), port 8788 published on 127.0.0.1,
  an empty stash folder owned by 1000:1000 and an empty app data folder.
- **wgserver** is the VPN provider: one WireGuard peer (boringtun on hosts
  without the kernel module) that masquerades tunnel traffic, so anything that
  came through the VPN reaches the others from wgserver's address.
- **tracker** (`tracker.py`, Python standard library, run in the PopcornStash
  image) answers announces with compact peer lists and writes one JSON line per
  announce with the address the request really came from.
- **seeder** is the PopcornStash image running only `popcornstash
  torrent-engine --allow-no-vpn` as root, with its own state and download
  folders. It seeds `Lantern.Bay.2018.720p.WEB.webm`: the 45 second VP9/Opus clip
  from `scripts/dev-fixture.sh`, remuxed to WebM under a release name, as a
  single-file torrent with 16 KiB pieces made by `mktorrent.py`. Its upload limit
  is 16 KiB/s, so the download takes about 40 seconds and its progress can be
  watched.
- **echo** (`tests/vpn/echo.sh`) is the DNS server named in the WireGuard file.
  The tracker and seeder also use it as their resolver, so the seeder's DHT
  never reaches the real internet.

The main container has an ordinary route to the seeder and the tracker on the
shared network. That is deliberate: only the policy routing and kill switch
keep the engine's traffic in the tunnel, so a leak would show up in the
tracker's log and the seeder's connections as the container's own address.

## Run

```sh
npm --prefix tests/fullstack ci     # once; @playwright/test 1.56.1 like tests/e2e
scripts/fullstack-e2e.sh            # builds the image first (base images from Docker Hub)

# Where Docker Hub is rate limited, or with an image already built:
DOCKER_HUB_MIRROR=1 scripts/fullstack-e2e.sh
SKIP_BUILD=1 IMAGE=popcornstash:dev DOCKER_HUB_MIRROR=1 scripts/fullstack-e2e.sh
```

Needs `docker` (with `docker compose`), `curl`, `jq` and Node on the host, a
kernel with `/dev/net/tun`, and Playwright's Chromium (`PLAYWRIGHT_BROWSERS_PATH`,
default `/opt/pw-browsers`; elsewhere `npx playwright install chromium` once).
Port 8788 on 127.0.0.1 must be free (`STASH_PORT` picks another; the browser
then opens `http://localhost:<port>`, which is also `STASH_PUBLIC_URL`). The run
takes about six minutes, most of it waiting for WireGuard to notice the peer is
gone. `KEEP=1` leaves the containers and folders in place; `FS_CHROMIUM` points
Playwright at another Chromium-based browser.

## What it checks

Before the browser (`scripts/fullstack-e2e.sh`):

| Check | How |
|---|---|
| Seeder | After priming (below) librqbit re-checks every piece and reports `seeding`; it announces itself to the tracker with nothing left |
| Start | PopcornStash answers on `http://localhost:8788`; the first-run setup code is read from the container log |

In the browser (`specs/fullstack.spec.ts`, one journey, in order):

| Step | Check |
|---|---|
| First run | `/` sends a fresh server to `/setup`; the code from the log and a name create the owner's passkey (virtual authenticator from `tests/e2e/helpers/passkeys.ts`); Home opens; the sidebar says torrents are blocked |
| VPN | Settings → VPN → "Use a WireGuard or OpenVPN file instead" → Choose file (the system file picker) → "Use this file"; the status card says **Protected** with the kill switch on, the sidebar tag says Protected, and the engine's internal `/health` reports `vpn.state = connected` on `psw0` and `engine.state = running` |
| Download | Downloads → paste the magnet (`dn` and `tr` set, no peers in it) → Add; the row shows real progress (the bar takes at least three values between 15 % and 85 %, "Downloading", speed, peers); mid-download the seeder's only peer connection comes from **wgserver's address**, never the container's; the row moves to Finished with **"Sorted into Movies: Lantern Bay (2018)"** |
| Library | Movies shows "Lantern Bay" with a thumbnail that loaded; Home lists it under Recently added; its page plays it directly (`mode: direct`, VP9/Opus WebM) and `currentTime` passes 1.5 s |
| Proof | Every announce for the torrent that did not come from the seeder came from **wgserver's address**, and none from the PopcornStash container's own address, including announces made mid-download; `Movies/Lantern Bay (2018)/Lantern Bay (2018).webm` and its folder are owned by **1000:1000** (adopted from the engine user) and the file has the torrent's size; `Downloads/In Progress` and `Downloads/Completed` are empty again; nothing in the stash belongs to the engine user (6881); the sidebar tag says Protected |
| VPN down | `docker stop wgserver`: the engine's `/health` stops saying `connected` and the engine is `blocked`; Settings no longer says Protected; Movies opens and the film plays again |
| Recovery | `docker start wgserver`: without touching PopcornStash the engine reports `connected` and `running` again, and Settings and the sidebar say Protected |

After the browser: the seeder uploaded the whole film, and the PopcornStash log
holds neither the WireGuard private key nor the internal token.

## Evidence

Saved in `evidence/fullstack/` (git-ignored):

- Screenshots at 1440×900: `vpn-connected-1440-light.png`,
  `vpn-connected-1440-dark.png`, `downloads-mid-1440-light.png`,
  `downloads-finished-1440-light.png`, `home-1440-light.png`,
  `movies-1440-light.png`, `vpn-down-1440-light.png`.
- `tracker-announces.jsonl` (the torrent's announces with the source address
  the tracker saw), `seeder-peers-mid-download.txt`, `progress-seen.txt`,
  `movie-owners.txt`, `engine-health-connected.json`,
  `engine-health-vpn-down.json` (with the seconds it took to notice),
  `torrent.json`, `seeder-torrents.json`, `main-settings.txt` (the `docker run`
  settings taken from compose.yaml) and every container's log.

## Notes and limits

- **Seeder priming.** The engine never reuses an existing folder for a new
  torrent (it picks `Name (2)` instead), so a file cannot simply be placed
  before adding the torrent. The script adds the torrent (librqbit creates the
  empty folder and file), stops the seeder, puts the film in place, deletes
  librqbit's fast-resume bitfield and starts the seeder again; librqbit then
  checks every piece and seeds.
- **Noticing a dead peer takes up to three minutes.** WireGuard has no
  disconnect message: a peer counts as gone once the last handshake is 180 s
  old. Torrents cannot leak in the meantime; the kill switch drops the engine's
  packets whatever the status says (`scripts/vpn-e2e.sh` checks that within
  seconds of the stop).
- **The public-IP proof** (`api.ipify.org`, NordVPN insights) is unit-tested
  in the engine against local fakes, but not checked here, so the VPN page
  shows no public address here.
- **H.264.** Playwright's Chromium has no H.264 decoder, which is why the film is
  VP9/Opus; it is played for real.
- On hosts without the WireGuard kernel module both wgserver and PopcornStash
  use boringtun.

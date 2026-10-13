# End-to-end tests

Playwright drives Chromium against the real `popcornstash serve` and a freshly built sample stash. Passkeys use Chrome's virtual WebAuthn authenticator (DevTools protocol: `ctap2`, `internal`, resident keys, user verification), so no hardware or mocks are involved.

## Run

From the repository root:

```sh
npm --prefix web run build          # the debug server serves web/dist from disk
cargo build -p popcornstash         # target/debug/popcornstash
npm --prefix tests/e2e ci           # @playwright/test 1.56.1 (Chromium revision 1194)
npm --prefix tests/e2e test         # or: cd tests/e2e && npx playwright test
```

Needs `ffmpeg`/`ffprobe` on the PATH (exiftool and vips are used when present). The suite uses Playwright's Chromium from `PLAYWRIGHT_BROWSERS_PATH`, which defaults to `/opt/pw-browsers`; elsewhere install it once with `npx playwright install chromium`.

Useful variables:

| Variable | Default | Meaning |
|---|---|---|
| `E2E_PORT` | `8788` | Port for the test server (`http://localhost:<port>`; passkeys need `localhost` or HTTPS) |
| `E2E_SERVER_BIN` | `target/debug/popcornstash` | Server binary to test |
| `E2E_CHROMIUM` | Playwright's Chromium | Another Chromium-based browser binary, for example Chrome with H.264 (see Playback) |
| `E2E_KEEP` | unset | `1` keeps the temporary stash, config and `server.log` after the run |
| `E2E_LOG` | `info,tower_http=warn` | Server log filter (`STASH_LOG`) |

Results land in `test-results/e2e/` (traces and screenshots of failures) and `playwright-report/` at the repository root; both are git-ignored. `npm --prefix tests/e2e run check` type-checks the suite.

## What a run does

`global-setup.ts` makes a temporary folder, builds the sample stash with `scripts/dev-fixture.sh`, makes three small clips for the upload and Inbox tests, and starts `popcornstash serve` with `STASH_ROOT`, `STASH_CONFIG`, `STASH_PUBLIC_URL=http://localhost:8788` and `STASH_BIND=127.0.0.1:8788`. No supervisor runs, so torrents are off. The first-run setup code is read from the server log. Teardown stops the server and deletes the folder.

The `first-run` project registers the owner and saves the owner's cookie and passkey; every other spec depends on it. Specs share one server and run in file order on one worker:

| Spec | Covers |
|---|---|
| `first-run` | `setup_required`, `/setup` with the code from the log, owner passkey, lands on Home |
| `01-auth` | Sign out and back in with only the passkey; a browser without it stays at `/login`; owner recovery with `popcornstash admin reset-owner` |
| `02-library` | Home counts and Recently added, Movies, Shows and seasons, Home Videos dates, Photos viewer with arrow keys, Other; no sideways scrolling at phone width |
| `03-sorting` | Startup scan moved Inbox files on disk; the owner's own folder is left alone; ambiguous file in Inbox, Edit and accept moves it; a file copied into Inbox while running is sorted and Movies updates live; Organize moves an owner-folder file only on request |
| `04-playback` | Direct play with range requests; HEVC/AC-3 MKV over HLS; segment alignment and resume; progress saving and Continue watching |
| `05-uploads` | Upload from Downloads and by dropping on the page; progress, `Downloads/Completed`, sorting, library |
| `06-links-and-torrents` | Private-address links are refused plainly and never fetched; magnets are refused while torrents are off; Settings say so; the PWA share target |
| `07-people-and-devices` | Invite link and member passkey; members cannot reach owner settings; device pairing, token delivered once, scopes |
| `08-security` | Cookie writes need a matching Origin; CSP and other headers; sandboxed library files; cookie flags |

## Sample stash

`scripts/dev-fixture.sh <dir>` builds the stash on its own too (for development: point `STASH_ROOT` at it). Clips are 320x180 and 3 to 8 seconds long, except one 45 second VP9 clip that is long enough for Continue watching. Titles are invented. The Inbox holds a release-named movie, an `SxxEyy` episode and a phone video (all sorted at startup) and an ambiguous `clip_final_v2.mkv` (waits for review). `Family 2019/beach.mp4` sits in an owner folder at the top level, which PopcornStash indexes in place and never moves on its own. Everything is back-dated an hour so the startup scan treats the Inbox as stable. HEIC is added only when the local vips can write it.

## Playback and H.264

Playwright's Chromium is an open-source build without H.264 or AAC decoders, and HLS output is always H.264/AAC. With that browser the playback specs check everything up to decoding: the server's direct-or-HLS decision, the original file served with range requests, the playlist and segments the player fetched, ffprobe reading the served segments as H.264/AAC with timestamps that line up, and the player's plain "could not be played in this browser" message. A VP9/Opus MP4 in the sample stash plays for real (its `currentTime` advances) and drives the progress and Continue watching checks. Point `E2E_CHROMIUM` at a Chrome build with proprietary codecs to also assert that the H.264 file and the HLS stream advance `currentTime`.

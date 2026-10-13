# PopcornStash for Chrome

The PopcornStash extension sends links, videos, images, magnet links and (if you want) every browser download to your own PopcornStash server. It talks to one server only, the one you pair it with, using a device key that the server issues after someone signed in to PopcornStash approves it.

It is a Manifest V3 extension for Chrome 116 or newer. Other Chromium browsers (Edge, Brave, Vivaldi) load it the same way.

## Install

The extension is not in the Chrome Web Store. Each PopcornStash release ships `popcornstash-extension.zip`.

1. Download `popcornstash-extension.zip` from the [latest release](https://github.com/thekozugroup/PopcornStash/releases/latest) and unzip it into a folder you will keep (Chrome loads the extension from that folder every time it starts).
2. Open `chrome://extensions` and turn on **Developer mode** (top right).
3. Click **Load unpacked** and choose the unzipped folder (the one that contains `manifest.json`).
4. The options page opens by itself. Pin the extension from the puzzle menu if you want the toolbar button visible.

To update, unzip the new release over the same folder and click the reload arrow on the extension's card in `chrome://extensions`. Your pairing and options are kept.

Build it yourself from this repository:

```sh
npm --prefix extension ci
npm --prefix extension run zip     # builds extension/dist and extension/popcornstash-extension.zip
```

## Pair with your server

1. In the options page, type the address you open PopcornStash at, for example `https://stash.example.com`, and click **Connect**.
   - Only `https://` addresses are accepted. Plain `http://` works only for `localhost` and `127.0.0.1` (local testing), the same rule PopcornStash itself uses for passkeys.
   - A path is ignored (`https://stash.example.com/downloads` connects to `https://stash.example.com`). Addresses with a user name or password in them are refused.
2. Chrome asks to let the extension reach that address. This is the only site the extension needs for sending links. Choose **Allow**.
3. The page shows a code such as `KQ7M-2XWD`. Click **Open PopcornStash to approve**: PopcornStash opens its pairing page with the code filled in. Check that the codes match and approve. You can also type the code in **Settings, Devices**.
4. The options page notices the approval within a few seconds and shows **Chrome is connected to PopcornStash**.

The device appears in PopcornStash as "Chrome on macOS" (or Windows, Linux, ChromeOS). A pairing code works for 10 minutes. The extension checks for approval every 3 seconds, or less often when the server asks it to (the real server suggests 6 seconds to stay inside its pairing rate limit, and the extension backs off further if it is ever rate limited).

The key the extension receives can only **add downloads** and **see their progress** (scopes `downloads:create` and `downloads:read`). It cannot browse the library, change settings or see other people.

## Send things to PopcornStash

- **Right-click menu:** "Send link to PopcornStash" on links (including `magnet:` links), "Send video to PopcornStash", "Send audio to PopcornStash" and "Send image to PopcornStash" on media. A notification confirms "Sent to PopcornStash" with the file name, and clicking it opens Downloads in PopcornStash.
- **Toolbar popup:** paste a web link or a magnet link and click **Send**. The popup also shows the connection, the VPN state, and the five most recent downloads with their progress.
- **Every browser download** (optional, see below).

Web links reach the server with the page's address as `Referer` (following Chrome's own rule: the full address for the same site, only the site's origin across sites) and Chrome's user agent, so download sites that check those still work. Magnet links are sent on their own, with no browser headers.

Links that only exist inside the browser cannot be sent: `blob:` (most streaming players), `data:` and `file:` links get a plain explanation instead.

### When something goes wrong

| What you see | What it means |
|---|---|
| Connect PopcornStash first | The extension is not paired yet. Open its options. |
| Pair this browser again | The server answered 401: the key was removed in Settings, Devices, or expired. The extension forgets it and asks you to pair again. |
| Cannot reach PopcornStash | The server did not answer. Check that it is running and reachable from this computer. |
| Torrents are off: the VPN is not ready | A magnet link was refused because this server only runs torrents through its VPN and the VPN is not set up. Web links still work. If the VPN is only reconnecting, the torrent is accepted and waits ("Waiting for the VPN to connect"). |
| PopcornStash did not take that link | The server rejected the link and says why (for example "That link is not valid."). |
| Allow access to your server again | Chrome no longer lets the extension reach the server (site access was changed in Chrome). The options page has a button to allow it again. |

## Optional: send all my browser downloads

Turn on **Send all my browser downloads to PopcornStash** in the options page. Chrome asks for permission to manage your downloads.

What happens to each new download:

1. Chrome starts the download as usual. The extension pauses it and sends its link (with the referring page, user agent, and cookies if that option is on) to PopcornStash.
2. If PopcornStash refuses the link, Chrome's download resumes and a notification says it was kept in Chrome and why.
3. If PopcornStash accepts it, the extension waits up to 10 seconds for the server to start fetching. If the server's own fetch fails in that time (for example the site answers 403 to the server), Chrome's download resumes.
4. Once PopcornStash is fetching (or the download is still queued behind others on the server), Chrome's copy is cancelled and removed from Chrome's download list, and a notification says "Sent to PopcornStash". If Chrome had already finished its copy, that file stays in your Downloads folder.

Downloads that stay in Chrome:

- files from PopcornStash itself, at the address you paired with or at the server's public address (so saving a film from your library never loops back);
- incognito windows;
- downloads started by other extensions;
- `blob:`, `data:` and `file:` downloads, which only exist in this browser.

Limits to know about: a download that needs a form submission (POST) or a link that works only once may fail on the server, because Chrome has already used the link. PopcornStash then shows it as failed in Downloads; download that file with the option turned off.

## Optional: include the site's sign-in cookies

Some sites only let signed-in people download. Turn on **Include this site's sign-in cookies so protected downloads work** and each link you send carries the cookies Chrome holds for that link's own site, so PopcornStash can fetch the file as you. Chrome asks to let the extension read cookies on all sites, because the extension cannot know in advance which site you will send from.

The trade-off, in plain words: those cookies can sign in as you on that site. PopcornStash holds them in memory for that one download and never writes them to its database or logs, but anyone in control of the server could use them while the download runs. Only turn this on for a server you run yourself.

Cookies are never sent for magnet links, and never for the PopcornStash server's own host names (any port): the address you paired with and the host of the server's public URL, which the server reports when pairing (`server_name`). So your PopcornStash session never travels inside a download request. The extension's own requests to PopcornStash never carry browser cookies either; they use the device key only.

## What each permission is for

| Permission | When | Why |
|---|---|---|
| `contextMenus` | Always | The "Send … to PopcornStash" right-click items. |
| `storage` | Always | Remembering the server, the device key and your two options, in `chrome.storage.local`. |
| `notifications` | Always | "Sent to PopcornStash" and plain-language problems after a right-click send. |
| Access to your server's address | Asked when you click Connect | Calling the PopcornStash API at that one address. Requested for exactly that origin (for example `https://stash.example.com/*`). |
| `downloads` | Asked when you turn on "Send all my browser downloads" | Seeing new downloads, pausing, resuming, cancelling and removing Chrome's copy. Removed again when you turn the option off. |
| `cookies` and access to all sites | Asked when you turn on the cookie option | Reading the cookies for the one site a link points to. Removed again when you turn the option off. |

The manifest declares `optional_host_permissions` of `https://*/*`, `http://localhost/*`, `http://127.0.0.1/*` and `<all_urls>`. These are only the outer limits of what may be asked for; nothing is granted until you allow it in Chrome's prompt.

Turning the cookie option off removes Chrome's access to all sites. Chrome may count your server's address as part of that and take it away too. The options page then shows **Allow access to** your server, which asks for that one address again.

## Privacy

- The device key is stored only on this computer, in Chrome's local extension storage (`chrome.storage.local`). It is never put in `chrome.storage.sync`, so it does not travel to your Google account or other computers. The extension never logs it.
- Links go only to your PopcornStash server. The extension has no analytics and sends nothing anywhere else.
- Cookies travel only when the cookie option is on, only for the site a link points to, and never for PopcornStash itself.
- The server keeps forwarded headers (cookie, referer, user agent) in memory for that download only (see docs/architecture.md, section 5, Web downloads).

## Disconnect and revoke access

- **Disconnect** in the options page signs this browser out: it calls `POST /api/auth/logout` with the device key, which makes the server delete that key (it disappears from **Settings, Devices**), then forgets the key on this computer and removes the extension's access to the server's address.
- If the server cannot be reached (or Chrome no longer lets the extension reach it), the key is still forgotten here, and the options page says plainly that it still works on the server, with a button that opens **Settings, Devices** so you can remove it there.
- If a key is removed on the server first, the extension notices at its next request (a 401), forgets the key, and asks you to pair again.
- Removing the extension from Chrome deletes its local storage, including the key. Remove the device in Settings, Devices as well.

## How it is built

- TypeScript (strict), bundled with esbuild into `extension/dist`. No framework: the popup and options page are plain DOM code, so the popup opens instantly and the whole extension stays around 30 KB zipped.
- Styling is plain CSS with custom properties that mirror DESIGN.md and the web app's Popcorn theme: zinc neutrals, blue-500 as the single action and focus colour with zinc-950 text on blue, the system font, flat surfaces with thin dividers, 8px controls, 44px targets, light and dark from the system setting, and reduced motion respected. React and Astryx are not used in the extension (Astryx is a React component library; pulling React into a 344px popup would cost more than it gives).
- Code layout: `src/background.ts` (context menus, notifications, download takeover), `src/popup/`, `src/options/`, and `src/lib/` (address validation, request payloads, API client, error wording, storage).

Scripts (run with `npm --prefix extension run <name>`):

| Script | Does |
|---|---|
| `build` | Bundle into `dist/` and stamp the manifest with the package version. |
| `check` | `tsc --noEmit` and the unit tests (Vitest). |
| `test:e2e` | Build, then load the unpacked extension in Chromium (Playwright, persistent context, headless) against a mock PopcornStash server: pair, send from the popup, take over a browser download with cookies, recover from a revoked key, sign out on Disconnect (and say so when the server is unreachable), finish pairing in a second options tab, and leave refused, failed, `data:` and PopcornStash's own downloads in Chrome. An accessibility audit runs every popup and options view in light and dark: axe-core (WCAG 2.2 A/AA) with no violations, a 44px hit area for every control, and 3:1 contrast for the switches. Uses browsers from `PLAYWRIGHT_BROWSERS_PATH`; it never downloads them. Chrome cannot show permission prompts to an automated test, so the test copy of the manifest pre-grants what a person would allow. Screenshots of the popup and options page in light and dark go to `evidence/extension/`. |
| `zip` | Build and pack `dist/` into `popcornstash-extension.zip`. |
| `icons` | Re-render `static/icons/*.png` from `assets/brand-mark.svg` (the web app's "p" mark). Needs `rsvg-convert`. The PNGs are committed. |

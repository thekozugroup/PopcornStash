# PopcornStash

Part of the Popcorn series of open source video projects (see Popcorn SR).

## Purpose
A secure, private home for a family's videos, photos and downloaded media. One folder on a NAS or Unraid share holds everything. PopcornStash keeps it organized, plays it in any browser, and brings new files in from torrents, links, uploads and drag-and-drop without the family having to think about folders.

## Users
- **The owner:** a self-hoster running Docker on a NAS, Unraid server or home PC. Comfortable with a compose file, not interested in babysitting a media stack.
- **Family members:** invited by the owner. They sign in with a passkey on their phone or laptop, watch, browse photos, and add files. They never see a file path unless they ask for it.

## Design
Taste + Impeccable, built from Astryx components. Same world as Popcorn SR: restrained Apple-like hierarchy, native system typography, zinc neutral surfaces, Tailwind blue-500 as the single action/focus accent with dark text on blue. Emerald/amber/red only for status, always with words. Flat sections, thin dividers, no decorative charts, gradients, glass or fake activity. The library's own imagery (posters, photos, video frames) provides the colour. 8px control radius, 16px panel radius, 44px targets. Light/dark from system preference; reduced motion respected. One primary action per view.

## Journey
1. **Start:** run the container, open the HTTPS address, enter the one-time setup code from the logs, create the owner passkey.
2. **Protect downloads:** paste a NordVPN access token (or upload a WireGuard/OpenVPN file). Torrent traffic only flows through the tunnel; the page shows the VPN's public address as proof.
3. **Bring media in:** magnet links, .torrent files, web links, the Chrome extension, Android share, browser uploads, or files copied into `Inbox/`. Everything passes through `Downloads/In Progress` and `Downloads/Completed`, then gets sorted.
4. **Enjoy:** Movies, Shows, Home Videos and Photos, organized automatically. Anything uncertain waits in Inbox for one tap of review. Videos play in the browser, transcoded only when the device needs it.
5. **Share with family:** invite people; each person adds their own passkeys.

## Honest boundaries
- Passkeys need HTTPS on a domain name (or `localhost`). A bare LAN IP cannot use passkeys; say so plainly and point to the fix.
- Torrents stop when the VPN is down. There is no silent fallback to the home connection.
- Files outside `Inbox/` and `Downloads/` are indexed where they are and never moved without an explicit action.
- No online metadata scraping by default: nothing about the library leaves the server.
- The Android app is a later phase; the API and pairing flow are designed for it now.

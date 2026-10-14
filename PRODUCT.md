# PopcornStash

Part of the Popcorn series of open source video projects (see Popcorn SR).

## Purpose
A secure, private home for a family's videos, photos and downloaded media. One folder on a NAS or Unraid share holds everything. PopcornStash keeps it organized, plays it in any browser, and brings new files in from torrents, links, uploads and drag-and-drop without the family having to think about folders.

## Users
- **The owner:** a self-hoster running Docker on a NAS, Unraid server or home PC. Comfortable with a compose file, not interested in babysitting a media stack. Decides which libraries the stash has, what each one holds and who sees it.
- **Family members:** invited by the owner. They sign in with a passkey on their phone or laptop, watch, browse photos, and add files, in the libraries the owner shares with them. They never see a file path unless they ask for it.

## Design
Taste + Impeccable, built from Astryx components. Same world as Popcorn SR: restrained Apple-like hierarchy, native system typography, zinc neutral surfaces, Tailwind blue-600 as the single action/focus accent with white on blue. Emerald/amber/red only for status, always with words. Flat sections, thin dividers, no decorative charts, gradients, glass or fake activity, with one exception: the Android app's floating toolbar is frosted glass on Android 12 and later (opaque on Android 9 to 11 and when the phone's contrast setting is medium or high). The library's own imagery (posters, photos, video frames) provides the colour. 8px control radius, 16px panel radius, 44px targets. Light/dark from system preference; reduced motion respected. One primary action per view.

## Journey
1. **Start:** run the container, open the HTTPS address, enter the one-time setup code from the logs, create the owner passkey.
2. **Protect downloads:** paste a NordVPN access token (or upload a WireGuard/OpenVPN file). Torrent traffic only flows through the tunnel; the page shows the VPN's public address as proof.
3. **Shape the libraries:** a new stash starts with Movies, Shows, Home Videos and Photos. The owner can rename them, give each its own icon, add more (Kids, Holidays, Grandma's photos), reorder them, and choose who sees each one. A library is videos or photos; videos are either series with seasons and episodes or one-offs. Each library can take one or more kinds of new file (movies, TV episodes, camera videos, photos), and each kind goes to one library at most. An existing folder on the share can become a library without moving a file.
4. **Bring media in:** magnet links, .torrent files, web links, the Chrome extension, the Android app's share sheet, browser uploads, or files copied into `Inbox/`. Everything passes through `Downloads/In Progress` and `Downloads/Completed`, then gets sorted into the library that takes that kind of file. Music, documents and everything else go to Other files.
5. **Enjoy:** every library you can see, organized automatically, on the web and in the Android app. Anything uncertain, or a kind no library takes, waits in Inbox, where you pick the library with one tap. Videos play in the browser and the app, transcoded only when the device needs it.
6. **Share with family:** invite people; each person adds their own passkeys and sees the libraries shared with everyone or with them.

## Honest boundaries
- Passkeys need HTTPS on a domain name (or `localhost`). A bare LAN IP cannot use passkeys; say so plainly and point to the fix.
- Torrents stop when the VPN is down. There is no silent fallback to the home connection.
- Files outside `Inbox/` and `Downloads/` are indexed where they are and never moved without an explicit action.
- Libraries are views of folders. Making, renaming, reordering or deleting a library never moves, renames or deletes a file; a library's folder is chosen once and stays. A library's type changes only while it is empty.
- Only owners manage libraries. A library shown to "only selected people" is invisible to everyone else everywhere in PopcornStash: pages, search, Home, downloads, live updates. Hiding it stops new access at once, but a device keeps what it has already loaded.
- Library access lives in the app, not on the disk. Anyone who can open the share or the server's disk reads every folder, hidden libraries included. Give family members no share access, or write access to `Inbox/` only.
- Owners decide where each kind of file goes. A file you add can be sorted into a library you cannot see; it then leaves your view, and your download says only that it was sorted.
- Downloads, uploads and shares are seen by the person who added them and the owners. Other people see a download only once it is sorted into something they can see.
- Other files (music, documents) are private: each file is seen by the person who added it and the owners. Folders on the share that are not a library are seen by owners only, until an owner makes them a library.
- No online metadata scraping by default: nothing about the library leaves the server.
- The Android app has three tabs (Downloads, Home, Settings); library management is native, the other admin areas open the web settings for now.

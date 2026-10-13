# PopcornStash for Android: design system

The Android app lives in the same world as the web app (`../DESIGN.md`): zinc chrome, Action Blue only where the next action is, colour from the posters, frames and photos, flat surfaces, thin dividers, the system sans. Material 3 is the base for structure and behaviour (insets, system Back, sheets, tooltips, ripples, accessibility); the Popcorn tokens theme it. There is no dynamic (wallpaper) colour: the library's pictures provide the colour, and a wallpaper tint would compete with them.

Every value below is a token in `app/src/main/kotlin/com/thekozugroup/popcornstash/ui/theme/`. To try a change, edit the token, run `./gradlew recordRoborazziDebug` and look at `app/src/test/screenshots/`.

## Tokens

### Colour (`Color.kt`)

| Token | Light | Dark | Used for |
| --- | --- | --- | --- |
| `page` | zinc-50 `#FAFAFA` | zinc-950 `#09090B` | the canvas behind every page, the top app bar |
| `surface` | white `#FFFFFF` | zinc-900 `#18181B` | toolbar, sheet, panels |
| `subtle` | zinc-100 `#F4F4F5` | zinc-800 `#27272A` | media placeholders, avatar, progress track |
| `divider` | zinc-200 `#E4E4E7` | zinc-800 `#27272A` | panel borders, row dividers, the scrolled top bar's rule |
| `text` | zinc-900 `#18181B` | zinc-50 `#FAFAFA` | main text and icons |
| `textMuted` | zinc-600 `#52525C` | zinc-400 `#9F9FA9` | metadata, unselected toolbar icons, ghost buttons |
| `accent` (Action Blue) | blue-500 `#2B7FFF` | blue-500 `#2B7FFF` | the selected toolbar square, primary buttons, progress, focus |
| `onAccent` | zinc-950 `#09090B` | zinc-950 `#09090B` | icons and text on Action Blue (5.4:1) |
| `link` | blue-600 `#155DFC` | blue-400 `#51A2FF` | inline links only |
| `mediaHairline` | zinc-950 at 8% | white at 8% | the 1dp inner edge on tiles |
| `stage` | black | black | photo viewer and player |
| `mediaControl` | zinc-950 at 62% | zinc-950 at 62% | discs behind controls laid over a picture |
| `connected` / `warning` / `error` | emerald-700, amber-700, red-700 | emerald-400, amber-400, red-400 | status words; the dot beside them uses the 500 shade |

Material roles: `primary` = accent, `onPrimary` = onAccent, `background` and `surface` = page, every `surfaceContainer*` = surface, `onSurface` = text, `onSurfaceVariant` = textMuted, `outlineVariant` = divider, `outline` = zinc-300 / zinc-700, `secondaryContainer` = subtle, `inverseSurface` = zinc-900 / zinc-50 (tooltips), `error` = red-700 / red-400.

### Type (`Type.kt`)

The device's system sans (Roboto on most phones), in sp so it follows the font-size setting. Hierarchy comes from size and weight, never a second face.

| Token | Size / line | Weight | Material role | Used for |
| --- | --- | --- | --- | --- |
| `headline` | 28 / 34 | 600, -0.025em | headlineMedium | page title in the top bar |
| `title` | 20 / 28 | 600, -0.025em | titleLarge | "Continue watching", "In progress", photo months |
| `section` | 16 / 24 | 600 | titleMedium | shelf headings, the person's name in the sheet |
| `body` | 16 / 24 | 400 | bodyLarge | sentences |
| `item` | 14 / 20 | 500 | bodyMedium | tile titles, row names |
| `action` | 14 / 20 | 600 | labelLarge | buttons, "See all" |
| `metadata` | 13 / 18 | 400, tabular figures | bodySmall | the line under a tile or row ("2016 · 1 h 52 min") |
| `tag` | 12 / 16 | 500 | labelSmall | counts and badges |

Sentence case everywhere, no em dashes, two facts join with a middle dot.

### Spacing (`Spacing.kt`)

`4 · 8 · 12 · 16 · 20 · 24 · 32 · 40` dp. The page gutter is 20dp (the web phone gutter). 8dp inside a group, 16dp between related items, 24dp under a section heading, 32dp between page blocks, 40dp between Home's sections. Shelves scroll edge to edge with the gutter kept as content padding.

### Shape (`Shape.kt`)

One scale: tags 6dp, controls 8dp, media 10dp, toolbar squares 12dp, panels, sheets and the toolbar 16dp, the avatar a circle. Library icon squares in lists are 40dp with 8dp corners; icon picker cells are 48dp with 8dp corners. The toolbar's outer radius is always the square's radius plus the toolbar padding (12 + 4 = 16), so the selected square sits concentric in the corner squares.

### Elevation

Flat. Pages, panels and tiles have no shadow; tiles get the 1dp `mediaHairline` instead. Only layers that float above the page cast one: the toolbar (8dp soft shadow plus a 1dp `divider` border so it holds its edge on a bright photo and on black) and Material's own sheet scrim and tooltip.

### Motion (`Motion.kt`)

| Token | Value | Used for |
| --- | --- | --- |
| `pageSpring` | spring, no bounce, stiffness 500 (about 300 ms) | page slides |
| `indicatorSpring` | spring, damping 0.85, stiffness 500 | the toolbar's selection square |
| `fast` | 150 ms | icon tint, crossfades |
| `toolbarShowHide` | 200 ms | the toolbar leaving for the viewer and the player |
| `push` | 30dp horizontal shift with a fade, `pageSpring` | opening and closing a page inside a tab (a library, a show, an item, the library editor) |

When the system's "Remove animations" setting is on (`LocalReduceMotion`), tabs and pushed pages crossfade instead of sliding, the carousel snaps without gliding, and the selection square moves without travelling. Nothing loops.

### Touch and icons

Every target is at least 48dp. Icons are Lucide (the web's set, from the same Lucide release as the web's `lucide-react` 1.55.0) at 24dp with a 1.75 stroke (`IconStroke`); empty-state icons are 28dp at 1.5. Fixed glyphs live in `ui/icons/PopcornIcons.kt`; library icons in `ui/icons/LibraryIcons.kt`, one entry per name in the server's curated list (`GET /api/libraries/icons`, docs/architecture.md section 8), with `folder` for a name the app does not know.

| Place or action | Lucide glyph |
| --- | --- |
| Home tab | `house` |
| Downloads tab | `arrow-down-to-line` |
| Settings tab | `settings` |
| Each library | the icon its owner chose (the four starting libraries: `film`, `tv`, `clapperboard`, `image`) |
| Other files | `file-text` |
| Ready to import | `inbox` |
| Folder not in a library | `folder` |
| Add a link | `plus` |
| Pause, resume | `pause`, `play` |
| More | `ellipsis-vertical` |
| Back | `arrow-left` |
| Open on the web | `external-link` |
| Close (viewer, player) | `x` |
| Favourite | `heart` |
| Server | `server` |
| Sign out | `log-out` |
| Open a library | `chevron-right` |

## Navigation

### Floating toolbar (`ui/components/FloatingNavToolbar.kt`)

The app's navigation: one elongated rounded rectangle floating at the bottom centre, icons only, no labels.

- Three destinations in a row: Downloads on the left, Home in the centre, Settings on the right. Home is the start tab.
- Each icon sits in its own square target: 52dp, 12dp corners. The squares touch, so together they make one bar; the bar adds 4dp of padding and a 16dp outer radius (12 + 4: the outer radius is always the inner radius plus the padding). On a screen narrower than the bar plus two 16dp margins, the squares shrink, never below 48dp.
- Container: `surface`, 1dp `divider` border, 8dp soft shadow. 16dp above the gesture or navigation bar inset.
- Unselected icons: `textMuted`. Selected: the square fills with Action Blue and the icon turns `onAccent`. This is the one place navigation uses blue, by the owner's direction; elsewhere navigation stays neutral as on the web.
- The Downloads square carries the number of files ready to import (the `inbox` count) as a small `text`-coloured badge on its top-end corner (`tag` type on `page`, never a status colour); it disappears at zero.
- Every square has a content description (its name, and for Downloads the count: "Downloads, 3 ready to import"), the tab role, its position ("1 of 3") and a ripple clipped to the square. A long press shows a plain tooltip with the name above the square.
- It floats over the content: pages scroll under it and end with enough bottom padding (bar height + its margin + the inset + 16dp) that the last row clears it.
- It leaves (slides down and fades, `toolbarShowHide`) on the player and the photo viewer, and while the keyboard is open; it comes back when they close.

### Selection indicator

One Action Blue square that lives behind the icons and slides along the bar to the tapped square with `indicatorSpring`. The icon tints cross over during the slide (`fast`). Under reduce motion it jumps.

### Tab transitions (`navigation/PageTransitions.kt`)

- Tapping a tab slides the pages horizontally, direction aware: the new page comes in from the side of the tapped square (from the right when it is to the right of the current one, from the left when it is to the left) while the old one leaves the other way. Full width, `pageSpring` (about 300 ms).
- Each tab keeps its own back stack, state and scroll position (a saveable state holder per tab, data in a view model per destination). Tapping the current tab again scrolls its first page to the top, or pops its stack to the first page.
- Inside a tab, pages open and close with `push`. The toolbar stays while browsing.
- System Back pops the current tab's stack first; at a tab's first page it returns to Home (sliding back), and from Home it leaves the app. Back closes the viewer or player before anything else.
- Reduce motion: a 150 ms crossfade for tabs and pushes.
- No swiping between tabs: Home's shelves and carousel scroll sideways and a tab swipe would fight them.

### Top app bar (`ui/components/PageTopBar.kt`)

- Part of each page, so it slides with it. 64dp tall below the status bar inset, on `page`.
- A tab's first page: the page title in `headline` ("Home", "Downloads", "Settings"). A pushed page: a 48dp back button (`arrow-left`, "Back") before the title in `title` type (the library's name, the show, the item, "New library").
- Trailing actions, at most two 48dp icon buttons with tooltips: Downloads has "Add a link" (`plus`); an owner's library page has "Edit library" (`settings`).
- When the page scrolls under it, a 1dp `divider` rule appears at its bottom edge; no colour change, no shadow.

## Pages

### Home

1. **Continue watching** (only when there is something to resume; no empty state here). A horizontal pager of 16:9 frames, each card the gutter-to-gutter width minus a 32dp peek of the next card, 12dp apart, snapping one card at a time. Under each frame, 10dp down: the title in `item` and one `metadata` line ("S01E02 · 12 min left"). The frame carries the 4dp Action Blue progress bar over a zinc-950 40% track along its bottom. Tapping a card resumes it in the player at once; a 48dp More button beside the title offers "Details" and "Start from the beginning". Content description: "Resume Bluey S01E02, 12 minutes left". The pager has no dots: the peek shows there is more.
2. **Libraries**, every one the person can see, in the owner's order, 40dp apart. Each starts with a 56dp header row: the library icon at 24dp in `textMuted`, 12dp, the name in `section`, its count in `metadata` beside it, and a trailing `chevron-right`; the whole row opens the library. Under it, 12dp down, a shelf of up to twelve of its newest items in the library's shape: posters for `posters` and `series` (a show's poster stands for its episodes), frames for `frames`, squares for `timeline`. A library with nothing yet keeps its header with "Nothing here yet" in `metadata` under it.
3. **Other files**, last, as a single 56dp row (`file-text`, "Other files", its count, chevron) when the person has any.

When the person can see no library: an empty state with the `house` icon, "Nothing has been shared with you yet." for members, or "No libraries yet" with "Make a library" (opening the editor in Settings) for the owner.

### A library (pushed from Home)

- **Posters or frames (one-offs):** a lazy grid (posters at least 104dp, three columns on a phone; frames at least 160dp, two columns), newest first, with a sort menu (Newest, Title, Year) as a trailing top bar action.
- **Series (episodes):** a poster grid of shows ("3 seasons" or "12 episodes" under each). A show page: its poster frame at the top (16:9 crop of the poster episode), "Resume S01E02" or "Play S01E01" as the primary button, a season picker (Material segmented buttons up to four seasons, a dropdown beyond), then episode rows: a 128dp 16:9 frame, the episode name in `item`, its state in `metadata` ("Watched", "Up next · 5 min left", or the runtime).
- **Timeline (photos):** month headings in `title` with the count in `metadata`, square tiles four across with 3dp gaps. A photo opens the full-screen viewer.
- **Other files:** a list of rows with the file-type glyph, name and "PDF · 592 B"; tapping one opens it with the system viewer through a download.
- Empty: the library's own icon, "Nothing in Kids yet", and how it fills, as on the web.
- A library that disappears while open (deleted or hidden) pops back to Home with a snackbar: "Kids is not available any more."

### Item detail (pushed)

The picture at the top (frame or poster crop, full width, 10dp corners inside the gutter), the title in `headline`, one `metadata` line ("2014 · 1 h 52 min"), then the one primary button ("Resume from 12:04" or "Play") with "Start from the beginning" as a text button under it, a favourite toggle (`heart`, "Add to favourites" or "Remove from favourites"), and the facts (video, audio, size) as label-over-value columns. An episode adds "Next: S01E03 The lantern keepers" as a row when there is one in the same library.

### Player and photo viewer (immersive)

Full-bleed `stage` black, the toolbar hidden, edge to edge under the system bars (bars hidden while controls are hidden). Player controls sit on `mediaControl` discs and plates: close at the top start, the title beside it, play and pause in the centre, the seek bar with Action Blue progress and times at the bottom, audio and quality in a trailing menu. At the end of an episode with a next one: "Play next episode" as the primary button and "Back"; nothing starts on its own. Picture-in-picture keeps only play and pause. When the server stops answering for this video: "This video is no longer available." and Back. The photo viewer pages sideways between photos, with close at the top start and the date and camera on a `mediaControl` plate at the bottom.

### Downloads

The tab where files come in: watch what is downloading, then put what finished into the right library.

1. **The VPN line** under the top bar, one row in the web's words with the status dot beside them ("Protected · NordVPN · Netherlands", "Torrents paused: VPN connecting", "No VPN: torrents use this network" with its reason line). For the owner it opens the web VPN settings in a Custom Tab (trailing `external-link`).
2. **In progress** (when any): rows with the name in `item` (two lines, breaking at dots, underscores and hyphens), a 4dp progress bar (Action Blue on `subtle`), and a `metadata` strip of facts spaced 12dp apart ("64%  2.1 MB/s  12 min left"), or the reason it waits ("Waiting for the VPN to connect.", "Waiting for a free slot (5 at once)."). Trailing: a 48dp Pause or Resume icon button and a More menu with "Remove" (confirmed: "Remove this download? What was downloaded so far is deleted.").
3. **Ready to import** (when any), with its count and "Accept all" as a text button in the section header. Each row: a 64dp frame or the file-type glyph, the file name, the reason it waits in `metadata` ("Not sure this is a movie.", "No library takes TV episodes yet."), then the library picker (an exposed dropdown, 48dp, showing the library's icon and name, or "Choose a library"; it lists only the libraries the person can see that fit the file, then Other files) and the actions: "Accept" (the primary button on the first row only, a tonal button on the others; it waits for a library when none is chosen), "Edit" (a sheet with the title, year or show, season and episode, the same picker, and the live preview "Will move to Kids" as the one place that says it in words) and a More menu with "Ignore". Ignored files follow under a closed disclosure row, "Left where they are (3)", with the same row.
4. **Finished:** the latest downloads with "Finished 3 min ago" and where they went, as a tappable line with the library's 16dp icon: "In Kids", "In Other files", "2 waiting to import".

Empty (nothing at all): the `arrow-down-to-line` icon, "Nothing downloading", "Links and magnets you add, and files you share to PopcornStash, show up here." and "Add a link". The add sheet: a "Link or magnet" field (URL keyboard, prefilled from the clipboard when it holds one), "Add" as the primary button.

### Settings

1. **You:** the 48dp avatar with initials, the name in `section`, the role in `metadata` ("Owner", "Member").
2. **This server:** the address with the `server` glyph and the version, not tappable.
3. **Libraries** (owner): rows like the web's: the icon on a 40dp `subtle` square, the name in `item`, a `metadata` strip of what it is, what it takes and who sees it ("Series  Takes TV episodes  Everyone"); the row opens the editor; a More menu holds "Move up", "Move down" and "Delete". Then "Make a library" as a secondary button. **Folders not in a library** follow when there are some: the `folder` glyph, the folder name with a trailing slash in the monospace face, counts, and "Make a library".
4. **On the web:** rows with a trailing `external-link`, each opening that web settings page in a Custom Tab: VPN, People, Devices, Security, System for the owner; Devices and Security for members. The first time, the browser asks the person to sign in with their passkey.
5. **Sign out:** a quiet row in `textMuted` with `log-out`, confirmed in a dialog ("Sign out on this phone?"; when uploads are running, "2 uploads stop." is added).

### Library editor (pushed from Settings)

The web editor's fields, in the same order and with the same words, as one scrolling form: Name; the icon picker with a preview of the library's Home header row under it; "Videos" or "Photos", then "Series" or "One-offs" (radio rows; read-only with "The type can change only while the library is empty." on a library that holds files); "Sort these here" checkboxes with where each kind goes now ("Sorted into Films now", "Moves here from Films"); Folder (making: "Make a new folder" with its name, or "Use a folder that is already there" with a picker; editing: the folder as a fact, "Renaming the library keeps this folder."); "Everyone" or "Only some people" with member checkboxes and "Owners see every library."; then the primary button ("Make library" or "Save changes") at the end of the form. "Delete library" sits last, apart, confirmed in a dialog that says the files stay. Server errors appear under the field they name.

### Icon picker (`ui/components/IconPicker.kt`)

A grid of 48dp cells (`LazyVerticalGrid` with adaptive 48dp columns and 8dp spacing; six or seven across on a phone) in the server's order. Each cell: the 24dp glyph in `textMuted` with 8dp corners. The chosen cell: `subtle` fill, a 1.5dp `text` ring and the glyph in `text`; neutral, not Action Blue, because it is a choice, not navigation or an action. Semantics: one selectable group, each cell a radio button whose content description is the icon's plain name ("Party popper"), with a tooltip on long press.

### Sign-in

1. **Server:** the brand, "Server address" (URL keyboard, `https://` assumed when no scheme is typed) and "Continue". Errors in words: "PopcornStash did not answer at this address.", "Use an https address."
2. **How to sign in:** "Sign in with a passkey" as the primary button when the server lists the app, and "Pair with a code" as a secondary button (the only way when it does not, with the sentence "Passkeys work in the app once the server lists it. Pair with a code instead.").
3. **Pairing:** the code large in the monospace face with spaced groups, "Open PopcornStash to approve" (primary, a Custom Tab on `/pair`), and "Waiting for approval" with a small indeterminate indicator; "This code expired." or "This request was turned down." with "Start again".

### Share sheet (`share/ShareActivity.kt`)

A bottom sheet over the app that shared. A link or magnet: the link (two lines), "Add to Downloads" as the primary button, "Cancel". Files: up to three names with sizes ("and 5 more"), "Uploads as Ana to stash.example.com." and "Upload" as the primary button. Not signed in: "Sign in to PopcornStash first." with "Open PopcornStash". The upload notification shows "Uploading 3 files" with progress, then "3 files uploaded. They will be sorted on their own." and opens Downloads when tapped.

## Components

### Tiles and shelves (`ui/components/MediaTile.kt`, `Shelf.kt`, `MediaGrid.kt`)

- The picture is the tile. Shapes follow the library's `layout`: posters 2:3 (`posters`, `series`), frames 16:9 (`frames`, continue watching), photos square (`timeline`). 10dp corners, the 1dp hairline inside the edge, `subtle` placeholder with the library's icon at 22dp when there is no picture.
- Under the picture, 10dp down: the title in `item` (two lines, then an ellipsis) and one `metadata` line. Photo tiles have no caption.
- Partial watch progress: a 4dp Action Blue bar along the bottom of the picture over a zinc-950 40% track.
- Shelves: a row that scrolls sideways with snapping, 12dp under their header. Tile widths: posters 132dp, frames 232dp, photos 112dp; 14dp between tiles.
- Grids: posters at least 104dp wide (three columns on a phone), frames at least 160dp (two), photos at least 96dp (four, 3dp gaps, grouped under a month `title`).

### Rows (`ui/components/ListRow.kt`)

Downloads, imports, libraries, folders and settings are rows on `page` separated by 1dp `divider` rules (no cards, no boxes inside boxes): a leading glyph or picture, a name in `item`, one `metadata` line or strip, and trailing actions. Rows are at least 56dp tall and the whole row is one target when it opens something.

### Empty states (`ui/components/EmptyState.kt`)

Centred in the page: a 28dp line icon (stroke 1.5, the place's glyph; a library's own icon) in `textMuted`, a `section` title ("Nothing in Kids yet"), one or two `body` sentences in `textMuted` that say how the space fills, at most one action. No illustration, no box around it.

### Loading and errors

Loading pages show placeholder tiles and rows in `subtle` at their final sizes (no spinners on pages; a small indeterminate indicator only inside a button or the pairing wait). A failed load shows one sentence and "Try again" where the content would be. Lost connection: a single row under the top bar, "Not connected to the server. Trying again.", in `metadata` with the Warning dot.

## Owner's knobs

The values most likely to change as the design is iterated, all in `ui/theme`: `ToolbarTokens` (square size, radius, padding, gap, elevation, selected colours, badge), `PopcornMotion.pageSpring`, `indicatorSpring` and `push`, `IconStroke`, the type table, the tile widths in `MediaTokens`, and `CarouselTokens` (card peek, gap, frame ratio).

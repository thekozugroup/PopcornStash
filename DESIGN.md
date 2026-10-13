---
name: PopcornStash
description: A quiet, private family library where the media itself provides the colour.
colors:
  accent: "oklch(62.3% 0.214 259.815)"
  accent-text: "oklch(54.6% 0.245 262.881)"
  accent-text-dark: "oklch(70.7% 0.165 254.624)"
  white-surface: "#ffffff"
  zinc-paper: "oklch(98.5% 0 none)"
  zinc-subtle: "oklch(96.7% 0.001 286.375)"
  zinc-divider: "oklch(92% 0.004 286.32)"
  zinc-dark-muted: "oklch(70.5% 0.015 286.067)"
  zinc-status: "oklch(55.2% 0.016 285.938)"
  zinc-muted: "oklch(44.2% 0.017 285.786)"
  zinc-dark-divider: "oklch(27.4% 0.006 286.033)"
  zinc-ink: "oklch(21% 0.006 285.885)"
  zinc-black: "oklch(14.1% 0.005 285.823)"
  stage-black: "oklch(0% 0 0)"
  media-control: "oklch(14.1% 0.005 285.823 / 0.62)"
  status-connected: "oklch(69.6% 0.17 162.48)"
  status-warning: "oklch(76.9% 0.188 70.08)"
  status-error: "oklch(63.7% 0.237 25.331)"
typography:
  headline:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "1.875rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "-0.025em"
  section:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: 1.5
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "14px"
    fontWeight: 550
    lineHeight: 1.5
  action:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "0.875rem"
    fontWeight: 600
    lineHeight: 1.4285714286
  navigation:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.4285714286
  metadata:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.5
  tag:
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.4
rounded:
  control: "8px"
  panel: "16px"
  media: "10px"
  tag: "6px"
  progress: "4px"
spacing:
  field-gap: "8px"
  group-gap: "16px"
  form-gap: "20px"
  section-gap: "24px"
  panel-padding: "28px"
  page-gap: "32px"
  fact-gap: "32px"
  major-gap: "40px"
  settings-columns: "48px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.zinc-black}"
    typography: "{typography.action}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "44px"
  button-secondary:
    backgroundColor: "{colors.white-surface}"
    textColor: "{colors.zinc-ink}"
    typography: "{typography.action}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "44px"
  button-ghost:
    textColor: "{colors.zinc-muted}"
    typography: "{typography.action}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "44px"
  input:
    backgroundColor: "{colors.white-surface}"
    textColor: "{colors.zinc-ink}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    height: "44px"
  status-tag:
    backgroundColor: "{colors.zinc-subtle}"
    textColor: "{colors.zinc-muted}"
    typography: "{typography.tag}"
    rounded: "{rounded.tag}"
    padding: "5px 9px"
  panel:
    backgroundColor: "{colors.white-surface}"
    rounded: "{rounded.panel}"
    padding: "28px"
  media-tile:
    backgroundColor: "{colors.zinc-subtle}"
    rounded: "{rounded.media}"
---

# Design System: PopcornStash

## Overview

**Creative North Star: "The family shelf"**

A private shelf for a family's videos and photos. Neutral surfaces step back so posters, frames and photos carry the colour. Every incoming file has a visible, honest path: in progress, completed, sorted. The interface earns trust by saying exactly where things are and whether torrent traffic is protected, once and in plain words.

PopcornStash shares Popcorn SR's world: Tailwind zinc neutrals, Tailwind blue-500 as the single action and focus accent with zinc-950 text on blue, the platform system sans, flat panels, thin dividers, 44px controls, system light and dark, and reduced motion. Components come from Astryx (`@astryxdesign/core`) with a custom `popcorn` theme (`web/src/themes/popcorn`, rebuilt with `npm run theme:build`) that maps these tokens onto Astryx's token surface. Astryx components are used as they are wherever one exists; local components are limited to media pieces (poster grid, shelf, episode rows, photo timeline, player chrome) and small layout wrappers (page header, settings group, status tag).

It is an operating surface, not a showcase: familiar patterns, steady density, and no decoration that the library's own pictures do not provide.

**Key Characteristics:**
- Zinc chrome, blue only where the next action is, colour from the media.
- One h1 per page with at most one sentence under it.
- Flat white panels with a 1px divider border; no shadows on the workspace.
- Status in words with a small dot, never a dot alone.
- 44px targets everywhere, including on touch for "small" controls.

## Colors

A cool neutral zinc ramp with one saturated blue, plus three status hues that only ever appear next to words.

### Primary
- **Action Blue** (`accent`): fill of the one primary button per view, focus rings (2px, 3px offset), the progress fill of downloads and the watched hairline on tiles, text selection tint, and the caret. Text on Action Blue is Zinc Black in both themes.
- **Link Blue** (`accent-text` in light, `accent-text-dark` in dark): inline text links only ("Nord Account", "Sunset Drive (2020)"). It steps one shade away from the surface so links keep AA contrast. A standalone link that is the way forward in a row is a ghost button instead, so it keeps a 44px target.

### Neutral
- **White Surface** / **Zinc Ink** (dark): panels, cards, inputs, menus, the sidebar.
- **Zinc Paper** / **Zinc Black** (dark): the page canvas behind everything.
- **Zinc Ink** / **Zinc Paper** (dark): main text and icons.
- **Zinc Muted** / **Zinc Dark Muted** (dark): supporting text, labels, ghost buttons, inactive navigation and its icons.
- **Zinc Divider** / **Zinc Dark Divider** (dark): panel borders, row dividers, the settings group rules.
- **Zinc Subtle** / **Zinc Dark Divider** (dark): status tag fill, media placeholders, the code box in pairing, the selected navigation fill (at low alpha).
- **Stage Black** (`stage-black`): the player and lightbox stage in both themes, so media is judged against neutral black.
- **Media Control** (`media-control`, `--popcorn-media-control`: zinc-950 at 62%, 80% on hover): the disc or plate behind controls laid over a picture (the photo viewer's arrows and close button, the full-screen button on a photo), so they read on a bright beach and a night sky alike.

### Status
- **Connected** (emerald), **Warning** (amber), **Error** (red): VPN protected, connecting or paused, blocked or off; download failures; low disk space. As text they step to the 700 shade in light and the 400 shade in dark for contrast.

| Semantic | Light | Dark |
| --- | --- | --- |
| Page | Zinc Paper | Zinc Black |
| Surface | White Surface | Zinc Ink |
| Main text | Zinc Ink | Zinc Paper |
| Supporting text | Zinc Muted | Zinc Dark Muted |
| Border/divider | Zinc Divider | Zinc Dark Divider |
| Subtle fill | Zinc Subtle | Zinc Dark Divider |
| Action fill/focus | Action Blue | Action Blue |
| Action text | Zinc Black | Zinc Black |
| Link text | Link Blue (blue-600) | Link Blue (blue-400) |

**The One Accent Rule.** Blue marks the next action, focus and real progress. Navigation, tabs, filters and secondary actions stay neutral; the selected tab marker is ink, not blue.

**The Words First Rule.** Emerald, amber and red never stand alone: every dot sits beside the words that carry the meaning.

## Typography

**Body and display font:** the platform system sans (`-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`).
**Code font:** `ui-monospace` for paths, codes and commands only.

**Character:** One quiet family for everything; hierarchy comes from size and weight steps, never from a second typeface.

### Hierarchy
- **Headline** (600, 30px, 1.2, -0.025em): the page h1 ("Movies", "Settings", a film's title).
- **Title** (600, 20px, 1.4, -0.025em): section headings on Home and Downloads ("Continue watching", "In progress"), photo months, status headlines in panels.
- **Section** (600, 16px, 1.5): h3 headings of settings groups and shelves ("Who has access", "Movies 3 new").
- **Body** (400, 16px, 1.5): page subtitles, panel text, values in fact rows. Subtitles cap at 58ch and wrap with `text-wrap: pretty`.
- **Label** (550, 14px): form labels and short headings inside panels.
- **Action** (600, 14px): every button label.
- **Navigation** (500, 14px): sidebar and tabs.
- **Metadata** (400, 13px): one line under tiles, download and inbox facts, settings group descriptions, helper text. Numbers use tabular figures. Two facts join with a middle dot ("2014 · 1 min"); a strip of three or more ("Upload  Finished 3 minutes ago  345 KB") is spaced 16px apart instead of dotted.
- **Tag** (400, 12px): status tags and badges.

**File names** ("Saltwater.Choir.2022.1080p.WEB.webm") in Downloads and the Inbox wrap after their dots, underscores and hyphens, never mid-word; only a long run without separators breaks where it must.

**The Sentence Case Rule.** Every label, heading and button is sentence case. No uppercase eyebrows, no letter-spaced labels, no em dashes.

**The Say It Once Rule.** A state is named once per place. A status line has a short name and, only when it adds something, a reason and what still works ("Torrents off" and "Web links and uploads still work.", not "Torrents unavailable" and "Torrents are turned off on this server."). One word per state across the app: "off" when torrents are off on purpose, "unavailable" when the engine is not answering ("Torrents unavailable: the torrent engine is not answering"; "Not answering" for the engine in System), "paused" while the VPN connects, "blocked" when the VPN fails. A VPN reads "Protected · NordVPN · Netherlands", each part once; when it wraps, it wraps between parts and no line starts or ends with a dot. Facts under a status line never repeat it (no "Provider: NordVPN" under "Protected · NordVPN · Netherlands"), a value never repeats its label ("ffmpeg: 6.1.1", not "ffmpeg: ffmpeg version 6.1.1"), and a form's live preview is the one place that says where a file goes ("Will move to Other files", not also "Goes to Other files" above it).

## Layout

**Shell.** Desktop: a sticky 228px sidebar (brand; Home, then every library the person can see in the owner's order, each with its own icon, then Other files; Inbox with its count, Downloads; Settings; a foot with the VPN tag, the person's name and Sign out) and a bounded main column (max 1320px, padding `clamp(24px, 5vw, 64px)`, more at the bottom). When the libraries do not fit the window height, the middle of the sidebar scrolls and the brand and the foot stay put. Under 800px the sidebar becomes a compact top bar with a menu drawer (the same sidebar, icons included), and a row of quick links (Home, the first three libraries the person can see, Downloads), each its icon above its label, sits on the 20px page gutter in equal cells 4px apart (fewer libraries, fewer cells), its current pill aligned with the content edge. See Navigation under Components.

**Page header.** One h1, at most one sentence under it, then 32px to the content. Trailing actions are rare (Inbox's "Accept all suggestions"); when the action is the page's only way forward ("Go to Home" on a not-found page) it sits under the sentence.

**Settings.** The Settings h1, then a tab strip (one tab per section), then the section's one-sentence lead. Under 640px an owner's six tabs would scroll sideways and hide Libraries and System, so they become one full-width section picker (Astryx Selector) that lists every section; a member's two tabs fit and stay tabs. Every section keeps its own address (`/settings/vpn`). Each section keeps its h2 for the document outline but hides it, because the selected tab already shows the name. Sections are groups: on screens 1100px and wider, a 200-260px intro column (h3, one or two sentences, and the group's action, such as "Invite someone", "Add a passkey" or "Create a token") beside the controls, with 48px between the columns. Groups are separated by a thin top rule and 32px of padding; wide tables (the audit log) stack the intro above.

**Media grids.** Columns come from `repeat(auto-fill, minmax(…))`, never fixed counts: posters 2:3 at least 148px with 20px columns and 28px rows, 16:9 frames at least 208px, square photos at least 112px with 4px gaps grouped by month. Which one a library uses is its `layout`, decided by the server (docs/architecture.md, Libraries): `posters` and `series` draw posters, `frames` and Other files draw frames, `timeline` draws photos. Home shelves scroll sideways with snap points: posters 168px, frames 288px, photos 148px (132, 232 and 112px on phones, edge to edge with the gutter kept as scroll padding). Shelves with one or two tiles sit side by side instead of taking a row each. Home has one shelf per library, in the owner's order, each holding up to twelve of that library's newest items, so a burst in one library never pushes another off Home; photos are square tiles without captions. A show is pictured by its poster everywhere in its library (Home, the library page, the show's page): the server's choice, its first episode in that library with a picture.

**Facts.** Short facts (a file's video, audio and size; the VPN's server, protocol and kill switch; background jobs) read as columns: label above value, 32px between columns, wrapping on narrow screens. Longer fact lists use label-beside-value rows.

**Rhythm.** 8px inside a field, 16px between related items, 20px between form fields, 24px between a section heading and its content, 32px between page blocks, 48px between Home's sections.

## Elevation & Depth

Flat by default. Workspace surfaces have no shadows: a white panel on the zinc page is separated by its 1px divider border alone. Dialogs, menus, popovers and toasts keep Astryx's own elevation because they float above the page. Media tiles, episode frames and the small thumbnails in Downloads use one 1px inner hairline token (`--popcorn-media-hairline`: zinc-950 at 8% in light, white at 8% in dark) instead of a shadow so light photos keep an edge on a light page; the same hairline carries the hover tint.

**The No Nesting Rule.** A panel never holds boxed cards. Inside a panel, groups are separated by a thin divider or by space (the Downloads path is three flat steps under one rule, not three filled boxes).

## Shapes

Gently rounded and consistent: controls 8px, panels and dialogs 16px, media tiles and episode frames 10px, tags and badges 6px, progress tracks 4px. The player stage is 16px on desktop and square edge to edge on phones. Pills are reserved for small selected states (the current quick link).

## Components

### Buttons
- **Shape:** 8px corners, 44px tall (36px "small" on fine pointers only; 44px on touch), 8px by 12px padding, 14px semibold labels.
- **Primary:** Action Blue fill, Zinc Black label. One per view: the action that moves the task forward ("Add", "Approve", "Connect NordVPN", "Resume S01E02").
- **Secondary:** White Surface fill, 1px Zinc Divider border, Zinc Ink label.
- **Ghost:** no fill, Zinc Muted label, a quiet fill on hover. Used for "See all", back links (chevron plus parent name), "Show file details", "Open VPN settings". When a ghost button ends or starts a text row it shifts by its 12px padding so its label lines up with the text.
- **Focus:** a 2px Action Blue ring with a 3px offset on every control.

### Inputs / Fields
- **Style:** White Surface, 1px border, 8px corners, 44px tall, 16px text. Label above, optional description under the label, error under the field. A select opens from anywhere on its 44px field.
- **Numbers:** typed or stepped with the arrow keys; no stepper buttons (they cannot hold a 44px target). Units sit inside the field ("KB/s", "days").
- **Focus:** the border turns Action Blue.
- **Copyable values:** a value to paste elsewhere (an invite link, a device token, the owner recovery command `docker exec popcornstash popcornstash admin reset-owner`) sits in a Zinc Subtle monospace box with a "Copy" secondary button beside it, under a label that says what to do with it.

### Navigation
- **Sidebar rows:** 44px tall with 8px corners; an 18px Lucide line icon at stroke 1.5, then 8px, then the 14px medium label, the icon centred on the label's line. Icon and label share one colour: Zinc Muted at rest (Zinc Dark Muted in dark), Zinc Ink (Zinc Paper in dark) on the selected navigation fill when current. No blue, no filled "selected" glyphs, no colour per category. The Inbox count stays a neutral badge at the trailing edge.
- **One icon per destination, everywhere:** fixed places keep fixed glyphs: Home `house`, Other files `file-text`, Inbox `inbox`, Downloads `arrow-down-to-line` (as on Home's downloads notice), Settings `settings`. Each library shows the icon its owner chose from the curated list (docs/architecture.md, Libraries; the four starting libraries use Movies `film`, Shows `tv`, Home Videos `clapperboard`, Photos `image`), and that same glyph is its empty-state icon and the placeholder on its picture-less tiles. The fixed glyphs are not in the list, so no library can look like Home or Inbox. Fixed places are defined once (`web/src/app/navigation.ts`) and library icons map through one table of names to lucide-react components, so the sidebar, the phone menu, the quick links, Home and the Android app never disagree. A name the web does not know draws `folder`.
- **Library names in navigation:** one line; a long name ends in an ellipsis and keeps its full text in a tooltip and as the link's name. Names are the owner's words, shown as typed (no case change).
- **Phone:** the menu drawer is the sidebar itself. The quick links stack the same 18px icon over a 12px medium label in a 52px cell, with no side padding so "Downloads" keeps the whole cell; a library name longer than its cell ends in an ellipsis; the current one takes the same fill as the sidebar's.
- **Icons never stand alone on the web:** every icon sits beside or above its label and is hidden from screen readers, so links keep their plain names ("Inbox 1").

### Disclosure rows
A Collapsible trigger ("Use a WireGuard or OpenVPN file instead") is a 44px row with the chevron before a semibold label; the theme sets the height (`collapsible-trigger`), so every disclosure keeps the target.

### Status tag
- **Style:** Zinc Subtle fill, 6px corners, 12px Zinc Muted text, a 7px status dot. One line: in the narrow sidebar a long VPN name ends in an ellipsis, with the whole label in its tooltip (with the detail) and for screen readers. Only a long reason in a narrow column (System) may wrap.
- **Use:** the VPN in the sidebar foot and on Downloads, engine and passkey health in System, failed job counts.

### VPN status panel
The VPN page opens with a panel: a status dot and a title in words ("Protected · NordVPN · Netherlands", "Torrents are off on this server"; a list of names wraps between its parts, each dot staying with the part it introduces and dropping away at the start of a line), one detail sentence only when it adds a reason or says what still works, an explanation of what the tunnel covers, Reconnect and Remove, and the connection facts (public address, server, protocol for NordVPN, connected since, kill switch, and the provider only while the status line does not already name it). With STASH_ALLOW_NO_VPN the title is "No VPN: torrents use this network" with the reason (naming an unused saved VPN), without the tunnel explanation or Reconnect; facts are only the public address and the kill switch; the setup group says "Not used while torrents skip the VPN." and how to turn the VPN back on (recreate the container, never "restart": a restart keeps the old environment), and the Web downloads choice is greyed out under "There is no VPN on this server, so they use this network." The page names the variable twice: once as the reason, once in the way back. On Downloads and the share page the tag "No VPN: torrents use this network" is followed only by the reason ("STASH_ALLOW_NO_VPN is on. Your router must protect the traffic."; for members "The server is set up this way.").

### Media tile
- **Character:** the picture is the tile. Thumbnail at its shape's ratio with a 10px radius and the inner hairline, then the title (two lines, then truncated) and one metadata line ("2014 · 1 min", "Jun 13, 2024 · 12 s", "PDF · 592 B").
- **States:** a 4px Action Blue hairline on the image shows partial watch progress; hover tints the image; keyboard focus rings the whole tile. Files without a picture show a 22px icon on Zinc Subtle: the library's icon in a library, the file type's (music, document) in Other files.

### Episode rows
A 176px 16:9 frame, then the episode's name, then its state ("Watched", "Up next · 5 min left", or its runtime). Inside its show an untitled episode is "Episode 3"; the code ("S01E03") appears only above a real episode title. The show's Resume or Play button names the code, and the line under it names the episode.

### Rows in panels
Downloads, Inbox decisions, people, devices, passkeys and browsers are rows inside one white panel, separated by thin dividers: name, a metadata line, and actions at the trailing edge (a quiet "More" menu for rarer actions).

### Empty states
Astryx EmptyState in a flat panel: a 28px line icon (stroke 1.5, the same icon as the place's tile placeholder: a library's own icon), a short title ("Nothing in Kids yet"), one or two sentences that say how the space fills, and at most one action. Inside an existing panel the frame is dropped. The sentences add to the page's subtitle and the controls above rather than restating them (empty Inbox: "Everything new was sorted on its own."; empty Downloads: "Every file takes the same path into your library." above the three steps).

### Library pages
Each library page is its name as the h1 (exactly as the owner typed it) and one sentence that counts what is there ("214 videos", "12 shows, 140 episodes", "3,214 photos"), then the layout its `layout` names. The page frame, the toolbar and the tiles are the same whatever the library; only the arrangement changes.
- **Posters and frames (one-offs):** the search, sort and favourites toolbar over an infinite grid of poster or frame tiles. Metadata: a year and runtime for posters ("2014 · 1 h 52 min"), a date and runtime for frames ("Jun 13, 2024 · 12 s").
- **Series (episodes):** a poster grid of shows ("3 seasons" or "12 episodes" under each), then each show's page: its poster, Resume or Play naming the code, and episode rows grouped under a season picker. The back link names the library.
- **Timeline (photos):** months as `title` headings with their counts, square tiles 4px apart, and the Lightbox viewer.
- **Empty:** the library's icon, "Nothing in Kids yet", and how it fills: "New TV episodes land here on their own." when it takes something; "Files you accept from Inbox into Kids show up here." when it takes nothing. The owner also gets "Edit library" as the one action.
- **Other files** is not a library and has no settings: a frame grid with file-type placeholders, and the sentence "Music, documents and anything else. Only you and the person who added each file see it." for members, "Music, documents and anything else." for the owner.
- **Gone:** a library that is deleted or hidden while open shows the not-found page ("This library is not here any more." and "Go to Home").
- **On Home:** Continue watching first, then one shelf per library in the owner's order. A shelf's heading is the library's 18px icon (Zinc Muted, stroke 1.5) and its name in Section type, the count in Metadata, and "See all" at the trailing edge; a library with nothing yet keeps its heading with "Nothing here yet" in place of the shelf, so the order stays readable. When there are no libraries at all, the owner sees one EmptyState ("No libraries yet", "Make a library") and members see "Nothing has been shared with you yet."
- **In Downloads:** a sorted download names where it went, from the server's `destinations`: "In Kids" as a link to the library, "In Other files", or "2 waiting in Inbox"; a person never sees a library they cannot open.

### Library settings (owner)
Settings, Libraries (`/settings/libraries`) replaces the old Library tab. Its groups, each with the intro column:
- **Libraries:** "What the sidebar and Home show, in this order." with "Make a library" as the group action. One panel of rows: the library icon at 20px on a 40px Zinc Subtle square (8px corners), the name in Label type, and one metadata strip of three facts spaced 16px apart: what it is ("Series", "One-offs", "Photos"), what it takes ("Takes TV episodes", "Takes nothing new"), and who sees it ("Everyone", "Only Ana and Sam", "Only you"). The row opens the editor; a quiet More menu holds "Move up", "Move down" and "Delete". Moving applies at once and keeps focus on the moved row's More button; a row whose folder is missing adds the Warning dot and "Folder missing" to its strip.
- **Folders not in a library:** shown only when there are some. "Only you see what is in them. Make one a library to share it." Rows with the `folder` icon, the folder name in the code font with a trailing slash (`Kids/`), counts ("124 videos  12 photos"), and "Make a library" as a secondary button that opens the editor with that folder chosen and the name and type prefilled from the server's suggestion.
- **Sorting, Folder layout, Scan the folder, Trash:** as before, except that Folder layout lists only the fixed places (Other, Inbox, Downloads) and how a sorted file is laid out by kind inside any library's folder (`Title (Year)/`, `Show Name/Season 01/`, `2024/06/`).

**The editor** (`/settings/libraries/new`, `/settings/libraries/:id`) is a page inside Settings with a back link ("Libraries"), the library's name (or "New library") as its heading, the same grouped form, and one primary action at the end: "Make library" or "Save changes", with "Cancel" as a ghost beside it. Errors sit under the field the server names (`error.field`).
1. **Name and icon:** a TextInput ("Name", 40 characters) and the icon picker. Under them a preview of the sidebar row as it will look (icon and name on the selected navigation fill), so the owner sees the pair before saving.
2. **What it holds:** a RadioList "Videos" or "Photos"; for videos a second RadioList, "Series" ("Shows with seasons and episodes") or "One-offs" ("Films and clips that stand on their own"). On a library that holds files both are read-only with the reason "The type can change only while the library is empty."
3. **New files:** a CheckboxList "Sort these here" listing only the kinds that fit the chosen type (Series: TV episodes; One-offs: Movies, Camera videos; Photos: Photos). Each item's description says where that kind goes now: "Sorted into Films now", "Not sorted anywhere: these wait in Inbox", or, once checked, "Moves here from Films". Changing the type unchecks kinds that no longer fit.
4. **Folder:** when making a library, a RadioList "Make a new folder" (a code-font TextInput prefilled from the name) or "Use a folder that is already there" (a Selector of the folders not in a library, with counts), and the live preview "Files stay in Kids/. Nothing is moved." for an existing folder. When editing, a fact row "Folder  Kids/" and "Renaming the library keeps this folder."
5. **Who sees it:** a RadioList "Everyone" or "Only some people"; the second reveals a CheckboxList of members by name, with "Owners see every library." under it. Choosing nobody is allowed and reads "Only owners" in the list.
6. **Delete library** (editing only), last and apart: "Takes Kids out of the app. Its files stay in Kids/ and only you will see them." and a destructive secondary "Delete library", confirmed in an AlertDialog titled "Delete Kids?" with the same sentence, "Delete library" and "Cancel".

### Icon picker
An Astryx ToggleButtonGroup (`type="single"`) of icon-only ToggleButtons, one per name in the curated list, in the server's order (screen, photos, people and home, places, pastimes, general). Each button is a 44px square with the 20px Lucide glyph at stroke 1.5, 8px corners and 4px between buttons, wrapping in rows (8 per row in the 400px form column on desktop, as many as fit on phones). At rest the glyph is Zinc Muted; hover takes the quiet fill; the chosen icon sits on the selected navigation fill with a Zinc Ink glyph and a 1px Zinc Ink inner ring, so it reads as chosen without blue (The One Accent Rule); focus keeps the 2px Action Blue ring. Every button's label is the icon's plain name in sentence case ("Party popper", "Paw print"), shown as its tooltip and read by screen readers; arrow keys move between buttons, and Space or Enter chooses. A new library starts on `film` for videos and `image` for photos until the owner picks.

### Choosing a library in Inbox
Each Inbox row's destination is a Selector labelled "Library" that lists, with their icons, only the libraries the person can see that fit the file, then Other files; the suggestion is preselected, and the Selector is the row's one statement of where the file goes. The Edit form (title, year, show, season, episode) keeps the same Selector and its live preview is the one place that says it in words ("Will move to Kids"). When no library the person can see takes that kind, the Selector reads "Choose a library", the row's reason says "No library takes TV episodes yet.", and Accept waits for a choice. "Accept all suggestions" skips those rows and says "Choose a library for 2 files." Ignored files sit after the waiting ones under a closed disclosure, "Left where they are (3)", with the same row and Accept.

### Player and lightbox
Native `<video>` (HLS through hls.js wherever Media Source Extensions work; the browser's own HLS only where they do not) on the Stage Black stage; Play or "Resume from 0:31" as the primary button over the poster with "Start from the beginning" as a ghost below it; quality, audio and "Next episode" in a bar under the stage. A photo's page shows it on the same stage (24px of black around it on desktop, edge to edge on phones) with a full-screen button on the photo's own top-right corner; photos open in Astryx's Lightbox with arrow keys, a count, the date and camera, and "Download original". In the viewer the photo keeps the same inset on every side on desktop (the controls' 12px offset, their 44px size and 12px again), and runs edge to edge on phones; the close, previous and next controls are 44px Media Control discs, and an arrow at the first or last photo stays in place at 40% opacity.

### Drop overlay
The whole app accepts dropped files; a full-window overlay says where they will go.

## Do's and Don'ts

### Do:
- **Do** let posters, frames and photos provide the colour; keep the chrome zinc.
- **Do** show the real state of downloads and the VPN in words, with the dot as a second signal.
- **Do** keep one primary action per view, 44px targets and a visible 2px focus ring.
- **Do** say where a file will move before moving it ("Will move to Kids", "Will move to Other files").
- **Do** show each library with the icon and the name its owner chose, everywhere it appears, and nothing about a library a person cannot see.
- **Do** give each page one h1 and at most one sentence under it, wrapped so no word sits alone on the last line.
- **Do** put a settings group's action under its description, and a lone way forward under the page's sentence.

### Don't:
- **Don't** add gradients, glass, decorative charts, shadows on workspace panels or fake activity.
- **Don't** use status colours as action colours, or blue for navigation.
- **Don't** repeat a state, a name or a code twice in one place (a title and detail that say the same thing, "NordVPN · Netherlands · nl812 · Netherlands", "S01E01" above "The Lantern Keepers S01E01").
- **Don't** nest boxed cards inside a panel.
- **Don't** hide the VPN state when torrents exist.
- **Don't** show file paths to family members unless they open details.

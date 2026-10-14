---
name: PopcornStash
description: A quiet, private family library where the media itself provides the colour.
colors:
  accent: "oklch(54.6% 0.245 262.881)"
  accent-hover: "oklch(48.8% 0.243 264.376)"
  accent-pressed: "oklch(42.4% 0.199 265.638)"
  on-accent: "#ffffff"
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
    textColor: "{colors.on-accent}"
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

PopcornStash shares Popcorn SR's world: Tailwind zinc neutrals, Tailwind blue-600 as the single action and focus accent with white on blue, the platform system sans, flat panels, thin dividers, 44px controls, system light and dark, and reduced motion. Components come from Astryx (`@astryxdesign/core`) with a custom `popcorn` theme (`web/src/themes/popcorn`, rebuilt with `npm run theme:build`) that maps these tokens onto Astryx's token surface. Astryx components are used as they are wherever one exists; local components are limited to media pieces (poster grid, shelf, episode rows, photo timeline, player chrome) and small layout wrappers (page header, settings group, status tag).

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
- **Action Blue** (`accent`, Tailwind blue-600): fill of the one primary button per view, checked boxes and radios, on switches, focus rings (2px, 3px offset), the progress fill of downloads and the watched hairline on tiles, text selection tint, and the caret. One blue for all of them. Everything on Action Blue is white in both themes: button labels and icons, checkbox marks and radio dots, the on switch's knob, a badge's text, a spinner inside a busy button. Hover goes darker, never lighter, so white keeps 4.5:1: a primary button takes blue-700 on hover and blue-800 when pressed (`accent-hover`, `accent-pressed`), and a checked box, radio or on switch mixes 15% of Zinc Black (light) or blue-800 (dark) into its fill on hover. Pressing a checked box, radio or on switch lays Astryx's own press overlay over it, Zinc Black at 10% in light and white at 12% in dark. The theme leaves that overlay as it is: for a touch press Astryx resolves it on the row, which has no checked state, so it cannot be set for checked controls alone. In dark the fill lightens for the moment of the press, and the white mark or knob is then 4.3:1 (a mark needs 3:1). Over a picture (MediaTheme) Astryx turns the accent into the surface's own on-colour, and what sits on it takes the opposite one (Zinc Black on a dark picture); its hover and press steps are zinc as well (zinc-200 and zinc-300 on a dark picture, zinc-800 and zinc-700 on a light one), never Action Blue.
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
- **Banner tints:** a Banner (PasskeyNotice, load errors, form errors) is filled with its status hue at 18% (warning), 14% (error, info) or 16% (success) in light. In dark the tint is fainter, warning 7%, error and info 12%, success 8%, because a button in a Banner draws its Action Blue focus ring on the tint and the ring needs 3:1 there whether the Banner sits on the page or in a panel (`banner` in the theme, `--popcorn-banner-*`). The Banner's title, text and icon in the status colour say which kind it is (8:1 or more on the tint).

| Semantic | Light | Dark |
| --- | --- | --- |
| Page | Zinc Paper | Zinc Black |
| Surface | White Surface | Zinc Ink |
| Main text | Zinc Ink | Zinc Paper |
| Supporting text | Zinc Muted | Zinc Dark Muted |
| Border/divider | Zinc Divider | Zinc Dark Divider |
| Subtle fill | Zinc Subtle | Zinc Dark Divider |
| Action fill/focus | Action Blue | Action Blue |
| Action fill, hovered / pressed | blue-700 / blue-800 | blue-700 / blue-800 |
| On Action Blue | White | White |
| Track behind an Action Blue fill | zinc-300 | Zinc Black |
| Fill that holds controls | Zinc Subtle | Zinc Black |
| Link text | Link Blue (blue-600) | Link Blue (blue-400) |

**The One Accent Rule.** Blue marks the next action, focus and real progress. Navigation, tabs, filters and secondary actions stay neutral; the selected tab marker is ink, not blue.

**Action Blue, measured** (WCAG 2 contrast; `web/src/themes/popcorn/contrast.test.ts` and `extension/test/unit/contrast.test.ts` hold every row to its bar). White on blue needs 4.5:1; a thin blue mark (a focus ring, the caret, a progress fill, a checked box's or an on switch's edge) needs 3:1 against what it sits on.

| Pairing | Light | Dark |
| --- | --- | --- |
| White on Action Blue (labels, marks, knob, badge text) | 5.3:1 | 5.3:1 |
| White on a hovered primary (blue-700) / a pressed one (blue-800) | 6.8:1 / 8.8:1 | 6.8:1 / 8.8:1 |
| White on a hovered checked box, radio or on switch | 6.6:1 | 5.7:1 |
| White on a pressed checked box, radio or on switch (Astryx's press overlay) | 6.1:1 | 4.3:1 |
| Action Blue against the page | 5.0:1 | 3.8:1 |
| Action Blue against a panel or dialog | 5.3:1 | 3.4:1 |
| A hovered checked control against the page / a panel | 6.3:1 / 6.6:1 | 3.5:1 / 3.1:1 |
| A hovered checked control against its rest state | 1.26:1 | 1.08:1 |
| Focus ring against the fill that holds the Inbox edit form | 4.8:1 | 3.8:1 |
| Focus ring on a Banner's tint over the page / a panel: warning | 4.4:1 / 4.6:1 | 3.5:1 / 3.0:1 |
| Focus ring on a Banner's tint over the page / a panel: error | 4.1:1 / 4.3:1 | 3.5:1 / 3.0:1 |
| Focus ring on a Banner's tint over the page / a panel: info | 4.1:1 / 4.3:1 | 3.5:1 / 3.1:1 |
| Focus ring on a Banner's tint over the page / a panel: success | 4.3:1 / 4.5:1 | 3.5:1 / 3.0:1 |
| Download progress fill against its track | 3.6:1 | 3.8:1 |
| Watched hairline on its Stage Black track | 4.0:1 | 4.0:1 |
| Selected text on the selection tint over a panel | 14.0:1 | 13.4:1 |

Four things changed with the darker blue so the thin marks keep 3:1. In dark the track behind a download's progress fill is Zinc Black, a groove in the panel (it was zinc-700: 2.0:1), and the Inbox edit form sits on Zinc Black rather than Zinc Dark Divider, where its focus rings were 2.8:1 (`--popcorn-progress-track`, `--popcorn-control-well`). In dark a Banner's tint is fainter (see Banner tints above): at the old strength the focus ring of PasskeyNotice's "Open <host>" was 2.6:1 on the page, and of SharePage's "Open VPN settings" 2.7:1 in its panel. The watched hairline's track on tiles and episode frames is opaque Stage Black: the translucent one let a bright or mid-grey frame show through, at 1.0:1 to 3.0:1. The extension does the same with its own tokens: its progress track in dark is halfway between Zinc Ink and Zinc Dark Divider (3.1:1; Zinc Dark Divider was 2.8:1), a notice that holds a button is Zinc Ink in dark (3.4:1), and an error notice's red fill is 12% in dark (3.5:1 on the page, 3.05:1 on a panel; at 14% it was 2.98:1 on a panel).

In dark the empty part of a download's progress bar reads faintly: the web's Zinc Black track is 1.12:1 against the Zinc Ink panel it sits in. That is the most it can be there: the blue fill is 3.4:1 against a dark panel, so a track that keeps the fill at 3:1 can differ from the panel by about 1.12:1 at most, lighter or darker. The percentage beside every bar carries the value. The extension's bars sit on the Zinc Black page, which leaves more room, so its track is lighter than Zinc Ink and shows at 1.22:1.

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

**File names** ("Saltwater.Choir.2022.1080p.WEB.webm") in Downloads and the Inbox wrap after their dots, underscores and hyphens, never mid-word; only a long run without separators breaks where it must. **Web addresses and paths** wrap after their slashes, dots, hyphens and other separators, never inside "https://" and never with a line that starts with a separator ("…/live-at-" then "the-pier", not "…live-at-the" then "-pier"); one inside a sentence ("enter this address: http://stash.local:8788") moves to the next line whole and breaks only when it is longer than a line. **Short facts stay whole:** a date ("Oct 14, 2026") never splits inside a sentence, and a folder's name in a list ("Grandma's Slides/") is shown whole: a short one keeps its line, and one too long for the row wraps at its spaces and separators instead of ending in an ellipsis, because a phone shows no tooltip. On phones a folder row's icon sits at the top, beside the name and its counts, with "Make a library" under them.

**Dates and times** of photos and home videos are the camera's clock as written (`taken_local`), the same for every viewer in every time zone ("Jun 14, 2024, 8:18 PM" is what the camera showed); a month heading is the server's month as written. Times the server records (added, last used, expires) are moments, shown in the viewer's own time zone.

**The Sentence Case Rule.** Every label, heading and button is sentence case. No uppercase eyebrows, no letter-spaced labels, no em dashes.

**The Say It Once Rule.** A state is named once per place. A status line has a short name and, only when it adds something, a reason and what still works ("Torrents off" and "Web links and uploads still work.", not "Torrents unavailable" and "Torrents are turned off on this server."). One word per state across the app: "off" when torrents are off on purpose, "unavailable" when the engine is not answering ("Torrents unavailable: the torrent engine is not answering"; "Not answering" for the engine in System), "paused" while the VPN connects, "blocked" when the VPN fails. A VPN reads "Protected · NordVPN · Netherlands", each part once; when it wraps, it wraps between parts and no line starts or ends with a dot. Facts under a status line never repeat it (no "Provider: NordVPN" under "Protected · NordVPN · Netherlands"), a value never repeats its label ("ffmpeg: 6.1.1", not "ffmpeg: ffmpeg version 6.1.1"), and a form's live preview is the one place that says where a file goes ("Will move to Other files", not also "Goes to Other files" above it). What is missing is said once too: while a field shows its own error ("Enter the episode number."), the preview leaves out its hint ("Add the episode number to see where it goes."). A status being checked is its label alone ("Checking VPN", no detail), and one that could not be read says what still works ("VPN status unavailable" and "Web links and uploads still work."). When the server does not answer at all, the page says so once and leaves the VPN line out, because nothing works then.

## Layout

**Shell.** Desktop: a sticky 228px sidebar (brand; Home, then every library the person can see in the owner's order, each with its own icon, then Other files; Inbox with its count, Downloads; Settings; a foot with the VPN tag, the person's name and Sign out) and a bounded main column (max 1320px, padding `clamp(24px, 5vw, 64px)`, more at the bottom). When the libraries do not fit the window height, the middle of the sidebar scrolls and the brand and the foot stay put. Under 800px the sidebar becomes a compact top bar with a menu drawer (the same sidebar, icons included), and a row of quick links (Home, the first three libraries the person can see, Downloads), each its icon above its label, sits on the 20px page gutter in equal cells 4px apart (fewer libraries, fewer cells), its current pill aligned with the content edge. See Navigation under Components.

**Page header.** One h1, at most one sentence under it, then 32px to the content. Trailing actions are rare (Inbox's "Accept all suggestions"); when the action is the page's only way forward ("Go to Home" on a not-found page) it sits under the sentence.

**Settings.** The Settings h1, then a tab strip (one tab per section), then the section's one-sentence lead. Under 640px an owner's six tabs would scroll sideways and hide Libraries and System, so they become one full-width section picker (Astryx Selector) that lists every section; a member's two tabs fit and stay tabs. Every section keeps its own address (`/settings/vpn`). Each section keeps its h2 for the document outline but hides it, because the selected tab already shows the name. Sections are groups: on screens 1100px and wider, a 200-260px intro column (h3, one or two sentences, and the group's action, such as "Invite someone", "Add a passkey" or "Create a token") beside the controls, with 48px between the columns. Groups are separated by a thin top rule and 32px of padding; wide tables (the audit log) stack the intro above.

**Media grids.** Columns come from `repeat(auto-fill, minmax(…))`, never fixed counts: posters 2:3 at least 148px with 20px columns and 28px rows, 16:9 frames at least 208px, square photos at least 112px with 4px gaps grouped by month. Which one a library uses is its `layout`, decided by the server (docs/architecture.md, Libraries): `posters` and `series` draw posters, `frames` and Other files draw frames, `timeline` draws photos. Home shelves scroll sideways with snap points: posters 168px, frames 288px, photos 148px (132, 232 and 112px on phones, edge to edge with the gutter kept as scroll padding). Shelves with one or two tiles sit side by side instead of taking a row each. Home has one shelf per library, in the owner's order, each holding up to twelve of that library's newest items, so a burst in one library never pushes another off Home; a library of series holds one tile per show instead (up to twelve, the most recently added first, from the server's `shows`), so its shelf agrees with its count; photos are square tiles without captions. A show is pictured by its poster everywhere in its library (Home, the library page, the show's page): the server's choice, its first episode in that library with a picture.

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
- **Primary:** Action Blue fill, white label and icon (5.3:1). Hover goes darker to blue-700 and a press to blue-800 (6.8:1 and 8.8:1), in both themes; Astryx's default overlay is white in dark, which would lighten the blue under the label, so the theme sets the primary's own (`button`, `variant:primary`). One per view: the action that moves the task forward ("Add", "Approve", "Connect NordVPN", "Resume S01E02").
- **Secondary:** White Surface fill, 1px Zinc Divider border, Zinc Ink label.
- **Ghost:** no fill, Zinc Muted label, a quiet fill on hover. Used for "See all", back links (chevron plus parent name), "Show file details", "Open VPN settings". When a ghost button ends or starts a text row it shifts by its 12px padding so its label lines up with the text.
- **Focus:** a 2px Action Blue ring with a 3px offset on every control.

### Inputs / Fields
- **Style:** White Surface, 1px border, 8px corners, 44px tall, 16px text. Label above, optional description under the label, error under the field. A select opens from anywhere on its 44px field.
- **Numbers:** typed or stepped with the arrow keys; no stepper buttons (they cannot hold a 44px target). Units sit inside the field ("KB/s", "days").
- **Focus:** the border turns Action Blue.
- **Switches:** on is the Action Blue track with a white knob in both themes (5.3:1); hovered, the track goes darker in both themes, 15% of Zinc Black in light (knob 6.6:1) and of blue-800 in dark (knob 5.7:1, track still 3:1 against a dark panel). Off reads on its own, without the blue to compare with: a Zinc Subtle track (Zinc Dark Divider in dark) inside a 2px ring, with a knob in the ring's colour, Zinc Status in light and Zinc Dark Muted in dark. Ring and knob are 3:1 or more against the page, a panel and the track (measured: ring 4.6:1 on the light page and 7.6:1 on the dark one, knob 4.4:1 and 5.7:1 on its track), and in dark the knob is lighter than its track. Set once in the theme (`switch`, `switch-thumb`), so every switch has it.
- **Checkboxes and radios:** checked is the Action Blue fill with a white mark or dot (5.3:1), tinted darker on hover like an on switch (6.6:1 in light, 5.7:1 in dark). Unchecked has the off switch's edge: a 1px border in Zinc Status (light) or Zinc Dark Muted (dark), 4.6:1 and 7.6:1 against the page, so an unchosen radio, an unchecked box and an off switch on one page read alike and all reach 3:1 (WCAG 1.4.11); the Android app draws them in its `fieldOutline` (zinc-500) for the same reason. Set once in the theme (`checkbox-indicator`, `radio-indicator`, and their `checked` state for the hover); disabled ones keep Astryx's faint border.
- **Choice lists:** a CheckboxList row is bare like a RadioList row: no inline inset and no checked fill, so checkboxes sit in the radios' column and a choice shows by its own mark.
- **Copyable values:** a value to paste elsewhere (an invite link, a device token, the owner recovery command `docker exec popcornstash popcornstash admin reset-owner`) sits in a Zinc Subtle monospace box with a "Copy" secondary button beside it, under a label that says what to do with it.

### Navigation
- **Sidebar rows:** 44px tall with 8px corners; an 18px Lucide line icon at stroke 1.5, then 8px, then the 14px medium label, the icon centred on the label's line. Icon and label share one colour: Zinc Muted at rest (Zinc Dark Muted in dark), Zinc Ink (Zinc Paper in dark) on the selected navigation fill when current. The current destination is the place the page belongs to: its address or anything under it (a show's page is inside its library), and on an item's page (`/item/:id`) the item's library, Other files, or Inbox for a file waiting there; an item outside all of them (a folder that is not a library, the trash) marks nothing. No blue, no filled "selected" glyphs, no colour per category. The Inbox count stays a neutral badge at the trailing edge.
- **One icon per destination, everywhere:** fixed places keep fixed glyphs: Home `house`, Other files `file-text`, Inbox `inbox`, Downloads `arrow-down-to-line` (as on Home's downloads notice), Settings `settings`. Each library shows the icon its owner chose from the curated list (docs/architecture.md, Libraries; the four starting libraries use Movies `film`, Shows `tv`, Home Videos `clapperboard`, Photos `image`), and that same glyph is its empty-state icon and the placeholder on its picture-less tiles. The fixed glyphs are not in the list, so no library can look like Home or Inbox, and neither is `folder`, which belongs to folders that are not a library. Fixed places are defined once (`web/src/app/navigation.ts`) and library icons map through one table of names to lucide-react components, so the sidebar, the phone menu, the quick links, Home and the Android app never disagree. A name the web does not know draws `folder`.
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
- **States:** a 4px Action Blue hairline on an opaque Stage Black track along the image's bottom edge shows partial watch progress (4.0:1 whatever the frame); hover tints the image; keyboard focus rings the whole tile. Files without a picture show a 22px icon on Zinc Subtle: the library's icon in a library, the file type's (music, document) in Other files.

### Episode rows
A 176px 16:9 frame, then the episode's name, then its state ("Watched", "Up next · 5 min left", or its runtime). Inside its show an untitled episode is "Episode 3"; the code ("S01E03") appears only above a real episode title. The show's Resume or Play button names the code, and the line under it names the episode.

### Rows in panels
Downloads, Inbox decisions, people, devices, passkeys and browsers are rows inside one white panel, separated by thin dividers: name, a metadata line, and actions at the trailing edge (a quiet "More" menu for rarer actions).

### Devices
Settings, Devices lists the connected devices, then how to connect each kind, the Android app first: "Android app" (what it does in one sentence; where it comes from: for an owner "It is not in a store, so you build it once." with a link to how, for anyone else "Get the app from whoever runs this server."; enter this server's address; then what always works first, "Choose Pair with a code and approve it on the page it opens, or here.", and the passkey only as what comes once the server is set up for it, with a link to how for an owner), then "Chrome extension", and last "Share without the app" (install PopcornStash from Chrome's menu, which adds it to the phone's share sheet). The steps never offer a way in that a default server does not allow. The share page's "On Android" note says the same in one paragraph. Group headings in the intro column are balanced (`text-wrap: balance`), and short enough to sit on one line there, so none leaves one word on a line of its own.

### Empty states
Astryx EmptyState in a flat panel: a 28px line icon (stroke 1.5, the same icon as the place's tile placeholder: a library's own icon), a short title ("Nothing in Kids yet"), one or two sentences that say how the space fills, and at most one action. Inside an existing panel the frame is dropped. The sentences add to the page's subtitle and the controls above rather than restating them (empty Inbox: "Everything new was sorted on its own."; empty Downloads: "Every file takes the same path into your library." above the three steps).

### Library pages
Each library page is its name as the h1 (exactly as the owner typed it) and one sentence that counts what is there ("214 videos", "12 shows, 140 episodes", "3,214 photos"), then the layout its `layout` names. The page frame, the toolbar and the tiles are the same whatever the library; only the arrangement changes.
- **Posters and frames (one-offs):** the search, sort and favourites toolbar over an infinite grid of poster or frame tiles. Metadata: a year and runtime for posters ("2014 · 1 h 52 min"), a date and runtime for frames ("Jun 13, 2024 · 12 s").
- **Series (episodes):** a poster grid of shows ("3 seasons" or "12 episodes" under each), then each show's page: its poster, Resume or Play naming the code, and episode rows grouped under a season picker. The back link names the library.
- **Timeline (photos):** months as `title` headings with their counts, square tiles 4px apart, and the Lightbox viewer.
- **Empty:** the library's icon, "Nothing in Kids yet", and how it fills: "New TV episodes land here on their own." when it takes something; "Files you accept from Inbox into Kids show up here." when it takes nothing. The owner also gets "Edit library" as the one action.
- **Other files** is not a library and has no settings: a frame grid with file-type placeholders, and one sentence under the title: "Music, documents and anything else. Only you and the owners see these." for members (everything a member sees here is a file they added), "Music, documents and anything else. Each file is seen by the person who added it and the owners." for owners.
- **Gone:** a library that is deleted or hidden while open shows the not-found page ("This library is not here any more." and "Go to Home").
- **On Home:** Continue watching first, then one shelf per library in the owner's order. A shelf's heading is the library's 18px icon (Zinc Muted, stroke 1.5) and its name in Section type (16px; on Home it is an h2 in the outline, drawn at Section size, so web and Android agree), the count in Metadata, and "See all" at the trailing edge; a library with nothing yet keeps its heading with "Nothing here yet" in place of the shelf, so the order stays readable. A series shelf has a poster tile for every show (up to twelve) with one line under the name: "New: S01E03" for one episode added in the last 30 days, "6 new episodes" for several, else its count ("5 episodes"), so "3 shows, 18 episodes" shows three shows. Consecutive short shelves (two tiles or fewer, or nothing yet) share one wrapping row, so the order still reads left to right; a short shelf with no short neighbour keeps the full width, so its "See all" lines up with the others. Other files, when the person has any, ends Home as a heading of the same kind (its `file-text` icon, "Other files", the count, "See all") over one quiet sentence, "Music, documents and anything else.": it is not a library and has no shelf. When there are no libraries at all, the owner sees one EmptyState ("No libraries yet", "Make a library") and members see "Nothing has been shared with you yet."
- **In Downloads:** a sorted download names where it went, from the server's `destinations`: "In Kids" as a link to the library, "In Other files", or "2 waiting in Inbox"; a person never sees a library they cannot open. A sorted download with no destination left to show (its files went to a library the person cannot see, or were deleted since) says only "Sorted". Members see their own downloads in every state and someone else's only once it is sorted into something they can see, so Downloads never names a file on its way to a library they cannot see. A row that leaves a person's view (the server's `removed` event) disappears without a message.

### Library settings (owner)
Settings, Libraries (`/settings/libraries`) replaces the old Library tab. Its groups, each with the intro column:
- **Libraries:** "What the sidebar and Home show, in this order." with "Make a library" as the group action (the section's one primary button). One panel of rows: the library icon at 20px on a 40px Zinc Subtle square (8px corners), the name in Label type, and one metadata strip of three facts spaced 16px apart: what it is ("Series", "One-offs", "Photos"), what it takes ("Takes TV episodes", "Takes nothing new"), and who sees it ("Everyone", "Only Ana and Sam", "Only owners"). The row opens the editor; a quiet More menu holds "Move up", "Move down" and "Delete". Moving applies at once and keeps focus on the moved row's More button; a row whose folder is missing or read-only adds the Warning dot and "Folder missing" or "Folder read-only" to its strip.
- **Folders not in a library:** shown only when there are some. "Only owners see what is in them. Make one a library to share it." Rows with the `folder` icon, the folder name in the code font with a trailing slash (`Kids/`) on one line, counts ("124 videos  12 photos"), and "Make a library" as a secondary button (at the row's trailing edge; on a phone under the counts, so the name keeps the row's width) that opens the editor with that folder chosen, the name and type prefilled from the server's suggestion, and "Who sees it" on "Only some people" with nobody chosen.
- **Sorting, Folder layout, Scan the folder, Trash:** as before, except that Folder layout lists only the fixed places (Other, Inbox, Downloads) and how a sorted file is laid out by kind inside any library's folder (`Title (Year)/`, `Show Name/Season 01/`, `2024/06/`), and that a trashed file its library would no longer show has no Restore and says why in its metadata line, in the server's words ("Kids does not show this kind of file any more, so it stays in the trash.").

**The editor** (`/settings/libraries/new`, `/settings/libraries/:id`) is a page inside Settings with a back link ("Libraries"), the library's name (or "New library") as its heading, the same grouped form, and one primary action at the end: "Make library" or "Save changes", with "Cancel" as a ghost beside it. Errors sit under the field the server names (`error.field`).
1. **Name and icon:** a TextInput ("Name", 40 characters) and the icon picker. Under them, captioned "In the sidebar", a preview of the sidebar row as it will look (icon and name on the selected navigation fill), so the owner sees the pair before saving.
2. **What it holds:** a RadioList "Videos" or "Photos"; for videos a second RadioList, "Series" ("Shows with seasons and episodes") or "One-offs" ("Films and clips that stand on their own"). On a library that holds files the type is a fact, not a choice: the group says "The type can change only while the library is empty." and shows a fact row "Holds  Videos, arranged as series" (or "Videos, arranged as one-offs", "Photos") in body text at full contrast, like the folder's fact row, never greyed radios the owner can barely read. Android shows it the same way.
3. **New files:** a CheckboxList "Sort these here" listing only the kinds that fit the chosen type (Series: TV episodes; One-offs: Movies, Camera videos; Photos: Photos). Each item's description says where that kind goes now: "Sorted into Films now", "Not sorted anywhere: these wait in Inbox", or, once checked, "Moves here from Films" ("Sorted here now" for a kind this library already takes, "Sorted here from now on" for one nobody took). Changing the type unchecks kinds that no longer fit. Saving a library that takes a kind from another asks first, in an AlertDialog: "Sort TV episodes into Kids?", "New TV episodes will go to Kids instead of Shows. Files already sorted stay where they are.", with "Move and save" (or "Move and make library") as its primary action.
4. **Folder:** when making a library, a RadioList "Make a new folder" (a TextInput, "Folder name", that follows the name until the owner edits it) or "Use a folder that is already there" (a Selector of the folders not in a library, with counts), and the live preview "Files stay in Kids/. Nothing is moved." for an existing folder. When the chosen type will not show some of that folder's files (from the folder's counts: photos in a Videos library, other files anywhere), the preview adds one sentence: "12 photos and 3 other files there will not show in this library." When editing, a fact row "Folder  Kids/" and "Renaming the library keeps this folder."
5. **Who sees it:** a RadioList "Everyone" or "Only some people"; the second reveals a CheckboxList of members by name, with "Owners see every library." under it. Choosing nobody is allowed and reads "Only owners" in the list. A library with a new folder starts on "Everyone" (there is nothing in it yet). A library made from a folder that is already there starts on "Only some people" with nobody chosen, because its files were seen by owners only until now; the group's live preview then says who will see what is there: "Only owners will see its 124 videos.", "Everyone will see its 124 videos.", "Ana and Sam will see its 124 videos." (counting only what the chosen type shows).
6. **Delete library** (editing only), last and apart: "Takes Kids out of the app. Its files stay in Kids/ and only owners will see them." and a destructive secondary "Delete library", confirmed in an AlertDialog titled "Delete Kids?" with the same sentence, "Delete library" and "Cancel".

### Icon picker
An Astryx ToggleButtonGroup (`type="single"`) of icon-only ToggleButtons, one per name in the curated list, in the server's order (screen, photos, people and home, places, pastimes, general). Each button is a 44px square with the 20px Lucide glyph at stroke 1.5, 8px corners and 4px between buttons, wrapping in rows (8 per row in the 400px form column on desktop, as many as fit on phones). At rest the glyph is Zinc Muted; hover takes the quiet fill; the chosen icon sits on the selected navigation fill with a Zinc Ink glyph and a 1px Zinc Ink inner ring, so it reads as chosen without blue (The One Accent Rule); focus keeps the 2px Action Blue ring. Every button's label is the icon's plain name in sentence case ("Party popper", "Paw print"), the same name on the web and Android (docs/architecture.md, Icons), shown as its tooltip and read by screen readers; arrow keys move between buttons, and Space or Enter chooses. A new library starts on `film` for videos and `image` for photos until the owner picks.

### Choosing a library in Inbox
Each Inbox row's destination is a Selector labelled "Library" (the label read by screen readers; the row's file name and reason already frame it) that lists, with their icons, only the libraries the person can see that fit the file, then Other files after a divider; the suggestion is preselected, and the Selector is the row's one statement of where the file goes. It leads the row's action line, 280px wide (the full width on phones), followed by Accept, Edit and Ignore. With one place to go (a document can only go to Other files) the Selector is read-only. When the chosen library is a series and the file has no show or episode number yet, Accept reads "Add details" and opens the Edit form instead of failing. When someone else added the file and the person sees it only because a library they see takes that kind, the Selector lists that one library and nothing else: sending it anywhere else is for the person who added it and the owners. The API does not say who added a file, so for members any row whose suggestion names a library is treated this way (docs/architecture.md section 10). The Edit form (title, year, show, season, episode) keeps the same Selector and its live preview is the one place that says it in words ("Will move to Kids"). Accept sends the library the row shows, never a later guess.

The row's reason line comes from the server's `waiting`: the classifier's sentence when it is unsure ("Not sure this is a movie."); "No library takes TV episodes yet." for owners, or "Choose a library for this file." for members, when no library the person can see takes that kind (the Selector then reads "Choose a library" and Accept waits for a choice); "The library's folder is not available." when the library's folder is missing or read-only; and no reason line when the suggestion only needs accepting. "Accept all suggestions" skips rows without a library and says "Choose a library for 2 files." Ignored files sit after the waiting ones under a closed disclosure, "Left where they are (3)", with the same row and Accept.

### Organize
Owners move a file into a library, or to another one, from the item's More menu ("Organize into library", or "Move to another library" for a file already in one). The dialog says where the file is now, then holds the same Library selector, details and live preview as the Inbox edit form; its primary action names the destination ("Move to Home Videos").

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

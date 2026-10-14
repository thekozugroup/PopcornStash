# PopcornStash for Android: design system

The Android app lives in the same world as the web app (`../DESIGN.md`): zinc chrome, Action Blue only where the next action is, colour from the posters, frames and photos, flat surfaces, thin dividers, the system sans. Material 3 is the base for structure and behaviour (insets, system Back, sheets, tooltips, ripples, accessibility); the Popcorn tokens theme it. There is no dynamic (wallpaper) colour: the library's pictures provide the colour, and a wallpaper tint would compete with them.

Every value below is a token in `app/src/main/kotlin/com/thekozugroup/popcornstash/ui/theme/`. To try a change, edit the token, run `./gradlew recordRoborazziDebug` and look at `app/src/test/screenshots/`.

## Tokens

### Colour (`Color.kt`)

| Token | Light | Dark | Used for |
| --- | --- | --- | --- |
| `page` | zinc-50 `#FAFAFA` | zinc-950 `#09090B` | the canvas behind every page, the top app bar |
| `surface` | white `#FFFFFF` | zinc-900 `#18181B` | toolbar, sheet, panels, the ring around the toolbar's selected square and its badge |
| `subtle` | zinc-100 `#F4F4F5` | zinc-800 `#27272A` | media placeholders, avatar |
| `divider` | zinc-200 `#E4E4E7` | zinc-800 `#27272A` | panel borders, row dividers, the scrolled top bar's rule |
| `fieldOutline` | zinc-500 `#71717B` | zinc-500 `#71717B` | the edge of a text field (3:1 or more on the page) |
| `text` | zinc-900 `#18181B` | zinc-50 `#FAFAFA` | main text and icons |
| `textMuted` | zinc-600 `#52525C` | zinc-400 `#9F9FA9` | metadata, unselected toolbar icons, ghost buttons |
| `accent` (Action Blue) | blue-600 `#155DFC` | blue-600 `#155DFC` | the selected toolbar square, primary buttons, progress, the seek bar, the focused field edge, the pull-to-refresh arc: one blue for all of them |
| `onAccent` | white `#FFFFFF` | white `#FFFFFF` | words, icons, the busy ring and press ripples on Action Blue (5.25:1) |
| `link` | blue-600 `#155DFC` | blue-400 `#51A2FF` | inline links only |
| `progressTrack` | zinc-100 `#F4F4F5` | zinc-900 `#18181B` | the track under a download's progress |
| `mediaHairline` | zinc-950 at 8% | white at 8% | the 1dp inner edge on tiles |
| `mediaProgressTrack` | zinc-950 at 90% | zinc-950 at 90% | the track under the watched bar on a picture |
| `glass` | white at 80% | zinc-900 at 80% | the floating toolbar's tint over its blur |
| `glassEdge` | zinc-950 at 10% | white at 10% | the glass toolbar's 1dp edge |
| `stage` | black | black | photo viewer and player |
| `mediaControl` | zinc-950 at 62% | zinc-950 at 62% | discs behind controls laid over a picture |
| `connected` / `warning` / `error` | emerald-700, amber-700, red-700 | emerald-400, amber-400, red-400 | status words; the dot beside them uses the 500 shade |

Material roles: `primary` = accent, `onPrimary` = onAccent, `background` and `surface` = page, every `surfaceContainer*` = surface, `onSurface` = text, `onSurfaceVariant` = textMuted, `outlineVariant` = divider, `outline` = zinc-300 / zinc-700, `secondaryContainer` = subtle, `inverseSurface` = zinc-900 / zinc-50 (tooltips, snackbars), `inversePrimary` = blue-400 / blue-600 (blue words on the inverse surface, such as a snackbar action, take the link blue for that surface: 6.72:1 on zinc-900, 5.03:1 on zinc-50), `error` = red-700 / red-400.

#### Action Blue, measured

Action Blue is blue-600 in both themes so that everything on it can be white: white on blue-500 was 3.76:1, under the 4.5:1 a button's words need; on blue-600 it is 5.25:1. Each pairing below is the WCAG 2 contrast ratio, worked out from the token values with sRGB blending (as Skia draws), and the glass rows checked against the rendered screenshots.

| Pairing | Light | Dark |
| --- | --- | --- |
| White on Action Blue: button words and icons, the busy ring, the toolbar's selected icon | 5.25:1 | 5.25:1 |
| The same under a press, keyboard focus or drag (an 8% white ripple, `OnAccentRipple`) | 4.61:1 | 4.61:1 |
| The same under a mouse hover (4% white) | 4.92:1 | 4.92:1 |
| Action Blue on `page`: buttons, the focused field edge | 5.03:1 | 3.79:1 |
| Action Blue on `surface`: buttons in sheets and dialogs, the pull-to-refresh arc on its disc | 5.25:1 | 3.38:1 |
| Action Blue on `progressTrack`: a download's progress | 4.77:1 | 3.38:1 |
| Action Blue on `mediaProgressTrack` over a picture: the watched bar | 3.04:1 over white, 3.81:1 over black | the same |
| Action Blue on its `surface` ring: the toolbar's selected square, on glass or on the opaque bar | 5.25:1 | 3.38:1 |

What changed to keep those numbers, and what is still short:

- **Press ripples.** Material's ripple is 10%, which would leave white words at 4.45:1 while pressed; presses on Action Blue use 8% instead (4% for a mouse hover). There is no darker hover as on the web: on a phone the press is the state that shows. Material draws the hover or focus layer and the press together, so two moments fall short while a key or button is held: hovering with a mouse and clicking adds up to 11.7% white (4.33:1), and pressing a button that has keyboard or D-pad focus adds up to 15.4% white (4.06:1). Keeping every combination at 4.5:1 would need 9.4% white in total: next to an 8% press, focus would be about 1.5%, too faint to see, and an even split (about 5% and 4%) would make both the press and the focus hard to see on the blue. A visible focus matters more than the moment of the press.
- **A download's progress (dark).** The track was `subtle` (zinc-800), where blue-600 is 2.84:1; it is now `progressTrack`, zinc-900 in dark (3.38:1). Light keeps zinc-100.
- **The watched bar.** Its zinc-950 track was 40%, so over a bright picture the blue was 1.93:1 (blue-500 was 1.38:1); at 90% it is 3.04:1 over white and 3.81:1 over black.
- **The player's seek bar** (always dark, on a `mediaControl` plate over the video). Over a white frame the plate turns mid grey and the blue thumb was 1.09:1 against it (blue-500 was 1.51:1), so the 16dp thumb now has a 2dp `onMedia` ring: 5.45:1 against the plate over a white frame, 19.45:1 over black. The played part against the unplayed track is 1.0 to 1.7:1 (1.64:1 over a white frame, 1.69:1 over black): the track is lighter than the blue over bright frames and darker over dark ones, so over a mid-grey frame the two match and the played part disappears into the track (blue-500 had the same problem, 1.0 to 2.4:1). A track that held 3:1 would have to be nearly opaque white or black and would hide the buffered part, so the position is carried by the ringed thumb, 5.45:1 or more against the plate over any frame, with the times beside the bar.
- **The toolbar's selected square.** On glass the blue square's neighbour is whatever the glass shows: over a pure white picture the dark glass turns grey (`#464649`) and the square was 1.79:1 against it (blue-500 would have been 2.49:1, so the glass causes it, not the darker blue); over black the light glass is `#CCCCCC` and the square was 3.27:1. The square now has a 2dp `surface` ring outside its edge (`ToolbarTokens.indicatorRing`) that slides with it, as the badge has, so its neighbour is always white or zinc-900: 5.25:1 in light and 3.38:1 in dark, on any picture and on the opaque bar. Making the dark glass opaque enough to hold 3:1 instead (zinc-950 at 90%) would have cost most of the translucency.
- The launcher icon is the white Lucide `popcorn` glyph on Action Blue (`res/values/colors.xml`, `ic_launcher_foreground.xml`), 5.25:1.
- Links keep their own tokens (blue-600 in light, blue-400 in dark). Checkboxes, radios and the icon picker stay neutral (`text`), because a choice is not an action.

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

### Spacing (`Tokens.kt`)

`4 · 8 · 12 · 16 · 20 · 24 · 32 · 40` dp. The page gutter is 20dp (the web phone gutter). 8dp inside a group, 16dp between related items, 24dp under a section heading, 32dp between page blocks, 40dp between Home's sections. Shelves scroll edge to edge with the gutter kept as content padding.

### Shape (`Shape.kt`)

One scale: tags 6dp, controls 8dp, media 10dp, toolbar squares 12dp, panels, sheets and the toolbar 16dp, the avatar a circle. Library icon squares in lists are 40dp with 8dp corners; icon picker cells are 48dp with 8dp corners. The toolbar's outer radius is always the square's radius plus the toolbar padding (12 + 4 = 16), so the selected square sits concentric in the corner squares.

### Elevation

Flat. Pages, panels and tiles have no shadow; tiles get the 1dp `mediaHairline` instead. Only layers that float above the page cast one: the toolbar and Material's own sheet scrim and tooltip. The toolbar casts the platform's 8dp elevation shadow (`toolbarShadow`), drawn outside the bar's shape only, so it never darkens the glass, and has a 1dp edge (`glassEdge` on glass, `divider` on the opaque bar) so it holds its edge on a bright photo and on black.

### Motion (`Motion.kt`)

| Token | Value | Used for |
| --- | --- | --- |
| `pageSpring` | spring, no bounce, stiffness 500 (about 300 ms) | page slides: tabs up and down, pushes sideways |
| `indicatorSpring` | spring, damping 0.85, stiffness 500 | the toolbar's selection square sliding up or down the bar |
| `fast` | 150 ms | crossfades |
| `toolbarShowHide` | 200 ms | the toolbar leaving (30dp towards the right edge, with a fade) for the photo viewer and the keyboard, and coming back |
| `push` | 30dp horizontal shift with a fade, `pageSpring` | opening and closing a page inside a tab (a library, a show, an item, the library editor) |

When the system's "Remove animations" setting is on (`LocalReduceMotion`), tabs and pushed pages crossfade instead of sliding, the carousel snaps without gliding, and the selection square moves without travelling. Nothing loops.

### Touch and icons

Every target is at least 48dp. Icons are Lucide (the web's set, from the same Lucide release as the web's `lucide-react` 1.55.0) at 24dp with a 1.75 stroke (`IconStroke`); empty-state icons are 28dp at 1.5. The glyphs are generated from the web's `lucide-react` into `ui/icons/LucideGlyphs.kt` (android/README.md, Icons) and drawn with round caps and joins at the stroke asked for. Fixed glyphs are named in `ui/icons/PopcornIcons.kt`; library icons in `ui/icons/LibraryIcons.kt` (with the plain name used for content descriptions and the picker's tooltips), one entry per name in the server's curated list (`GET /api/libraries/icons`, docs/architecture.md section 8), with `folder` for a name the app does not know. `folder` is not in the list an owner picks from: it belongs to folders that are not a library. The plain names are the web's (`web/src/lib/libraryIcons.ts`), checked by `LibraryIconsTest`.

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
| Sort a library | `arrow-up-down` |
| Edit library (owners) | `settings` |
| Save the original (photo viewer) | `download` |
| Details (photo viewer) | `info` |
| Music, archives (Other files) | `music`, `file-archive` |

## Navigation

### Floating toolbar (`ui/components/FloatingNavToolbar.kt`)

The app's navigation: one upright rounded bar floating on the right edge of the screen, its bottom where a right thumb rests, icons only, no labels. It is on the right for one-handed use, by the owner's direction: the same bar that sat at the bottom centre, turned upright and moved to the edge.

- Three destinations in a column: Downloads at the top, Home in the middle, Settings at the bottom (the order they had from left to right). Home is the start tab.
- Each icon sits in its own square target: 52dp, 12dp corners. The squares touch, so together they make one bar, 60 by 164dp; the bar adds 4dp of padding and a 16dp outer radius (12 + 4: the outer radius is always the inner radius plus the padding). When the window is too short for the bar between the status bar and the navigation bar with 16dp to spare at each end (a phone in split screen), the squares shrink, never below 48dp.
- Place (`AppShell`): physically on the right in every layout direction, because which hand holds the phone does not change with the language, so nothing in the bar mirrors. Its right edge is 16dp (`ToolbarTokens.margin`) in from the screen edge and its bottom 16dp above the gesture or navigation bar inset, where the bottom of the horizontal bar was. The 16dp also keeps it clear of the system's back-gesture edge for a swipe (a tap that close to the edge is fine). With the phone on its side the bar stays on the right edge, bottom-aligned the same way, 16dp from a navigation bar or a display cutout on that side (the navigation bar and cutout insets, right and bottom sides); `shell-landscape-home-light.png` and `shell-landscape-downloads-dark.png` show it.
- Container: frosted glass where the phone can blur (Frosted glass, below), the opaque `surface` where it cannot; the 8dp shadow outside the bar only and a 1dp edge.
- Unselected icons: `textMuted`. Selected: the square fills with Action Blue and the icon turns `onAccent` (white). This is the one place navigation uses blue, by the owner's direction; elsewhere navigation stays neutral as on the web. A press on the selected square uses the 8% white ripple.
- The blue square has a 2dp `surface` ring outside its edge (`indicatorRing`, radius 14dp, concentric with the square and the bar), so its edge keeps 3:1 against whatever the glass shows (Colour, Action Blue measured). On the opaque bar the ring is the bar's own colour and does not show.
- The Downloads square carries the number of files ready to import (the `inbox` count) as a small `text`-coloured badge on its top-left corner, the side away from the screen edge, in every layout direction (`tag` type on `page`, never a status colour, with a 2dp `surface` ring that parts it from the icon and the glass); it disappears at zero.
- Every square has a content description (its name, and for Downloads the count: "Downloads, 3 ready to import"), the tab role, its position in one column ("1 of 3", top to bottom: the bar is a selectable group of three rows in one column) and a ripple clipped to the square. A long press shows a plain tooltip with the name to the left of the square, over the page.
- It floats over the content: the pages scroll under it on the right. Every page ends with enough bottom padding (`TabPage.bottomClearance`: the bar's height + its margin + 16dp = 196dp, plus the navigation bar inset) that, scrolled to its end, its last row sits 16dp (`contentGap`) above the bar, so nothing on it (a trailing button, a More button, a progress line, a switch) stays under the bar. Mid-scroll the rows pass under the glass. Bottom padding rather than an end inset: the bar covers only the bottom right corner, and keeping the whole right side clear would narrow every row and grid by 92dp. `shell-downloads-end-*.png`, `shell-settings-end-light.png` and `library-posters-end-*.png` show pages at their end; their tests fail when any control or line of text overlaps the bar, and `AppShellTest` checks the 16dp.
- It leaves (slides 30dp towards the right edge and fades, `toolbarShowHide`) for the photo viewer (which asks it to, `HideToolbarWhileShown`; it comes back when the last page asking is gone) and while the keyboard is open. The player is its own activity, so the toolbar is not on screen while a video plays.
- In a right-to-left language the bar stays on the right with Downloads at the top, and the tabs still slide up and down; pages pushed inside a tab come in from the end side, the left.

### Frosted glass (`ui/components/FrostedGlass.kt`)

The pages scroll under the toolbar softened and tinted: what is behind it turns to its colours, so the bar shows where it is without competing with the pictures. One look, made three ways, chosen by `glassTierFor(SDK_INT, SDK_INT_FULL, moreContrast)` from the phone's release and its contrast setting (tested in `GlassTierTest`):

| Tier | Android | How the blur is made |
| --- | --- | --- |
| Backdrop | 17 QPR2 (SDK 37.2) and later | The platform blurs what the window drew behind the bar: a render node the bar's size with `RenderNode.setBackdropRenderEffect` (blur, then saturation) and the tint drawn inside it. Nothing is recorded. The bar fades in and out with its alpha applied to each drawing operation (`CompositingStrategy.ModulateAlpha`), not through a layer of its own: inside such a layer the platform would blur the empty layer instead of the window, and the pages would show sharp under the tint for the 200 ms of the fade. |
| Captured | 12 (API 31) to 17.1 | `AppShell` records the tab pages into one Compose `GraphicsLayer` each frame they draw (`glassSource`); the bar draws that layer moved to its own place, blurred (`BlurEffect`) and saturated, clipped to its shape, under the tint. Only the bar's part of the pages, and the blur's reach around it, is blurred. It follows scrolling, page slides and the bar's own slide in and out. |
| Opaque | 9 to 11 (API 28 to 30), and any phone set to medium or high contrast | No RenderEffect, or the person asked for stronger edges: the opaque `surface` bar with its `divider` edge, as before. |

Android 17 QPR2 reaches Pixels in December 2026, so the captured glass is what most phones show.

- Blur radius 24dp (`ToolbarTokens.glassBlur`): text and edges behind the bar become soft colour; 16dp left the shapes of the pictures readable, 32dp looked the same as 24dp.
- Saturation 1.8 (`glassSaturation`): the colours behind read through the tint instead of washing out to grey. It does not change white or black, so it does not change the contrast below.
- Tint (`glass`): `surface` at 80% in both themes (white in light, zinc-900 in dark). The unselected icons need 64% or more in light and 76% or more in dark to stay at 3:1 over anything. The selected square does not depend on the tint: its `surface` ring is its neighbour.
- Edge (`glassEdge`): zinc-950 at 10% in light, white at 10% in dark, drawn over the glass, so it reads as a hairline over any picture (it renders as `#E6E6E6` on white glass, `#58585B` on dark glass).
- Shadow: the platform's 8dp elevation shadow, drawn with the bar's own area cut out, so it never shows through the glass.
- The selected square, its slide (`indicatorSpring`) and its jump under reduce motion are the same on glass; reduce motion does not change the glass.
- Android has no setting for reduced transparency (checked in the SDK 37.2 sources). The nearest is the colour contrast setting (Android 14 and later, `UiModeManager.getContrast`): at medium or high contrast the bar is the opaque one, and it follows a change of the setting while the app is open (`rememberGlassTier`, tested in `GlassTierTest` and `FrostedGlassTest`). High-contrast text is left out: its query (`AccessibilityManager.isHighContrastTextEnabled`) is behind a feature flag in the SDK, so a phone may not have it.

Contrast on the glass, at its worst (the bar over a pure white and a pure black picture, white above the middle of the bar and black below it; `glass-white-black-light.png` and `-dark.png`):

| | Light, over white | Light, over black | Dark, over white | Dark, over black |
| --- | --- | --- | --- | --- |
| The glass renders as | `#FFFFFF` | `#CCCCCC` | `#464649` | `#131316` |
| Unselected icon (`textMuted`) | 7.72:1 | 4.81:1 | 3.59:1 | 7.07:1 |
| The selected square (Action Blue) against its `surface` ring | 5.25:1 | 5.25:1 | 3.38:1 | 3.38:1 |
| Action Blue against the bare glass, without the ring | 5.25:1 | 3.27:1 | 1.79:1 | 3.54:1 |
| White icon on the selected square | 5.25:1 | 5.25:1 | 5.25:1 | 5.25:1 |

The Downloads badge is opaque (`badge` with `onBadge` words, ringed in `surface`), so it reads the same on glass as on the opaque bar.

Cost: on Android 12 to 17.1 the pages are recorded once per frame they draw and the bar reuses that recording; the blur runs over the bar's area only (about 156 by 260dp with its reach). On 17 QPR2 and later nothing is recorded.

### Selection indicator

One Action Blue square that lives behind the icons and slides up or down the bar to the tapped square with `indicatorSpring`. The icons take their colour from the square itself, not from a timer: each icon is drawn twice, white clipped to the square's current bounds and `textMuted` clipped to the rest, so while the square slides the part of an icon over the blue (its top or its bottom) is white and the rest is grey. No icon is ever white on the glass (where it would vanish in light) or grey on the blue. Under reduce motion the square and the colours jump together. Tested frame by frame (`AppShellTest`): a few frames after a tap the square is between the two squares, straight below the first, and with its top edge across the Home icon the icon is white over the square and grey above it; under reduce motion the square is already under the tapped one. `transition-1-light.png` to `transition-3-light.png` show it.

### Tab transitions (`navigation/PageTransitions.kt`)

- Tapping a tab slides the pages vertically, the way the selection moves on the bar: a square below the current one brings its page up from the bottom while the old page leaves through the top; a square above brings its page down from the top while the old one leaves through the bottom. Full height, `pageSpring` (about 300 ms). The bar's order is the same in every layout direction, and so are these slides. Pages pushed inside a tab still move sideways (`push`); slide offsets are in screen pixels, so in a right-to-left layout a pushed page comes in from the end side (the left). `transition-1-light.png` to `transition-3-light.png` show Settings rising over Home early, half way and most of the way.
- Each tab keeps its own back stack, state and scroll position (a saveable state holder per tab, data in a view model per destination). Tapping the current tab again scrolls its first page to the top, or pops its stack to the first page. The stack is `navigation/PageStack.kt`: routes saved with the tab (they survive another tab, a rotation and a recreated process), one saveable state and one view model store per page; a page's view models live until it closes or the person's session ends.
- Inside a tab, pages open and close with `push`. The toolbar stays while browsing.
- System Back pops the current tab's stack first; at a tab's first page it returns to Home (sliding back: Home comes down from the top after Settings, up from the bottom after Downloads), and from Home it leaves the app. Back closes the viewer or player before anything else.
- Reduce motion: a 150 ms crossfade for tabs and pushes.
- No swiping between tabs: Home's shelves and carousel scroll sideways and a tab swipe would fight them.

### Top app bar (`ui/components/PageTopBar.kt`)

- Part of each page, so it slides with it. 64dp tall below the status bar inset, on `page`.
- A tab's first page: the page title in `headline` ("Home", "Downloads", "Settings"). A pushed page: a 48dp back button (`arrow-left`, "Back") before the title in `title` type (the library's name, the show, the item, "New library").
- Trailing actions, at most two 48dp icon buttons with tooltips: Downloads has "Add a link" (`plus`); an owner's library page has "Edit library" (`settings`).
- When the page scrolls under it, a 1dp `divider` rule appears at its bottom edge; no colour change, no shadow.
- A pushed page that starts with its own large heading under its picture (a show, an item) leaves the bar's title empty until that heading has scrolled away, so the name is never shown twice.

## Pages

### Home

1. **Continue watching** (only when there is something to resume; no empty state here). A horizontal pager of 16:9 frames, each card the gutter-to-gutter width minus a 32dp peek of the next card, 12dp apart, snapping one card at a time (`CarouselTokens`). Under each frame, 10dp down: the title in `item` (the show's name for an episode) and one `metadata` line ("S01E02 · 12 min left"); the More button takes no more height than these two lines, its 48dp target reaching past them. The frame carries the 4dp Action Blue progress bar over a `mediaProgressTrack` (zinc-950 90%) track along its bottom. Tapping a card resumes it in the player at once; a 48dp More button beside the title offers "Details" and "Start from the beginning". Content description: "Resume Bluey S01E02, 12 minutes left". The pager has no dots: the peek shows there is more.
2. **Libraries**, every one the person can see, in the owner's order, 40dp apart. Each starts with a 56dp header row: the library icon at 24dp in `textMuted`, 12dp, the name in `section`, its count in `metadata` beside it ("214 videos", "12 shows", "3,214 photos"; the name gives way with an ellipsis first), and a trailing `chevron-right` on the gutter; the whole row opens the library. Under it, 4dp down (the row's own padding parts the name from the tiles), a shelf of up to twelve of its newest items in the library's shape: posters for `posters` and `series` (a series library has a tile for every show, up to twelve, the show with the newest episode first, from `HomeLibrary.shows`, so the tiles match the heading's "3 shows": each pictured by its poster, with "New: S01E03", "3 new episodes", or "12 episodes" when nothing is new), frames for `frames`, squares for `timeline` (a photo opens the viewer on that shelf's photos; a phone clip plays). A library with nothing yet keeps its header, without a count, with "Nothing here yet" in `metadata` under it.
3. **Other files**, last, as a single 56dp row (`file-text`, "Other files", its count, chevron) when the person has any.

When the person can see no library: an empty state with the `house` icon, "Nothing has been shared with you yet." for members, or "No libraries yet" with "Make a library" (opening the editor in Settings) for the owner.

### A library (pushed from Home)

- **Posters or frames (one-offs):** a lazy grid (posters at least 104dp, three columns on a phone; frames at least 160dp, two columns) with a sort menu as a trailing top bar action, the web's choices (the API sorts by `added`, `title`, `taken` or `size`): posters "Recently added" (first), "Title", "Largest first"; frames "Date filmed" (first), "Recently added", "Title", "Largest first". The next page loads when the person nears the end; a page that does not come says why with "Try again" at the end of the grid.
- **Series (episodes):** a poster grid of shows ("3 seasons" or "12 episodes" under each). A show page: its poster frame at the top (16:9 crop of the poster episode), "Resume S01E02" or "Play S01E01" as the primary button, a season picker (Material segmented buttons up to four seasons, a dropdown beyond; Choices and confirmations, below), then episode rows: a 128dp 16:9 frame with the watched bar, the episode name in `item` ("3. The lantern keepers"; "Episode 5" when it has no title), its state in `metadata` ("Watched", "Up next · 5 min left" in `text`, or the runtime). The pictures part the rows, so they have no rules. What plays next: the last episode touched if it is not finished, else the one after it, else the first; specials only when the show has nothing else.
- **Timeline (photos):** month headings in `title` with the count in `metadata` (on the gutter; the count goes under the month when large text leaves no room beside it), each month as the server groups it, on the camera's clock (Dates and capture times, below), square tiles four across with 3dp gaps from edge to edge, without corners. A photo opens the full-screen viewer; a phone clip kept with the photos carries a 24dp `mediaControl` disc with `play` at its bottom start and plays when tapped.
- **Other files:** a list of rows with the file-type glyph, name and "PDF · 592 B", under the same sentence as the web ("Music, documents and anything else. Only you and the owners see these." for members). Tapping one opens it in another app: PopcornStash fetches a private copy, hands that app a read-only link to it and deletes the copy afterwards (docs/architecture.md section 10), so nothing lands in the phone's Downloads. While it fetches, the row's trailing spot shows a small indeterminate indicator; when no app opens the type, a snackbar says "No app on this phone opens this kind of file."
- Empty: the library's own icon, "Nothing in Kids yet", and how it fills, as on the web ("New movies land here on their own.", or "Files you accept from Downloads into Kids show up here." for a library that takes nothing); owners also get "Edit library".
- A library that disappears while open (deleted or hidden) pops back to Home, with every page opened from it, and a snackbar: "Kids is not available any more." A show that left the library closes the same way ("The Lantern Keepers is not in Shows any more."), and an item the person can no longer see ("That file is not available any more.").
- Snackbars sit at the bottom beside the floating toolbar, never under it (Choices and confirmations, Snackbars).

### Item detail (pushed)

The picture at the top (the 16:9 frame with the watched bar, full width, 10dp corners inside the gutter; a photo keeps its own shape between 3:4 and 16:9 and opens the viewer), the title in `headline`, one `metadata` line ("2014 · 1 h 52 min", "Season 1, episode 2 · 24 min", "Taken 12 Oct 2026, 14:05"), then the one primary button ("Resume from 12:04" or "Play") with the favourite beside it as a 48dp outlined square (`heart`; filled on a `subtle` square while it is on; "Add to favourites" or "Remove from favourites" as its name and tooltip) and "Start from the beginning" as a text button under them, lined up with the page's text. A photo has no play button: the favourite is a secondary button with its words. Then a 1dp rule and the facts (video, audio, subtitles, dimensions, camera, file size) as label-over-value columns, two across. An episode adds "Next" over "S01E03 The lantern keepers" as a row with a 96dp frame when there is one in the same library. A missing file says so with the warning dot.

### Player and photo viewer (immersive)

Full-bleed `stage` black, the toolbar hidden, edge to edge under the system bars (bars hidden while controls are hidden). Controls laid over a picture sit on `mediaControl` discs (circles) and plates (16dp panels). The photo viewer (`feature/photos/PhotoViewer.kt`) pages sideways between photos (16dp apart), with close on the player's 52dp disc (`MediaDisc`) at the top start and, at the bottom, a `mediaControl` plate (16dp corners) across the width; every control keeps the player's 16dp edge inside the same safe area (the cutout and the system bars, measured whether or not they show), so the viewer's chrome lines up with the player's and does not move when the bars come and go with the date and camera and two icon buttons: "Save the original" (`download`: the system's "Save to" picker, then the app copies the original into the chosen file with its own client; a small indicator while it saves, then a snackbar on top of the plate: light, the inverse of the stage, so "Original saved." never covers the plate's buttons) and "Details" (`info`: the photo's page). A tap shows or hides the controls, and the system bars with them (their icons stay light on the stage). A pinch zooms up to 5x around the fingers, a double tap zooms to 2.5x at the tap and back (no travel under reduce motion), a zoomed photo pans within its edges and the pager waits until it is whole again. Phone clips show their frame with the player's 72dp play disc. The photo is decoded at up to twice the window's long edge (4096 px at most) over its large thumbnail.

### Player (`feature/player/`, tokens in `ui/theme/PlayerTokens.kt`)

Its own activity (`PlayerActivity`), opened with `openPlayer(itemId)`, so it can turn to landscape, hide the system bars and shrink into picture-in-picture without touching the tabs. It is always dark, whatever the system theme: the stage is black and the picture decides the light in the room, and the end and error messages' buttons stay legible on the dimmed picture. Screenshots: `app/src/test/screenshots/player-*.png`.

- **Orientation:** landscape, either way round as the phone turns; a video taller than wide (a phone video held upright, its rotation applied) turns the player upright.
- **Top row:** close (`x`, "Close player") on a 52dp disc at the top start, the title plate beside it (the show in `section`, then "S01E02 · Pilot" in `metadata`; a film or a phone video has one line), and at the top end the audio and quality menu (`ellipsis-vertical`, "Audio and quality", "Audio" or "Quality") when the video offers either. Discs and plates in the top and bottom rows are all 52dp tall (the toolbar's square), 8dp apart, 16dp inside the safe area (the cutout and the system bars, measured whether or not they show, so nothing moves when the bars come and go).
- **Centre:** play or pause on a 72dp disc with a 32dp glyph. While the stream loads, buffers or switches, a 28dp ring turns inside the disc instead (the only spinner, inside a control); it stays even when the rest of the chrome has faded, so a stalled frame never looks frozen.
- **Bottom row:** a plate across the width with the position, the seek bar and the length ("12:04", "52:16", tabular figures). The bar is 4dp: Action Blue for what played, `onMedia` at 50% for what loaded, at 28% for the rest, a 16dp Action Blue thumb with a 2dp `onMedia` ring (so it holds 3:1 against the plate over a bright frame), a 48dp touch height. TalkBack hears "Position, 12 minutes 4 seconds of 52 minutes 16 seconds". Beside it, when the episode has a next one in the same library, a "Next episode" plate.
- **Menu:** a Material dropdown on `surface` with 16dp corners: "Audio" (each track as the web names it, "English" or "Commentary (English)", with "Dolby Digital Plus 5.1" under it), a divider, "Quality" ("Original" with "1080p, converted only if needed", then the smaller steps the phone does not already get, "Smaller stream for slow connections"), the chosen line with `check`, and "Converted for this phone while you watch" at the end while the server converts. A choice asks the server for a new stream at the same place; the ring shows until it plays.
- **Showing and hiding:** the chrome fades out (`fast`) 3 seconds after the last touch while playing (TalkBack's recommended timeout instead when it is on) and a tap on the picture brings it back. It never hides while paused, loading, at the end or after a failure, and pausing by any route (headphones unplugged) brings it back. The screen stays on while playing.
- **The end:** the picture dims under a full-screen `mediaControl` scrim. With a next episode: "Next: S01E02 · The lantern keepers" in `section`, then "Play next episode" (primary, `play`) and "Back"; nothing starts on its own. Without: "Play again" (primary, `rotate-ccw`) and "Back".
- **Failures:** the same scrim. A video the person can no longer see (access removed, file gone): "This video is no longer available." and "Back" as the one action. Anything else: "Playback stopped." in `section`, the reason in `body` (the server's own sentence, or "The stream stopped. Check the connection and try again."), "Try again" (primary) and "Back".
- **Picture-in-picture:** leaving the app while a video plays shrinks it into a window of the video's shape (by itself on Android 12 and later), growing out of the picture itself. The window shows only the picture, a 20dp ring on a 32dp disc while it waits, or the reason it stopped; its one action is play or pause (Lucide `play` and `pause`). Closing the window ends the player; expanding it brings the player back, and closing the player then returns to the app.

### Downloads

The tab where files come in: watch what is downloading, then put what finished into the right library.

1. **The VPN line** under the top bar, one row in the web's words with the status dot (the 500 shade) beside the label in its status colour and one `metadata` sentence under it: "Protected · NordVPN · Netherlands" with "Torrents run only through the VPN. Public address 203.0.113.5.", "Torrents paused: VPN connecting", "No VPN: torrents use this network", "Torrents off" with the server's reason, the owner's and the member's sentences as on the web (`feature/downloads/VpnText.kt`). For the owner it opens the web VPN settings in a Custom Tab (trailing `external-link`); while the VPN connects there is nothing to open. While the live events are cut off, "Not connected to the server. Trying again." sits above it with the warning dot.
2. **In progress** (when any), the count beside the heading: files shared from this phone first ("PXL_20261012_181502.mp4 and 2 more", "Uploading  41%  1 of 3 done", or "Waiting for a connection to upload."; More: "Stop uploading"), then downloads, newest first, with the name in `item` (two lines, breaking at dots, underscores and hyphens), a 4dp progress bar (Action Blue on `progressTrack` while it moves, the muted text colour when paused or waiting, an empty track while the size is not known; nothing loops), and a `metadata` strip of facts spaced 12dp apart ("64%  2.14 MB/s  12 min left", "Paused  31%  128 MB of 412 MB", "Looking for peers"), or the reason it waits as a sentence ("Waiting for the VPN to connect.", "Waiting for a free slot (5 at once)."). Trailing: a 48dp Pause or Resume icon button ("Pause Harbor.Lights…" for screen readers, a tooltip) and a More menu with "Remove" (confirmed: "Remove this download?", its name, "What was downloaded so far is deleted.").
3. **Ready to import** (when any), with its count and "Accept all" as a text button in the section header (from two files that are ready; confirmed: "Accept all 4 suggestions?", "Each file moves to the library shown in its row.", then "Moving 2 of 4."). Each row: a 64dp square tile (the server's thumbnail, else the file's glyph), the file name, the reason it waits in `metadata` from the server's `waiting`, in the web's words ("Not sure this is a movie."; "No library takes TV episodes yet." for the owner or "Choose a library for this file." for members; "The library's folder is not available."; no line when it only needs accepting), the size and length, then the library picker (an exposed dropdown, 48dp, showing the library's icon and name, or "Choose a library"; it lists only the libraries the person can see that fit the file, then Other files after a rule, the chosen one ticked; with one place to go it shows it without a chevron or menu, on a `divider` edge; for a file someone else added that the person sees only through the library that takes its kind, it lists that one library) and the actions: "Accept" (the primary button on the first row only, a tonal button on the others; disabled until a library is chosen; "Add details" opens Edit when the library is a series and the show or episode is missing; it sends the library the row shows), "Edit" and a More menu with "Ignore". Ignored files follow under a closed disclosure row, "Left where they are (3)", with the same row (no Ignore).
4. **Finished:** the latest 20 downloads ("Show more" for the next 20) with "Finished 3 min ago" (or "Started 3 days ago" for one that failed) and the size, and where they went as a 48dp tappable line with the library's 16dp icon and a chevron: "In Kids" (opens the library), "In Other files", "2 waiting to import" (scrolls to Ready to import), or just "Sorted" with a check when nothing it brought in is visible to the person; "Failed. …" in the error colour, "Cancelled before it finished.", "Finished. Waiting to be sorted.", "Sorting into your library.". More: "Try again" for a failed link or magnet (resumed, or added again), "Remove" (a sorted download leaves the list and its files stay in the library). Members see their own downloads and someone else's only once sorted into something they can see; a row the server removes from view disappears without a message.

Pull down to load everything again (the Material indicator in Action Blue on `surface`). After an action a snackbar says what happened ("Moved to Kids.", "Ignored. clip.mkv stays where it is.", "Download removed.", "Added clip.mp4.") or the server's sentence, in the snackbar beside the toolbar (Choices and confirmations, Snackbars).

Loading: the VPN line ("Checking VPN", the label alone) and placeholder rows in `subtle` at their final sizes. A failed load: "Downloads could not be loaded", the reason, "Try again". While the server does not answer (the load failed, or the events stream is cut off: "Not connected to the server. Trying again.") and the VPN status was never read, the VPN line is left out: the page says it once, and "Web links and uploads still work." would not be true. A status the server did report stays. When the server answers but the VPN status cannot be read: "VPN status unavailable" and "Web links and uploads still work."

Empty (nothing at all): the `arrow-down-to-line` icon, "Nothing downloading", "Links and magnets you add, and files you share to PopcornStash, show up here." and "Add a link" (primary, `plus`). The add sheet ("Add a link"): a "Link or magnet" field (URL keyboard, Go adds), prefilled from the clipboard when it holds a link (a clip the copying app marked sensitive is never read), and under it what the link is in the web's words ("From your clipboard. Magnet link. Downloads as a torrent, only through the VPN.") or why it cannot be added ("Add https:// to the start of the web address."); "Add" as the primary button, "Cancel".

The edit sheet ("Where does it go?"): the file's tile and name, "Library" with the same picker, "What is this file?" ("A movie" or "A camera video", segmented) in a One-offs library, Title and Year side by side for a movie, Show then Season and Episode for a series, then the live preview under a rule ("Will move to Movies", the folder for owners, "Renamed to Paper Boats (2011).mkv" or what is missing) as the one place that says the destination in words, "Move it there" and "Cancel". The server's sentence sits under the field it names.

### Settings

1. **You:** the 48dp avatar with initials, the name in `section`, the role in `metadata` ("Owner", "Member").
2. **This server:** the address with the `server` glyph and the version, not tappable. A long address wraps after a dot or a hyphen (Wrapping, below).
3. **Libraries** (owner), under "What Home and the web sidebar show, in this order.": rows like the web's, 72dp: the icon at 20dp in `text` on a 40dp `subtle` square with 8dp corners, the name in `item`, a `metadata` strip spaced 12dp apart (wrapping) of what it is, what it takes and who sees it ("Series  Takes TV episodes  Only Tomas"), with "Folder missing" or "Folder read-only" in the warning words and dot when the folder cannot be used. The row opens the editor; a 48dp More button ("More for Shows") opens a menu with "Move up", "Move down" (each greyed at its end of the list) and, under a rule, "Delete" in the error colour. TalkBack offers the same three as actions on the row. A move shows at once and saves the whole order (`PUT /api/libraries/order`); if it fails the row goes back and a snackbar says why. Rules between rows start where the names start. Then "Make a library" (`plus`) as a secondary button, shown once the list is there, greyed with "A stash can have 24 libraries." at the limit. Loading: four placeholder rows at the rows' size. Failed: "Libraries did not load." with the reason and "Try again". None: "No libraries yet. Make one to give the family something to browse."
   **Folders not in a library** follow when there are some, under "Only owners see what is in them. Make one a library to share it.": the `folder` glyph on the same square, the folder name with a trailing slash in the monospace face (the whole name: a short one on one line, a long one wrapping at its spaces, never cut off with an ellipsis), its counts ("12 videos  20 photos  4 other files", or "Nothing indexed yet"), and "Make a library" in `action` type at the end; the whole row is the target and opens the editor on that folder.
4. **On the web:** rows with a trailing `external-link`, each opening that web settings page in a Custom Tab: for the owner "Sorting and trash" (the rest of the web's Libraries page: automatic sorting, the folder layout, a scan and the trash), VPN, People, Devices, Security, System; Devices and Security for members. The first time, the browser asks the person to sign in with their passkey.
5. **Sign out:** a quiet row in `textMuted` with `log-out`, confirmed in a dialog ("Sign out on this phone?"; when uploads are running, "2 uploads stop." is added).

### Library editor (pushed from Settings)

The web editor's fields, in the same order and with the same words, as one scrolling form inside the gutter (`feature/settings/LibraryEditorScreen.kt`). The top bar says "New library" or the library's name. Each group is a `section` heading with one `metadata` sentence under it, 16dp above its fields; groups are parted by a 1dp `divider` rule with 24dp on each side.

1. **Name and icon** ("How it looks on Home and in the web sidebar."): the Name field ("Up to 40 characters.", or the error), "Icon" and the icon picker, then "On Home" over a preview of the library's Home header row (its icon in `textMuted`, its name in `section`, `chevron-right`) on a `subtle` plate with 8dp corners, "New library" while the name is empty.
2. **What it holds**: "Videos" or "Photos" ("Photos and the short clips phones take with them"), then "Arranged as": "Series" ("Shows with seasons and episodes") or "One-offs" ("Films and clips that stand on their own"). On a library that holds files the group's sentence becomes "The type can change only while the library is empty." and the choices give way to a fact: "Holds" over "Videos, arranged as series" (or "… one-offs", "Photos") in `body` and `text`, like the folder's fact, as the web shows it. Changing the type unchecks kinds that no longer fit, and the icon follows the type (`film`, `image`) until the owner picks one.
3. **New files** ("Each kind of new file goes to one library at most. A kind no library takes waits in Inbox."): "Sort these here" with a checkbox for each kind that fits, and under each where it goes now ("Sorted into Movies now", "Moves here from Shows", "Sorted here now", "Not sorted anywhere: these wait in Inbox").
4. **Folder**. Making: "Make a new folder" with the "Folder name" field in the monospace face (it follows the library's name until edited), or "Use a folder that is already there" (greyed with "Every top-level folder is already a library." when there is none) with a menu of the folders, each its name and counts (the field shows the whole name, wrapping at its spaces rather than cutting it off), and under it "Files stay in Family 2019/. Nothing is moved." (the folder name kept on one line, Wrapping, below) plus what the chosen type will not show ("4 other files there will not show in this library."). Editing: the folder as a fact in the monospace face with its warning words when it cannot be used, and "Renaming the library keeps this folder." (or why new files wait in Inbox).
5. **Who sees it** ("Owners see every library."): "Everyone" or "Only some people", with the members as checkboxes indented under it (or "Nobody else has joined yet, so only owners will see it. Invite people on the web, in Settings, People."). A library made from a folder that is already there starts on "Only some people" with nobody chosen, and says who will see what ("Only owners will see its 12 videos and 20 photos.").
6. The primary button, full width, 32dp below: "Make library" or "Save changes". A save the server refused without naming a field shows "The library was not saved." and the reason in the error colour above it; one that names a field shows the message under that field and scrolls it into view.
7. **Delete library** (editing), apart below: its sentence ("Takes Kids out of the app. Its files stay in Kids/ and only owners will see them.") and an outlined button with the words in the error colour.

Taking a kind another library has is confirmed first: "Sort movies into Kids?", "New movies will go to Kids instead of Movies. Files already sorted stay where they are.", "Move and make library" or "Move and save". Delete is confirmed with "Delete Kids?", the same sentence and "Delete library" in the error colour. Saving or deleting closes the editor with a snackbar ("Kids is ready.", "Changes saved.", "Kids is no longer in the app. Its files stay in Kids/."); a save that changed nothing just closes. Loading shows the form's shape in `subtle` (a heading, the field, two rows of picker cells, two choices); a failed load says "This library could not be loaded." with the reason and "Try again"; a library deleted elsewhere, before or while it is open, says "This library is not here any more." with "Back to Settings". Tapping Settings again closes the editor.

### Icon picker (`ui/components/IconPicker.kt`)

A grid of square cells in the server's order that fills the width: as many columns as fit at 48dp or more with 8dp between them, the cells growing to fill the row (six across on a 411dp phone, about 55dp each); `IconPickerTokens`. It is a plain grid, not a lazy one, because it sits in the editor's scrolling form. Each cell: the 24dp glyph in `textMuted` with 8dp corners. The chosen cell: `subtle` fill, a 1.5dp `text` ring and the glyph in `text`; neutral, not Action Blue, because it is a choice, not navigation or an action. Semantics: one selectable group, each cell a radio button whose content description is the icon's plain name ("Party popper"), with a tooltip on long press.

### Sign-in

1. **Server:** the brand, "Server address" (URL keyboard, `https://` assumed when no scheme is typed) and "Continue". Errors in words: "PopcornStash did not answer at this address.", "Use an https address."
2. **How to sign in:** above it and on the pairing page, the server line: the `server` glyph, the address and "Version 0.2.0" beside it, or under it when the address leaves no room (never squeezed). "Sign in with a passkey" as the primary button when the server lists the app, and "Pair with a code" as a secondary button (the only way when it does not, with the sentence "Passkeys work in the app once the server lists it. Pair with a code instead.").
3. **Pairing:** the code large in the monospace face with spaced groups, "Open PopcornStash to approve" (primary, a Custom Tab on `/pair`), and "Waiting for approval" with a small indeterminate indicator; "This code expired." or "This request was turned down." with "Start again".

### Share sheet (`share/ShareActivity.kt`)

A bottom sheet over the app that shared (a see-through window, the sheet on `surface` with the 16dp top corners and a 40% scrim). A link or magnet: "Add to Downloads?", a 40dp `link` tile with the link's name and the link itself (two lines, wrapping at its separators), the sentence about what it is, "Add to Downloads" as the primary button, "Cancel"; once added, "Added to Downloads" with the download's name, "Done" (primary) and "Open Downloads". Files: "Upload 5 files?", up to three names (one line, the middle elided) with their glyph tile and size, "and 2 more", "Uploads as Ana Ribeiro to stash.example.com." and, when some could not be taken, "1 file stays behind: PopcornStash takes files only when another app shares them with it."; "Upload" as the primary button (it asks for the notification permission on Android 13 and later the first time; the upload runs either way), "Cancel". Not signed in: "Sign in to PopcornStash first." with "Open PopcornStash". Nothing usable: "Nothing here PopcornStash can take" and "Close". While the session or the files are read: placeholder bars at the sheet's height.

The upload notification (channel "Uploads", low importance, private on the lock screen) shows "Uploading 3 files" with "1 of 3 done · 42%" and a progress bar and a "Stop" action, then "3 files uploaded" with "They will be sorted on their own." ("1 file uploaded", "It will be sorted on its own."), or "Upload stopped" with "Share the file again to finish uploading." or the server's sentence; it opens Downloads when tapped. Signing out removes them.

## Components

### Tiles and shelves (`ui/components/MediaTile.kt`, `Shelf.kt`; grids in `feature/library/LibraryScreen.kt`)

- The picture is the tile. Shapes follow the library's `layout`: posters 2:3 (`posters`, `series`), frames 16:9 (`frames`, continue watching), photos square (`timeline`). 10dp corners, the 1dp hairline inside the edge, `subtle` placeholder with the library's icon at 22dp when there is no picture or while it loads; the picture fades in over it (`fast`; none under reduce motion). Pictures come through Coil with the session's own client (`ui/media/StashImages.kt`): thumbnails `sm` (400 px) for photos and episode rows, `lg` (1280 px) for posters, frames, continue watching and the item page.
- Under the picture, 10dp down: the title in `item` (two lines, then an ellipsis) and one `metadata` line. Photo tiles have no caption.
- Partial watch progress: a 4dp Action Blue bar along the bottom of the picture over a `mediaProgressTrack` (zinc-950 90%) track, so the blue keeps 3:1 over any picture.
- Shelves: a row that scrolls sideways with snapping, 12dp under their header. Tile widths: posters 132dp, frames 232dp, photos 112dp; 14dp between tiles.
- Grids: posters at least 104dp wide (three columns on a phone), frames at least 160dp (two), photos at least 96dp (four, 3dp gaps, grouped under a month `title`).

### Rows (`ui/components/ListRow.kt`)

Downloads, imports, libraries, folders and settings are rows on `page` separated by 1dp `divider` rules (no cards, no boxes inside boxes): a leading glyph or picture, a name in `item`, one `metadata` line or strip, and trailing actions. Rows are at least 56dp tall and the whole row is one target when it opens something.

### Choices and confirmations (`feature/settings/FormParts.kt`)

- Radio rows and checkboxes: the whole row is the target (48dp, 56dp with a line under the label), the control at 24dp, 12dp, the label in `item` and its line in `metadata`. Checked and selected draw in `text` (the check mark in `page`), unchecked in `fieldOutline`; neutral like the icon picker, because a choice is not an action. Greyed choices fade their words to `textMuted`.
- Segmented choices (seasons, "A movie" or "A camera video"; `ui/components/ChoiceSegments.kt`): every segment has a `fieldOutline` edge (3:1 or more on `page` and `surface` in both themes); the chosen one adds a `check` before its words, a `text` edge and a `subtle` fill, so which one is chosen never rests on a tint alone (the fill on its own is 1.05:1 in light). The others' words are `textMuted`.
- Text fields: outlined, 8dp corners, the `fieldOutline` edge, Action Blue only while focused; errors in the error colour under the field.
- Dialogs: one component for every confirmation (`ui/components/ConfirmDialog.kt`): Material's alert dialog on `surface` with 16dp corners and 24dp from the screen's edges; a title that asks the question, a sentence that says what happens, the action named by what it does and "Cancel" as 48dp text buttons; a destructive action ("Remove", "Delete library") in the error colour; while the action runs both buttons wait in `textMuted`.
- Snackbars (`ui/components/TabSnackbarHost.kt`, one host for every tab): the inverse surface (zinc-900 in light, zinc-50 in dark) with 8dp corners. They sit at the bottom, level with the floating toolbar's bottom edge, 16dp from the left edge and 16dp from the bar (`TabPage.snackbarPadding`), so a snackbar is never under the bar and sits where the thumb already is. On a wide window (a phone on its side) a snackbar keeps Material's 600dp maximum and stays beside the bar. A line too long for the width wraps to two lines. When the bar has left for the photo viewer, the viewer shows its own messages on top of its plate (Player and photo viewer). `shell-snackbar-light.png` and `-dark.png` show one; `AppShellTest` checks where it sits.

### Dates and capture times (`feature/library/MediaFormat.kt`)

A photo's or camera video's time is the camera's clock where it was taken (`taken_local`, docs/architecture.md section 4, Capture time), shown as it reads in every time zone: "Taken 12 Sep 2026, 18:05" for a photo from Lisbon reads the same on a phone in New York or Tokyo. `taken_at` is never formatted. Photo months are the server's (`2026-07` is July 2026). When a file was added or a download finished is a moment, shown in the phone's zone. The phone's own order and clock style apply to both.

### Wrapping

Words that read as one stay on one line; a line ends at a space, or after a dot, hyphen or underscore, never inside a word.

- A date, a time and a month never break inside ("12 Sep 2026", "6:05 PM", "July 2026"); a date and its time may part between them.
- A folder named in a sentence ("Files stay in Grandma's Slides/.") stays whole when it fits on a line; a longer one wraps at its spaces.
- File names, links and server addresses wrap after their dots, hyphens and underscores ("popcorn-stash.family-" over "home.example.com"); an extension stays with the word before it, so "zip" or "mp4" is never alone on a line.
- A heading gives way before what sits beside it: "Accept all" and a count keep their width while "Ready to import" wraps at its words; a photo month's count goes under the month when large text leaves no room.
- `LargeTextScreenshotTest` draws these places on a 360dp phone at the largest font size.

### Empty states (`ui/components/EmptyState.kt`)

Centred in the page: a 28dp line icon (stroke 1.5, the place's glyph; a library's own icon) in `textMuted`, a `section` title ("Nothing in Kids yet"), one or two `body` sentences in `textMuted` that say how the space fills, at most one action. No illustration, no box around it.

### Menus

Dropdown menus (sort, More, seasons beyond four) sit on `surface` with 8dp corners and a 1dp `divider` edge, so they hold their edge over a bright picture; the chosen entry carries a trailing `check`.

### Loading and errors

Loading pages show placeholder tiles and rows in `subtle` at their final sizes (no spinners on pages; a small indeterminate indicator only inside a button or the pairing wait). A failed load shows one sentence and "Try again" where the content would be. Lost connection: a single row under the top bar, "Not connected to the server. Trying again.", in `metadata` with the Warning dot.

## Owner's knobs

The values most likely to change as the design is iterated, all in `ui/theme`: `ToolbarTokens` (square size, radius, padding, gap, the margin from the screen edge, the gap between a page's end and the bar, elevation, the glass's blur and saturation, badge), the `glass` and `glassEdge` colours, `PopcornMotion.pageSpring`, `indicatorSpring` and `push`, `IconStroke`, the type table, the tile widths in `MediaTokens` (also the episode frame and the row glyph square), `CarouselTokens` (card peek, gap, frame ratio), and `PlayerTokens` (disc and plate size, play disc, seek bar, how soon the chrome hides).

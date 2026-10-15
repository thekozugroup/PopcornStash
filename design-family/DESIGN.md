---
name: Popcorn family
description: Quiet, private media tools with familiar controls and warm editorial headings.
version: 1
products: [PopcornStash, PopcornSR, PopcornBatch]
---

# Popcorn family design

Use this file as the shared design contract. Product-specific workflows belong in each
repository's own DESIGN.md. Preserve permissions, authentication, playback, processing
and storage behavior while applying this visual system.

## Identity

All products use the same popcorn silhouette, scale, glass depth and lighting. Their icon
gradients distinguish them: blue for PopcornStash, violet for PopcornSR, orange for
PopcornBatch. Editable Apple Icon Composer documents and native flattened exports are in
`icons/`. The seed colors are #0088FF, #7C3AED and #EA580C; Icon Composer derives the gradient.
Use the exported icon rather than recreating its glass with CSS, shadows or vector filters.

The transparent popcorn PNG merges all SVG strokes before Icon Composer applies glass;
importing the separate stroked paths clips the popcorn's side contours. Keep that merged
source layer, a 1024px canvas and the -45 degree light angle. Export iOS/macOS, Default,
1024pt, 1x. Check the whole silhouette and preview the icon at 16, 32, 48 and 128px.

The web product lockup uses the standalone Lucide Popcorn symbol at 32px, in the text color with stroke 1.75 (40px for a large standalone mark), 10px gap, “Popcorn” in system sans at 19px/650 and the
product suffix at 12px/500 in muted text. Use an accessible full product name. Keep the whole popcorn contour visible. Do not put the flattened app tile in web headers or sidebars. On launcher/install surfaces, keep the native corner contour; do not clip it with another rounded rectangle.

Gradients and flattened Liquid Glass belong to app icons. App backgrounds stay flat zinc.
An existing native frosted navigation bar may retain its platform treatment; do not spread
glass panels or icon gradients across content screens.

## Type

Bundle Newsreader under the SIL Open Font License for page, title and section headings.
Use the system sans for body text, navigation, labels, buttons, filenames and metadata.
Use system monospace for codes and commands. Include the font license in distributed builds.
The supplied web font is variable; the Android heading font is the 600-weight, 28 optical-size
instance. No remote font service is required.

| Role | Web size / line | Android size / line | Weight |
| --- | --- | --- | --- |
| Page headline | 30 / 36px | 28 / 34sp | 600 |
| Title | 20 / 28px | 20 / 28sp | 600 |
| Section | 16 / 24px | 16 / 24sp | 600 |
| Body | 16 / 24px | 16 / 24sp | 400 |
| Action | 14 / 20px | 14 / 20sp | 600 |
| Metadata | 13 / 19.5px | 13 / 18sp | 400 |

Use sentence case and familiar verbs. Headline/title tracking is -0.025em. Filenames wrap
at separators; only an unbroken run may break mid-word. Use tabular numbers for progress,
size and time. Descriptions wrap; critical actions never disappear behind ellipses.

## Color and geometry

`tokens.json` is the portable source of values; `popcorn.css` implements the web defaults.

| Role | Light | Dark |
| --- | --- | --- |
| Page | #FAFAFA | #09090B |
| Surface | #FFFFFF | #18181B |
| Subtle | #F4F4F5 | #27272A |
| Divider | #E4E4E7 | #27272A |
| Main text | #18181B | #FAFAFA |
| Muted text | #52525C | #9F9FA9 |
| Action/focus | #155DFC | #155DFC |
| On action | #FFFFFF | #FFFFFF |

UI action blue stays the same across all three products. Product-specific icon colors do
not become competing button colors. Hover/press use #1447E6 / #193CB8. Dark inline links
use #51A2FF. Status colors always accompany words: “Connected”, “Waiting”, “Failed”.

Use the 4, 8, 12, 16, 24, 32, 48px spacing scale. Controls have 8px corners; media 10px;
panels 16px. Use thin dividers and flat surfaces. Elevation belongs to dialogs, menus and
toasts. Minimum targets: web 44px, Android 48dp. A focus ring is 2px blue, offset 3px.

## Layout and navigation

Web desktop: one 216–240px left sidebar, equal 44px rows, 4px gaps, 16px group spacing.
Put account/status controls in a distinct footer. Keep page content in a readable width
with 24–32px outer padding. Compact web windows use a bottom row or the product's existing
accessible mobile drawer; choose one navigation pattern consistently across its pages.

Android: bottom navigation on phones, including landscape. A current window at least
600dp wide and 480dp tall uses a physical right rail. Split-screen windows are classified
by available size rather than device model. Keep three equal 52dp targets, 4dp outer padding,
16dp edge margin and the same order. Reserve 92dp plus the bottom inset on phones, or a
92dp strip plus the right inset beside the rail. Content and controls never sit underneath.
State and scroll position survive resizing, folding and tab changes. Tooltips and screen
reader row/column semantics follow the axis. Reduced motion uses crossfades.

Foldable media: a horizontal half-open hinge separates media above from persistent controls
below. Avoid the full hinge bounds, including a physical gap. Maintain the playback engine,
position and session. A flat or vertical book posture uses the normal layout; picture-in-
picture uses the whole picture. Use Jetpack WindowManager posture data, not guessed angles.

## Components and behavior

Use the product's maintained component system. For Astryx products, adopt the supplied
Popcorn theme source and consult the component API before changing props. For Android,
apply the supplied Popcorn type and navigation references over Material 3.

- Page header: serif headline, optional plain subtitle, one clear primary action.
- Settings group: serif section title, plain explanation, rows with label/value/action aligned.
- Processing/download row: filename, visible state, progress and one trailing action menu.
- Empty state: explain what belongs here and provide one relevant action.
- Error: say what failed and offer a recovery action; keep the person's input.
- Confirmation: name the item and actual consequence. Destructive actions require explicit intent.
- Loading: preserve layout; skeleton or progress communicates work without blocking navigation.

Motion is brief and state-driven: 160ms fast, 200ms ordinary, 600ms only for a slow state
change. Honor reduced motion, enlarged text, right-to-left languages, keyboard input and
screen readers. Color must never be the only carrier of state.

## Agent acceptance checklist

Inspect the existing product and its DESIGN.md first. Use `preview.html` to compare the
shared shell in light/dark and narrow/wide windows; it is a reference, not a replacement
for product behavior. Apply tokens to existing components, then inspect the actual app.
Check sidebar spacing, heading wrapping, long names, empty/error/loading states, 200% text,
keyboard focus and both themes. Android agents also check phone landscape, split screen,
tablet, RTL, tabletop posture and picture-in-picture. Run the repository's required checks
and preserve its release/signing process. Record what was actually verified.

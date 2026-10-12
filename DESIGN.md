---
name: PopcornStash
description: A quiet, private family library where the media itself provides the colour.
colors:
  accent: "oklch(62.3% 0.214 259.815)"
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
  major-gap: "40px"
---

# Design System: PopcornStash

## Overview

**Creative North Star: "The family shelf"**

A private shelf for a family's videos and photos. Neutral surfaces step back so posters, frames and photos carry the colour. Every incoming file has a visible, honest path: in progress, completed, sorted. The interface earns trust by saying exactly where things are and whether torrent traffic is protected.

PopcornStash shares Popcorn SR's world: Tailwind zinc neutrals, Tailwind blue-500 as the single action/focus accent with zinc-950 text on blue, the platform system sans, flat panels, thin dividers, 44px controls, system light/dark and reduced motion. Components come from Astryx (`@astryxdesign/core`) with a custom `popcorn` theme that maps these tokens onto Astryx's token surface. Astryx components are used as-is wherever one exists; local components are limited to media-specific pieces (poster grid, photo timeline, player chrome).

## Colors

Same assignment table as Popcorn SR:

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

Emerald/amber/red are status only (VPN protected/connecting/blocked, download failed) and always paired with words. The video player and photo lightbox use a true-black stage in both themes so media is judged against neutral black.

**The One Accent Rule.** Blue marks the next action, focus and real progress. Navigation, filters and secondary actions stay neutral.

## Typography

System sans only, sentence case, no uppercase eyebrows. Hierarchy from size and weight. Titles of media wrap to two lines then truncate; file paths and sizes use the metadata style and wrap.

## Layout

Desktop: sticky 228px sidebar (Home, Movies, Shows, Home Videos, Photos, Other · Inbox, Downloads · Settings) and a bounded main column (max 1320px, padding `clamp(24px, 5vw, 64px)`). Under 800px the sidebar becomes a compact top bar with a menu; the five most used destinations stay one tap away.

Media grids: posters 2:3 for Movies and Shows, 16:9 frames for Home Videos and Other, square-cropped tiles for Photos grouped by month. Grid columns from `repeat(auto-fill, minmax(…))`, never fixed counts.

## Elevation & Depth

No shadows on workspace surfaces. Dialogs and menus use Astryx's own elevation. Media tiles use a 1px inner hairline in place of a shadow so light photos do not bleed into a light page.

## Shapes

Controls 8px, panels 16px, media tiles 10px, tags 6px, progress 4px.

## Components

- **Buttons, inputs, selects, switches, dialogs, toasts, menus, tabs, progress, badges, empty states:** Astryx.
- **Poster/frame tile:** thumbnail, title, one metadata line (year, duration or date). Progress hairline when partially watched. Keyboard focus ring on the whole tile.
- **Player:** native `<video>` with hls.js when HLS is needed; black stage; title, back, and quality/audio menus in Astryx menus.
- **Lightbox:** Astryx `Lightbox` for photos, arrow-key navigation, original download.
- **Download row:** name, source kind, state words, real progress bar (accent), speed/ETA in metadata style, actions in a quiet menu.
- **VPN status:** a tag with a status dot and words ("Protected · NordVPN · Netherlands", "Torrents paused: VPN connecting", "Torrents blocked: no VPN").
- **Drop zone:** the whole app accepts dropped files; a full-window overlay explains where they will go.

## Do's and Don'ts

### Do:
- Let media imagery provide colour; keep chrome neutral.
- Show the real state of downloads and the VPN in words.
- Keep 44px targets, visible focus and system light/dark.
- Say where a file will be moved before moving it.

### Don't:
- Don't add gradients, glass, decorative charts or fake activity.
- Don't use status colours as action colours.
- Don't hide the VPN state when torrents exist.
- Don't show file paths to family members unless they open details.

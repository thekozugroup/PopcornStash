# AGENTS

Project-specific guidance for AI coding agents working in `web/`.

## PopcornStash web UI

- Read ../PRODUCT.md, ../DESIGN.md and ../docs/architecture.md (sections 8 and 10) first. `src/api/types.ts` mirrors section 8; change both together.
- Astryx is compiled from source by `astryxStylex()` in vite.config.ts. Do not import `@astryxdesign/core/astryx.css` (that is the prebuilt path; it would duplicate styles).
- Theme: `src/themes/popcorn/popcornTheme.ts` (extends neutral). After editing it run `npm run theme:build`; `npm run check` fails if the built files are stale. `src/astryx-themes.ts` and `src/themes/popcorn/popcorn.{js,css,d.ts}` are generated.
- Shared pieces: `src/components` (PageHeader, StatusTag, VpnStatusTag, MediaGrid/MediaTile, EmptyPanel, PasskeyNotice), `src/lib/format.ts`, `src/lib/vpn.ts`, `src/api/*` (client, webauthn, events, upload).
- Pages live one per file in `src/pages`; routes are in `src/app/router.tsx`.
- Dropped files arrive on `/downloads` as `location.state.droppedFiles` (see `src/hooks/useFileDrop.ts`).
- Copy: sentence case, plain words, no emoji, no em or en dashes (eslint enforces the dashes).

<!-- ASTRYX:START -->
Astryx v0.6.8 · 168 components
CLI: run every command as `npx astryx <cmd>` (shown below as `astryx ...`).

SETUP (once, in app entry e.g. main.tsx) — without these, components render unstyled:
  import "@astryxdesign/core/reset.css";
  import "@astryxdesign/core/astryx.css";

WORKFLOW — start every page from a template, never from scratch:
1. `astryx build "<idea>"` — names the template to scaffold from and the parts it lacks.
2. `astryx template <name> <path>` — scaffold it; keep its frame, gap and padding; replace the content.
3. `astryx template <Block>` for parts it lacks; `astryx component <Name>` to read props before using one.
Changing an existing page? Keep it: skip step 2 and add blocks/components inside its sections.

RULES:
- No <div> — components handle all layout and spacing.
- Read `astryx docs layout` before changing a template's frame.
- Dense data = rows (Table, List/Item), not Card-wrapped lists. Badge = counts only.
- Style with component props first, then the xstyle prop / StyleX tokens. No raw hex/px.
- Palettes and custom colors go through the theme, never :root overrides. See `astryx docs theme`.
- SELF-CHECK: re-read the file; replace any className=, style={{…}}, raw <div>/<span>, imported .css/@apply, or hardcoded #hex/px with the component or the xstyle prop + a token.

`astryx help` lists every command. Key ones beyond the workflow:
  search "<query>"   find any component / hook / doc / template
  discover <words>   integrations you could add
  docs <topic>       getting-started, principles, tokens, theme … (`astryx docs` lists all)
  docs cli           commands, API reference, integration authoring
  upgrade --from <old version> --apply   run after a dependency bump
<!-- ASTRYX:END -->

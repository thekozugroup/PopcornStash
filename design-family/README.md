# Popcorn family handoff

Give this whole folder to the agents for PopcornSR and PopcornBatch.

Start with `DESIGN.md`, then inspect `preview.html` in a browser. Choose the product and
toggle light/dark. Resize the window to check the shared shell. The preview is offline;
all icons and fonts are bundled in the zip.

`icons/` contains editable Icon Composer sources and native flattened 1024px PNG exports.
`fonts/` includes Newsreader web/Android fonts and its SIL Open Font License. `tokens.json`
and `popcorn.css` provide portable defaults. `references/` contains PopcornStash's current
web theme, Android type, adaptive navigation and Flex-mode implementation as integration
references. Adopt the contract while keeping each project's own workflows and permissions.

Suggested agent instruction:

> Apply the attached Popcorn family DESIGN.md to this product. Use its assigned launcher icon and standalone web popcorn symbol,
> Newsreader headings, neutral surfaces and common spacing. Refine the existing components
> and navigation; preserve all functional and security behavior. Verify actual light/dark,
> compact/wide, large-text, keyboard and error states. Commit, test and report the completed
> changes under this repository's own instructions.

Build the zip again from PopcornStash with `python3 scripts/package-design-family.py`.
The archive includes a SHA256SUMS file for all packaged assets.

The kit's code and design documentation follow PopcornStash's AGPL-3.0-or-later license
(`LICENSE`). Newsreader retains its OFL license; the popcorn glyph comes from Lucide
under ISC (`icons/Lucide-LICENSE`). Icon material and lighting were rendered by Apple
Icon Composer. The full source and flattened export are retained for reproducibility.

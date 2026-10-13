Commit and push as thekozugroup <thekozugroup@gmail.com>. No AI co-author tags.
Keep replies concise; explain the user impact before technical detail.
Keep main as the session-end source of truth. Commit only intended files.
Read PRODUCT.md and docs/architecture.md before changing behavior. docs/architecture.md is the API and data contract; update it in the same commit when a contract changes.
Use the installed Taste (`.claude/skills/design-taste-frontend`) and Impeccable (`.claude/skills/impeccable`) skills for every design change and every design review. Build UI from Astryx (`@astryxdesign/core`) components; look up props with `npm --prefix web run astryx -- component <Name> --props`.
Rust does the processing and backend work. TypeScript is limited to the web UI and the Chrome extension.
Before shipping, run: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `npm --prefix web run check`, `npm --prefix web run build`, `npm --prefix extension run check`, `npm --prefix extension run zip`, and the Docker smoke test (`scripts/docker-smoke.sh`). `scripts/check.sh` runs them all in that order and stops at the first failure (`--quick` skips Docker). There is no CI service; releases are made locally with `scripts/release.sh` (README, Releasing).
Never move, rename or delete files outside `Inbox/` and `Downloads/` without an explicit user action. Never route torrent traffic outside the VPN tunnel. Never weaken the passkey requirement.
Record current scope and evidence in working.md. No credentials, VPN files, tokens or personal media in commits.

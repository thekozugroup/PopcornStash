#!/usr/bin/env bash
# Everything AGENTS.md asks for before shipping, in one command, on this machine.
# There is no CI service; .githooks/pre-push can run this before every push to main.
#
#   scripts/check.sh            Rust, web UI, extension, Android app, Docker image and smoke test
#   scripts/check.sh --quick    the same without Docker
#   scripts/check.sh --e2e      also the Playwright suite in tests/e2e
#   scripts/check.sh --full     also tests/e2e, scripts/vpn-e2e.sh and scripts/fullstack-e2e.sh
#   scripts/check.sh --audit    also cargo audit and npm audit --omit=dev (web, extension)
#
# Options combine (scripts/release.sh runs --quick --e2e --audit). Steps run in this
# order and the script stops at the first failure:
#   git ignores the usual secret file names (VPN credentials, keys, tokens, .env)
#   and tracks none of them; every manifest says AGPL-3.0-or-later
#   cargo fmt --all --check
#   cargo clippy --workspace --all-targets --locked -- -D warnings
#   cargo test --workspace --locked
#   web/: npm ci (when node_modules is missing, or package.json or package-lock.json
#         changed since the last install), npm run check, npm run build
#   extension/: the same, then npm run zip (popcornstash-extension.zip)
#   android/: ./gradlew test lint assembleDebug verifyRoborazziDebug checkLucideGlyphs
#         (unit and Robolectric UI tests, lint, the debug APK, the committed
#         screenshots and the Lucide glyphs); skipped with a note when no Android SDK
#         is found (ANDROID_HOME, ANDROID_SDK_ROOT or android/local.properties)
#   scripts/docker-smoke.sh, which builds IMAGE from the tree and tests it
# It ends with a PASS/FAIL/SKIP line for every step and exits non-zero on failure.
#
# Environment (passed on to the Docker scripts, which document them):
#   IMAGE              image to build and test (default popcornstash:dev)
#   SKIP_BUILD=1       test an existing IMAGE instead of building it
#   REGISTRY           base image registry (default docker.io/library)
#   DOCKER_HUB_MIRROR=1  use mirror.gcr.io/library when REGISTRY is not set
#   BUILD_CA           CA bundle for builds behind a TLS-intercepting proxy
#   E2E_PORT, E2E_CHROMIUM, STASH_PORT, SUBNET_PREFIX  see tests/*/README.md
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  sed -n '2,/^set -Eeuo/{/^set -Eeuo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

docker_checks=1
e2e=0
full=0
audit=0
for arg in "$@"; do
  case "$arg" in
    --quick) docker_checks=0 ;;
    --audit) audit=1 ;;
    --e2e) e2e=1 ;;
    --full)
      e2e=1
      full=1
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      printf 'check.sh: unknown option %s (see --help)\n' "$arg" >&2
      exit 2
      ;;
  esac
done
if ((full && !docker_checks)); then
  printf 'check.sh: --full runs the Docker tests, so it cannot be combined with --quick\n' >&2
  exit 2
fi

export IMAGE="${IMAGE:-popcornstash:dev}"

# ------------------------------------------------------------- reporting ----
declare -a STEPS=() RESULTS=() TIMES=()
started=$SECONDS

bold=""
plain=""
if [[ -t 1 ]]; then
  bold=$'\033[1m'
  plain=$'\033[0m'
fi

say() {
  printf '\n%s==> %s%s\n' "$bold" "$1" "$plain"
}

summary() {
  local i
  printf '\n---------------------------------------------------------------- summary\n'
  for i in "${!STEPS[@]}"; do
    printf '%-5s %-64s %6s\n' "${RESULTS[i]}" "${STEPS[i]}" "${TIMES[i]}"
  done
}

# step "description" command...: runs one step; on failure prints the summary and exits.
step() {
  local what="$1" t0=$SECONDS status=0
  shift
  say "$what"
  "$@" || status=$?
  STEPS+=("$what")
  TIMES+=("$((SECONDS - t0))s")
  if ((status == 0)); then
    RESULTS+=("PASS")
    return 0
  fi
  RESULTS+=("FAIL")
  summary
  printf '\nFAIL  %s (exit %s). Its output is above.\n' "$what" "$status"
  exit 1
}

skip() {
  STEPS+=("$1")
  RESULTS+=("SKIP")
  TIMES+=("-")
}

# ----------------------------------------------------------------- tools ----
need() {
  local missing=() tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
  done
  if ((${#missing[@]})); then
    printf 'missing on the PATH: %s\n' "${missing[*]}"
    printf 'The Rust tests use ffmpeg, ffprobe and vips (Debian/Ubuntu: apt-get install ffmpeg libvips-tools libssl-dev pkg-config);\n'
    printf 'the Docker tests use docker (with buildx and compose), curl and jq; --audit uses cargo-audit (cargo install cargo-audit --locked).\n'
    return 1
  fi
}

tools() {
  local want=(cargo rustfmt node npm ffmpeg ffprobe vips)
  if ((docker_checks)); then
    want+=(docker curl jq)
  fi
  if ((audit)); then
    want+=(cargo-audit)
  fi
  need "${want[@]}" || return 1
  if ((docker_checks)) && ! docker info >/dev/null 2>&1; then
    printf 'Docker is installed but not answering (is the daemon running?). Use --quick to skip the image checks.\n'
    return 1
  fi
}

# ------------------------------------------------------------------- npm ----
# npm ci in DIR when node_modules is missing, or package.json or package-lock.json
# changed after the last install (npm writes node_modules/.package-lock.json on every
# install). npm ci fails when package.json and package-lock.json disagree, so an edit
# to package.json without npm install is caught here.
npm_install() {
  local dir="$1" installed="$1/node_modules/.package-lock.json"
  if [[ -f "$installed" && ! "$dir/package-lock.json" -nt "$installed" && ! "$dir/package.json" -nt "$installed" ]]; then
    printf 'node_modules is up to date with package.json and package-lock.json\n'
    return 0
  fi
  npm --prefix "$dir" ci --ignore-scripts --no-audit --no-fund
}

# ------------------------------------------------------------------ e2e ----
# CI=1 makes Playwright refuse a stray test.only (forbidOnly in playwright.config.ts).
e2e_suite() {
  cargo build -p popcornstash --locked || return 1
  npm --prefix tests/e2e run check || return 1
  CI=1 npm --prefix tests/e2e test
}

# --------------------------------------------------------------- android ----
# The Android app builds with the SDK named by ANDROID_HOME, ANDROID_SDK_ROOT or
# android/local.properties (sdk.dir). Without one the Android steps are skipped.
android_sdk() {
  local dir="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  if [[ -z "$dir" && -f android/local.properties ]]; then
    dir="$(sed -n 's/^sdk\.dir=//p' android/local.properties | tail -n 1)"
  fi
  [[ -n "$dir" && -d "$dir/platforms" ]]
}

gradle() {
  (cd android && ./gradlew --console=plain "$@")
}

# --------------------------------------------------------------- secrets ----
# The names PopcornStash and VPN providers give to secrets (.dockerignore keeps the
# same ones out of image builds) must be ignored by git, and none may be tracked.
SECRET_NAMES=(
  .env config/.env.local openvpn.auth config/torrent/vpn/openvpn.auth nord.ovpn.auth
  internal.token config/torrent/internal.token server.key tls/cert.pem
  wireguard.conf us1234.ovpn popcornstash.db
  android/release.jks android/upload.keystore android/local.properties
)
secrets_ignored() {
  local listing status=0 not_ignored tracked
  # Exit 1 only means that none of the names is ignored; -n -v lists each either way.
  listing="$(git check-ignore --no-index -v -n "${SECRET_NAMES[@]}")" || status=$?
  ((status <= 1)) || return 1
  not_ignored="$(awk -F'\t' '$1 == "::" { print $2 }' <<<"$listing")"
  if [[ -n "$not_ignored" ]]; then
    printf 'not ignored by .gitignore: %s\n' "$(tr '\n' ' ' <<<"$not_ignored")"
    return 1
  fi
  tracked="$(git ls-files -ci --exclude-standard)"
  if [[ -n "$tracked" ]]; then
    printf 'tracked although .gitignore lists them: %s\n' "$(tr '\n' ' ' <<<"$tracked")"
    return 1
  fi
  printf 'git ignores %s secret file names, and none is tracked\n' "${#SECRET_NAMES[@]}"
}

# --------------------------------------------------------------- license ----
# Every manifest declares the project's license, AGPL-3.0-or-later (LICENSE).
PROJECT_LICENSE="AGPL-3.0-or-later"
licenses_agree() {
  local wrong=() dir got
  got="$(awk '/^\[/ { s = $0 } s == "[workspace.package]" && /^license *= *"/ { sub(/^license *= *"/, ""); sub(/".*$/, ""); print; exit }' Cargo.toml)"
  [[ "$got" == "$PROJECT_LICENSE" ]] || wrong+=("Cargo.toml [workspace.package]: $got")
  for dir in web extension tests/e2e tests/fullstack; do
    got="$(jq -r '.license // "none"' "$dir/package.json")"
    [[ "$got" == "$PROJECT_LICENSE" ]] || wrong+=("$dir/package.json: $got")
    got="$(jq -r '.packages[""].license // "none"' "$dir/package-lock.json")"
    [[ "$got" == "$PROJECT_LICENSE" ]] || wrong+=("$dir/package-lock.json (root package): $got")
  done
  got="$(sed -n 's/.*org\.opencontainers\.image\.licenses="\([^"]*\)".*/\1/p' Dockerfile)"
  [[ "$got" == "$PROJECT_LICENSE" ]] || wrong+=("Dockerfile image label: $got")
  if ((${#wrong[@]})); then
    printf 'not %s: %s\n' "$PROJECT_LICENSE" "${wrong[*]}"
    return 1
  fi
  printf 'every manifest says %s\n' "$PROJECT_LICENSE"
}

# ----------------------------------------------------------------- steps ----
mode="Rust, web UI, extension, Android app"
((audit)) && mode+=", cargo audit and npm audit"
((docker_checks)) && mode+=", Docker image ($IMAGE)"
((e2e)) && mode+=", tests/e2e"
((full)) && mode+=", VPN and full-stack Docker tests"
printf 'PopcornStash checks: %s\n' "$mode"

step "tools are installed" tools
step "git ignores secret file names" secrets_ignored
step "license fields say $PROJECT_LICENSE" licenses_agree
# .cargo/audit.toml lists the reviewed advisories cargo audit may skip, and why.
if ((audit)); then
  step "cargo audit" cargo audit
  step "web: npm audit --omit=dev" npm --prefix web audit --omit=dev
  step "extension: npm audit --omit=dev" npm --prefix extension audit --omit=dev
else
  skip "cargo audit and npm audit (--audit)"
fi
step "cargo fmt --all --check" cargo fmt --all --check
step "cargo clippy --workspace --all-targets --locked -- -D warnings" \
  cargo clippy --workspace --all-targets --locked -- -D warnings
step "cargo test --workspace --locked" cargo test --workspace --locked

step "web: npm ci (if needed)" npm_install web
step "web: npm run check" npm --prefix web run check
step "web: npm run build" npm --prefix web run build

step "extension: npm ci (if needed)" npm_install extension
step "extension: npm run check" npm --prefix extension run check
step "extension: npm run build" npm --prefix extension run build
step "extension: npm run zip" npm --prefix extension run zip

if android_sdk; then
  step "android: test, lint, assembleDebug, screenshots, glyphs" \
    gradle test lint assembleDebug verifyRoborazziDebug checkLucideGlyphs
else
  printf '\nNo Android SDK found (set ANDROID_HOME, or sdk.dir in android/local.properties): skipping the Android app.\n'
  skip "android (no Android SDK)"
fi

if ((e2e)); then
  step "tests/e2e: npm ci (if needed)" npm_install tests/e2e
  step "tests/e2e: Playwright suite" e2e_suite
else
  skip "tests/e2e (--e2e or --full)"
fi

if ((docker_checks)); then
  step "Docker image and smoke test (scripts/docker-smoke.sh)" scripts/docker-smoke.sh
else
  skip "Docker image and smoke test (--quick)"
fi

if ((full)); then
  step "VPN kill switch in Docker (scripts/vpn-e2e.sh)" env SKIP_BUILD=1 scripts/vpn-e2e.sh
  step "full stack in Docker (scripts/fullstack-e2e.sh)" env SKIP_BUILD=1 CI=1 scripts/fullstack-e2e.sh
else
  skip "VPN and full-stack Docker tests (--full)"
fi

summary
printf '\nPASS  all checks (%ss). Extension zip: extension/popcornstash-extension.zip' "$((SECONDS - started))"
if ((docker_checks)); then
  printf '; image: %s' "$IMAGE"
fi
printf '\n'

#!/usr/bin/env bash
# Third-party license notices for what the PopcornStash image ships. The outputs are
# committed, and the Dockerfile copies them to /usr/share/doc/popcornstash/.
#
#   scripts/notices.sh           regenerate THIRD_PARTY_LICENSES and THIRD_PARTY_LICENSES-web
#   scripts/notices.sh --check   regenerate them elsewhere and fail if the committed ones differ
#
# THIRD_PARTY_LICENSES: every Rust crate built into popcornstash (this workspace and
#   Cargo.lock) and into boringtun-cli (the crates.io release the Dockerfile builds,
#   with its own Cargo.lock), grouped by license text, from cargo-about and
#   scripts/notices/about.toml. PopcornStash's own crates are left out (LICENSE).
# THIRD_PARTY_LICENSES-web: every npm package the web app can bundle (the non-dev
#   packages in web/package-lock.json, without type definitions) with the license
#   files it ships, read from web/node_modules (npm ci first).
#
# A package that ships no license file gets its upstream license from the release it
# was published from, pinned by SHA-256 (boringtun in about.toml, npm packages in
# UPSTREAM_LICENSES below). The run fails when a license cannot be found.
#
# Needs cargo-about 0.9.2 (ABOUT_VERSION below; cargo install --locked cargo-about --version
# 0.9.2 --features cli), jq, curl and network access (crates.io, GitHub).
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

ABOUT_VERSION=0.9.2
ABOUT_CONFIG=scripts/notices/about.toml
RUST_OUT=THIRD_PARTY_LICENSES
WEB_OUT=THIRD_PARTY_LICENSES-web

# npm packages published without a license file: "name version url sha256".
UPSTREAM_LICENSES=(
  "@astryxdesign/core 0.6.8 https://raw.githubusercontent.com/facebook/astryx/v0.6.8/LICENSE a6855be541fc8f446acd1bc4f2f8efce1ace6dce71dba32fcd8da553ee54b473"
  "@stylexjs/stylex 0.19.1 https://raw.githubusercontent.com/facebook/stylex/0.19.1/LICENSE da6d3703ed11cbe42bd212c725957c98da23cbff1998c05fa4b3d976d1a58e93"
)

check=0
case "${1:-}" in
  "") ;;
  --check) check=1 ;;
  -h | --help)
    sed -n '2,/^set -Eeuo/{/^set -Eeuo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
    exit 0
    ;;
  *)
    printf 'notices.sh: unknown option %s (see --help)\n' "$1" >&2
    exit 2
    ;;
esac

die() {
  printf 'notices.sh: %s\n' "$1" >&2
  exit 1
}

for tool in cargo jq curl tar cmp; do
  command -v "$tool" >/dev/null 2>&1 || die "needs '$tool' on the PATH"
done

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}
about_version="$(cargo about --version 2>/dev/null | awk '{ print $2 }')" ||
  die "needs cargo-about: cargo install --locked cargo-about --version $ABOUT_VERSION --features cli"
[[ "$about_version" == "$ABOUT_VERSION" ]] ||
  die "needs cargo-about $ABOUT_VERSION, not '${about_version:-none}' (other versions word the output differently): cargo install --locked cargo-about --version $ABOUT_VERSION --features cli"
[[ -d web/node_modules ]] || die "web/node_modules is missing: run npm --prefix web ci first"

work="$(mktemp -d "${TMPDIR:-/tmp}/popcornstash-notices.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# download URL SHA256 FILE: fetch a file and check it against its pinned checksum.
download() {
  curl -fsSL -m 60 -o "$3" "$1" || die "could not download $1"
  local got
  got="$(sha256 "$3")"
  [[ "$got" == "$2" ]] || die "$1 has SHA-256 $got, not the pinned $2"
}

# ------------------------------------------------------------------- Rust ----
# about JSON_OUT [cargo-about options...]
about() {
  local out="$1"
  shift
  cargo about generate --locked --fail -c "$ABOUT_CONFIG" --format json -o "$out" "$@" 2>"$work/about.log" || {
    cat "$work/about.log" >&2
    die "cargo-about failed (log above)"
  }
}

printf 'notices: Rust crates in popcornstash\n' >&2
about "$work/popcornstash.json" --workspace

# boringtun-cli exactly as the Dockerfile fetches it.
bt_version="$(sed -n 's/^ARG BORINGTUN_VERSION=//p' Dockerfile)"
bt_sha="$(sed -n 's/^ARG BORINGTUN_CLI_SHA256=//p' Dockerfile)"
[[ -n "$bt_version" && -n "$bt_sha" ]] || die "could not read BORINGTUN_VERSION and BORINGTUN_CLI_SHA256 from the Dockerfile"
printf 'notices: Rust crates in boringtun-cli %s\n' "$bt_version" >&2
download "https://static.crates.io/crates/boringtun-cli/boringtun-cli-$bt_version.crate" "$bt_sha" "$work/boringtun-cli.crate"
tar -xzf "$work/boringtun-cli.crate" -C "$work"
about "$work/boringtun-cli.json" --manifest-path "$work/boringtun-cli-$bt_version/Cargo.toml"

own_crates="$(cargo metadata --no-deps --format-version 1 --locked | jq -c '[.packages[].name]')"

# One section per license text, each listing the crates it covers and the program
# (popcornstash, boringtun-cli) they are built into.
jq -n -r --argjson own "$own_crates" \
  --slurpfile pst "$work/popcornstash.json" --slurpfile bt "$work/boringtun-cli.json" '
  def entries($doc; $program):
    $doc.licenses[] | {id, name, text,
      crates: [.used_by[].crate | select(.name as $n | $own | index($n) | not)
        | {name, version, repository, program: $program,
           authors: ([(.authors // [])[] | sub(" *<[^>]*>"; "")] | join(", "))}]}
    | select(.crates | length > 0);
  def crate_line:
    "  \(.name) \(.version) (\(.programs | join(", ")))"
    + (if .authors != "" then ", by \(.authors)" else "" end)
    + (if .repository then "\n      \(.repository)" else "" end);
  def rule: "--------------------------------------------------------------------------------";
  [entries($pst[0]; "popcornstash"), entries($bt[0]; "boringtun-cli")]
  | group_by(.text)
  | map({id: .[0].id, name: .[0].name, text: .[0].text,
         crates: ([.[].crates[]] | group_by([.name, .version])
                  | map(.[0] + {programs: (map(.program) | unique)}) | sort_by(.name, .version))})
  | sort_by(.id, .crates[0].name) as $sections
  | ([$sections[].crates[] | "\(.name) \(.version)"] | unique | length) as $count
  | ($sections | group_by(.id)
     | map(([.[].crates[] | "\(.name) \(.version)"] | unique | length) as $n
           | "  \(.[0].id): \($n) crate\(if $n == 1 then "" else "s" end)")
     | join("\n")) as $overview
  | "Third-party software in PopcornStash: Rust crates\n"
    + "==================================================\n\n"
    + "The popcornstash and boringtun-cli programs in the PopcornStash image are built\n"
    + "from the \($count) Rust crates below. Each section is one license text followed by\n"
    + "the crates it covers. PopcornStash itself is licensed under the GNU Affero\n"
    + "General Public License, version 3 or later (LICENSE and NOTICE).\n\n"
    + "Generated by scripts/notices.sh (cargo-about); do not edit by hand.\n\n"
    + "Licenses:\n\($overview)\n"
    + ($sections | map("\n" + rule + "\n\(.name) (\(.id))\n\nUsed by:\n"
        + (.crates | map(crate_line) | join("\n")) + "\n\n" + (.text | sub("\\s+$"; "")) + "\n") | join(""))
' >"$work/$RUST_OUT"

# -------------------------------------------------------------------- web ----
printf 'notices: npm packages in the web app\n' >&2
upstream_license() {
  local name="$1" version="$2" entry n v url sha
  for entry in "${UPSTREAM_LICENSES[@]}"; do
    read -r n v url sha <<<"$entry"
    if [[ "$n" == "$name" && "$v" == "$version" ]]; then
      download "$url" "$sha" "$work/upstream-license"
      printf '(No license file in the npm package; from %s)\n\n' "$url"
      cat "$work/upstream-license"
      return 0
    fi
  done
  return 1
}

{
  printf 'Third-party software in PopcornStash: web app\n'
  printf '=============================================\n\n'
  printf 'The PopcornStash web app (served by popcornstash, and at /third-party-licenses.txt)\n'
  printf 'bundles code from the npm packages below, each followed by its license.\n\n'
  printf 'Generated by scripts/notices.sh from web/package-lock.json; do not edit by hand.\n'
  # The packages a production install has: no dev tools, and no type definitions,
  # which never reach the bundle.
  jq -r '.packages | to_entries[]
    | select(.key != "" and (.value.dev // false | not) and (.key | test("node_modules/@types/") | not))
    | [.key, .value.version, (.value.license // "UNKNOWN")] | @tsv' web/package-lock.json |
    LC_ALL=C sort |
    while IFS=$'\t' read -r path version license; do
      name="${path##*node_modules/}"
      dir="web/$path"
      [[ -f "$dir/package.json" ]] || die "$dir is not installed: run npm --prefix web ci"
      installed="$(jq -r .version "$dir/package.json")"
      [[ "$installed" == "$version" ]] || die "$dir is $installed, but package-lock.json says $version: run npm --prefix web ci"
      repo="$(jq -r '(.repository | if type == "object" then .url else . end) // .homepage // empty' "$dir/package.json" |
        sed -E 's#^git\+##; s#^git://#https://#; s#\.git$##; s#^github:#https://github.com/#')"
      printf '\n--------------------------------------------------------------------------------\n'
      printf '%s %s (%s)\n' "$name" "$version" "$license"
      [[ -n "$repo" ]] && printf '%s\n' "$repo"
      printf '\n'
      mapfile -t files < <(find "$dir" -maxdepth 1 -type f \( -iname 'licen[cs]e*' -o -iname 'copying*' \) | LC_ALL=C sort)
      if ((${#files[@]})); then
        for file in "${files[@]}"; do
          sed -e 's/\r$//' "$file"
          printf '\n'
        done
      elif ! upstream_license "$name" "$version"; then
        die "$name $version ships no license file and has no pinned upstream license: add one to UPSTREAM_LICENSES in scripts/notices.sh"
      fi
    done
} >"$work/$WEB_OUT"

# ----------------------------------------------------------------- result ----
if ((check)); then
  stale=()
  for file in "$RUST_OUT" "$WEB_OUT"; do
    cmp -s "$work/$file" "$file" || stale+=("$file")
  done
  if ((${#stale[@]})); then
    for file in "${stale[@]}"; do
      diff -u "$file" "$work/$file" | head -n 20 >&2 || true
    done
    die "out of date: ${stale[*]}. Run scripts/notices.sh and commit the result."
  fi
  printf 'notices: %s and %s are up to date\n' "$RUST_OUT" "$WEB_OUT" >&2
else
  cp "$work/$RUST_OUT" "$RUST_OUT"
  cp "$work/$WEB_OUT" "$WEB_OUT"
  printf 'notices: wrote %s (%s bytes) and %s (%s bytes)\n' \
    "$RUST_OUT" "$(wc -c <"$RUST_OUT" | tr -d ' ')" "$WEB_OUT" "$(wc -c <"$WEB_OUT" | tr -d ' ')" >&2
fi

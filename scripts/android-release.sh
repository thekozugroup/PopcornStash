#!/usr/bin/env bash
# Publish the Android app from main as a GitHub release the app updates itself from.
# README.md ("Releasing", Android app) has the one-time setup; docs/android.md says how
# the app finds and installs these builds.
#
#   scripts/android-release.sh             build, sign, verify and publish build N
#   scripts/android-release.sh --dry-run   build, sign and verify; print what it would publish
#   scripts/android-release.sh --force     publish even when nothing under android/ changed
#
# Run it after pushing to main a change that touches android/ (AGENTS.md). It refuses
# unless the tree is clean, on main, and HEAD equals REMOTE/main after a fetch. The build
# number N is the number of commits on main (git rev-list --count HEAD), so it only ever
# grows: versionCode N, versionName "<Cargo workspace version>+N" (0.1.0+412). When no
# file under android/ changed since the newest android-* tag it says so and stops
# (exit 0), unless --force.
#
# The APK is signed with the release key from the environment, never from the
# repository, and checked with apksigner: its certificate's SHA-256 must equal
# android/release-cert.sha256 (the public fingerprint, committed). Without that file the
# script refuses to publish and says how to fill it.
#
# The release is tagged android-N on HEAD, named "Android app, build N", and marked as a
# pre-release, so GitHub's "latest release" stays the server's. Its notes are the commit
# and the subjects of the commits since the previous android-* tag that touch android/.
# It is made as a draft, given its two assets (popcornstash-android-N.apk and
# popcornstash-android-N.apk.sha256), and only then published, so the app never sees a
# build without its files. A dry run publishes nothing; the files are left in
# target/android-releases/android-N/ either way.
#
# Environment:
#   POPCORNSTASH_KEYSTORE_B64       the keystore, base64 encoded (decoded to a private
#                                   temporary file that is deleted on exit), or
#   POPCORNSTASH_KEYSTORE           the path to the keystore (.jks or .p12)
#   POPCORNSTASH_KEYSTORE_PASSWORD  keystore password
#   POPCORNSTASH_KEY_ALIAS          key alias
#   POPCORNSTASH_KEY_PASSWORD       key password (defaults to the keystore password)
#   GITHUB_RELEASE_TOKEN            a token that can create releases (contents: write).
#                                   Sent as a Bearer token when set; when it is not, no
#                                   Authorization header is sent (a network rule of the
#                                   environment may add one). Never printed.
#   GITHUB_REPO                     where the release goes (default thekozugroup/PopcornStash)
#   GITHUB_API_URL                  default https://api.github.com
#   GITHUB_UPLOADS_URL              default https://uploads.github.com
#   REMOTE                          git remote main is compared with (default origin)
#   ANDROID_HOME, ANDROID_SDK_ROOT  the Android SDK (or sdk.dir in android/local.properties)
#   GRADLE_FLAGS                    extra Gradle arguments, split at spaces (--max-workers=2)
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  sed -n '2,/^set -Eeuo/{/^set -Eeuo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

# Secrets stay in shell variables: Gradle gets only what it signs with, and nothing
# below prints them.
release_token="${GITHUB_RELEASE_TOKEN:-}"
keystore_b64="${POPCORNSTASH_KEYSTORE_B64:-}"
unset GITHUB_RELEASE_TOKEN POPCORNSTASH_KEYSTORE_B64

dry_run=0
force=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --force) force=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      printf 'android-release.sh: unknown argument %s (see --help)\n' "$arg" >&2
      exit 2
      ;;
  esac
done

GITHUB_REPO="${GITHUB_REPO:-thekozugroup/PopcornStash}"
GITHUB_API_URL="${GITHUB_API_URL:-https://api.github.com}"
GITHUB_UPLOADS_URL="${GITHUB_UPLOADS_URL:-https://uploads.github.com}"
GITHUB_API_URL="${GITHUB_API_URL%/}"
GITHUB_UPLOADS_URL="${GITHUB_UPLOADS_URL%/}"
REMOTE="${REMOTE:-origin}"
CERT_FILE="android/release-cert.sha256"
APK_BUILT="android/app/build/outputs/apk/release/app-release.apk"

# ------------------------------------------------------------- reporting ----
problems=()

say() {
  printf '\n==> %s\n' "$1"
}

ok() {
  printf 'ok    %s\n' "$1"
}

note() {
  printf 'note  %s\n' "$1"
}

# A problem stops a real run before anything is built. A dry run lists it and carries on.
problem() {
  problems+=("$1")
  printf 'FAIL  %s\n' "$1"
}

die() {
  printf '\nFAIL  %s\n' "$1" >&2
  exit 1
}

need() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || die "this script needs '$tool' on the PATH"
  done
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# A fingerprint as 64 lowercase hex digits, whatever the colons, spaces or case.
normalize_fingerprint() {
  tr -d ': \t\r\n' | tr '[:upper:]' '[:lower:]'
}

# ------------------------------------------------------------- temp files ----
work_dir="$(mktemp -d)"
chmod 700 "$work_dir"
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

need git curl jq awk

# ------------------------------------------------------------- preflight ----
say "Preflight"

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [[ "$branch" == main ]]; then
  ok "on main"
else
  problem "not on main (on ${branch:-a detached HEAD}): builds are published from main only"
fi

dirty="$(git status --porcelain --untracked-files=normal)"
if [[ -z "$dirty" ]]; then
  ok "no uncommitted or untracked files"
else
  problem "the tree has uncommitted or untracked files; commit or remove them first:"$'\n'"$dirty"
fi

head_sha="$(git rev-parse HEAD)"
if git fetch --quiet "$REMOTE" main 2>"$work_dir/fetch.err"; then
  remote_sha="$(git rev-parse --verify --quiet "refs/remotes/$REMOTE/main" || true)"
  if [[ "$remote_sha" == "$head_sha" ]]; then
    ok "HEAD is $REMOTE/main (${head_sha:0:12})"
  else
    problem "HEAD (${head_sha:0:12}) is not $REMOTE/main (${remote_sha:0:12}); push main, or pull, first"
  fi
else
  problem "could not fetch main from $REMOTE: $(tail -n 1 "$work_dir/fetch.err")"
fi
# The tags GitHub made for earlier builds; a failure here only means none are known.
git fetch --quiet "$REMOTE" '+refs/tags/android-*:refs/tags/android-*' 2>/dev/null || true

build="$(git rev-list --count HEAD)"
tag="android-$build"
cargo_version="$(awk '/^\[/ { s = $0 } s == "[workspace.package]" && /^version *= *"/ { sub(/^version *= *"/, ""); sub(/".*$/, ""); print; exit }' Cargo.toml)"
[[ -n "$cargo_version" ]] || die "no version in Cargo.toml [workspace.package]"
version_name="$cargo_version+$build"
ok "build $build: versionCode $build, versionName $version_name"

# The newest earlier build, by number.
previous_tag="$(git tag --list 'android-*' | awk -F- '$2 ~ /^[0-9]+$/ && $2 < '"$build"' { print $2 }' | sort -n | tail -n 1)"
previous_tag="${previous_tag:+android-$previous_tag}"
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
  printf '\nBuild %s is already published (tag %s). Nothing to do.\n' "$build" "$tag"
  exit 0
fi
if [[ -n "$previous_tag" ]]; then
  if git diff --quiet "$previous_tag" HEAD -- android/; then
    if ((force)); then
      note "nothing under android/ changed since $previous_tag; --force publishes anyway"
    else
      printf '\nNothing under android/ changed since %s. Skipping (use --force to publish anyway).\n' "$previous_tag"
      exit 0
    fi
  else
    ok "android/ changed since $previous_tag"
  fi
else
  note "no earlier android-* build: this is the first"
fi

expected_cert=""
if [[ -s "$CERT_FILE" ]]; then
  expected_cert="$(normalize_fingerprint <"$CERT_FILE")"
fi
if [[ "$expected_cert" =~ ^[0-9a-f]{64}$ ]]; then
  ok "$CERT_FILE holds the release certificate's fingerprint"
elif [[ -n "$expected_cert" ]]; then
  problem "$CERT_FILE is not a SHA-256 fingerprint (64 hex digits, colons allowed)"
else
  problem "$CERT_FILE is missing or empty. Put the release key's certificate SHA-256 in it and commit it:
        keytool -list -v -keystore release.jks -alias ALIAS | grep 'SHA256:'
      (the line after SHA256:, colons and all), or run this script with --dry-run and copy
      the fingerprint it prints for the APK it signed."
fi

# The signing key: from POPCORNSTASH_KEYSTORE_B64, or the POPCORNSTASH_KEYSTORE path.
if [[ -n "$keystore_b64" ]]; then
  keystore_file="$work_dir/release.keystore"
  (
    umask 077
    printf '%s' "$keystore_b64" | tr -d ' \t\r\n' | base64 -d >"$keystore_file" 2>/dev/null
  ) || problem "POPCORNSTASH_KEYSTORE_B64 is not base64"
  chmod 600 "$keystore_file"
  export POPCORNSTASH_KEYSTORE="$keystore_file"
  ok "keystore from POPCORNSTASH_KEYSTORE_B64 (a private temporary file)"
elif [[ -n "${POPCORNSTASH_KEYSTORE:-}" ]]; then
  if [[ -f "$POPCORNSTASH_KEYSTORE" ]]; then
    ok "keystore from POPCORNSTASH_KEYSTORE"
  else
    problem "POPCORNSTASH_KEYSTORE names a file that does not exist"
  fi
else
  problem "no signing key: set POPCORNSTASH_KEYSTORE_B64 (or POPCORNSTASH_KEYSTORE), POPCORNSTASH_KEYSTORE_PASSWORD and POPCORNSTASH_KEY_ALIAS"
fi
[[ -n "${POPCORNSTASH_KEYSTORE_PASSWORD:-}" ]] || problem "POPCORNSTASH_KEYSTORE_PASSWORD is not set"
[[ -n "${POPCORNSTASH_KEY_ALIAS:-}" ]] || problem "POPCORNSTASH_KEY_ALIAS is not set"

# The Android SDK's build tools, for apksigner and aapt2.
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$sdk" && -f android/local.properties ]]; then
  sdk="$(sed -n 's/^sdk\.dir=//p' android/local.properties | tail -n 1)"
fi
build_tools=""
if [[ -n "$sdk" && -d "$sdk/build-tools" ]]; then
  build_tools="$(find "$sdk/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
fi
if [[ -n "$build_tools" && -x "$build_tools/apksigner" && -x "$build_tools/aapt2" ]]; then
  ok "build tools: $build_tools"
else
  problem "no Android SDK build tools with apksigner and aapt2 (set ANDROID_HOME, or sdk.dir in android/local.properties)"
fi

if ((${#problems[@]})); then
  if ((dry_run)); then
    printf '\nA release would stop here (%d problem(s) above). The dry run carries on.\n' "${#problems[@]}"
  else
    die "preflight found ${#problems[@]} problem(s); nothing was built or published"
  fi
fi

# ----------------------------------------------------------------- build ----
say "Build $tag (release, signed)"
gradle_flags=()
if [[ -n "${GRADLE_FLAGS:-}" ]]; then
  read -ra gradle_flags <<<"$GRADLE_FLAGS"
fi
rm -f "$APK_BUILT"
(cd android && ./gradlew --console=plain "${gradle_flags[@]}" :app:assembleRelease \
  -PpopcornstashVersionCode="$build" -PpopcornstashVersionName="$version_name")
[[ -f "$APK_BUILT" ]] || die "Gradle finished without $APK_BUILT"

# ---------------------------------------------------------------- verify ----
say "Verify"
[[ -n "$build_tools" ]] || die "cannot verify the APK without the build tools"
if ! "$build_tools/apksigner" verify --print-certs "$APK_BUILT" >"$work_dir/certs.txt" 2>"$work_dir/apksigner.err"; then
  die "apksigner does not accept the APK (is the signing key set?): $(tail -n 1 "$work_dir/apksigner.err")"
fi
signers="$(grep -c 'certificate SHA-256 digest' "$work_dir/certs.txt" || true)"
((signers == 1)) || die "the APK has $signers signers; it must have exactly one"
actual_cert="$(sed -n 's/^Signer #1 certificate SHA-256 digest: //p' "$work_dir/certs.txt" | normalize_fingerprint)"
ok "apksigner verifies the APK; its certificate SHA-256 is $actual_cert"
if [[ "$expected_cert" =~ ^[0-9a-f]{64}$ ]]; then
  if [[ "$actual_cert" == "$expected_cert" ]]; then
    ok "it matches $CERT_FILE"
  else
    die "the APK is signed by $actual_cert, but $CERT_FILE says $expected_cert. The app on phones would refuse this update."
  fi
else
  note "no fingerprint in $CERT_FILE to compare with; if this is the release key, put the value above in it"
fi
"$build_tools/aapt2" dump badging "$APK_BUILT" >"$work_dir/badging.txt"
package_line="$(head -n 1 "$work_dir/badging.txt")"
[[ "$package_line" == *"name='com.thekozugroup.popcornstash'"* ]] || die "the APK is not com.thekozugroup.popcornstash: $package_line"
[[ "$package_line" == *"versionCode='$build'"* ]] || die "the APK's versionCode is not $build: $package_line"
[[ "$package_line" == *"versionName='$version_name'"* ]] || die "the APK's versionName is not $version_name: $package_line"
ok "package com.thekozugroup.popcornstash, versionCode $build, versionName $version_name"

out_dir="target/android-releases/$tag"
apk_name="popcornstash-android-$build.apk"
mkdir -p "$out_dir"
cp "$APK_BUILT" "$out_dir/$apk_name"
apk_sha="$(sha256 "$out_dir/$apk_name")"
printf '%s  %s\n' "$apk_sha" "$apk_name" >"$out_dir/$apk_name.sha256"
ok "$out_dir/$apk_name ($(wc -c <"$out_dir/$apk_name" | tr -d ' ') bytes, SHA-256 $apk_sha)"

# ----------------------------------------------------------------- notes ----
notes="$out_dir/notes.md"
{
  printf 'Built from commit %s on main.\n\n' "$head_sha"
  if [[ -n "$previous_tag" ]]; then
    printf 'Changes to the app since build %s:\n\n' "${previous_tag#android-}"
    range="$previous_tag..HEAD"
  else
    printf 'Changes to the app:\n\n'
    range="HEAD"
  fi
  total="$(git rev-list --count "$range" -- android/)"
  git log --no-merges --format='- %s' -n 30 "$range" -- android/
  if ((total > 30)); then
    printf -- '- and %d earlier changes\n' "$((total - 30))"
  fi
  printf '\nThe app installs this build itself (Settings, This app). To install it by hand, download %s and check it against %s.sha256.\n' "$apk_name" "$apk_name"
} >"$notes"

if ((dry_run)); then
  say "Would publish"
  printf 'release   %s, "Android app, build %s", pre-release, on %s (%s)\n' "$tag" "$build" "$head_sha" "$GITHUB_REPO"
  printf 'assets    %s\n          %s.sha256\n' "$apk_name" "$apk_name"
  printf 'notes:\n'
  sed 's/^/    /' "$notes"
  if ((${#problems[@]})); then
    printf '\nDry run finished. A real run would have stopped at the preflight:\n'
    printf '  - %s\n' "${problems[@]%%$'\n'*}"
    exit 1
  fi
  printf '\nDry run finished: nothing was published. Run scripts/android-release.sh to publish.\n'
  exit 0
fi

# --------------------------------------------------------------- publish ----
say "Publish $tag to $GITHUB_REPO"
headers="$work_dir/headers"
(
  umask 077
  {
    printf 'Accept: application/vnd.github+json\n'
    printf 'X-GitHub-Api-Version: 2022-11-28\n'
    if [[ -n "$release_token" ]]; then
      printf 'Authorization: Bearer %s\n' "$release_token"
    fi
  } >"$headers"
)

# github METHOD URL [curl arguments...]: prints the body; the status goes to $work_dir/status.
github() {
  local method="$1" url="$2"
  shift 2
  curl --silent --show-error --location --header "@$headers" --request "$method" \
    --output "$work_dir/response" --write-out '%{http_code}' "$@" "$url" >"$work_dir/status" || printf '000' >"$work_dir/status"
  cat "$work_dir/response" 2>/dev/null || true
}

status() {
  cat "$work_dir/status"
}

github GET "$GITHUB_API_URL/repos/$GITHUB_REPO/releases/tags/$tag" >/dev/null
case "$(status)" in
  404) ok "no $tag release yet" ;;
  200) die "the $tag release already exists on GitHub" ;;
  *) die "could not check for a $tag release (HTTP $(status))" ;;
esac

jq -n --arg tag "$tag" --arg sha "$head_sha" --arg name "Android app, build $build" --rawfile body "$notes" \
  '{tag_name: $tag, target_commitish: $sha, name: $name, body: $body, draft: true, prerelease: true}' >"$work_dir/create.json"
created="$(github POST "$GITHUB_API_URL/repos/$GITHUB_REPO/releases" --header 'Content-Type: application/json' --data-binary "@$work_dir/create.json")"
[[ "$(status)" == 201 ]] || die "GitHub did not create the release (HTTP $(status)): $(jq -r '.message // empty' <<<"$created" 2>/dev/null)"
release_id="$(jq -r '.id' <<<"$created")"
[[ "$release_id" =~ ^[0-9]+$ ]] || die "GitHub's answer has no release id"
ok "draft release $release_id"

unfinished() {
  printf '\nFAIL  %s\n' "$1" >&2
  printf '      The draft release %s (id %s) is not published, so no phone sees it. Delete it on\n' "$tag" "$release_id" >&2
  printf '      https://github.com/%s/releases and run this script again.\n' "$GITHUB_REPO" >&2
  exit 1
}

upload() {
  local file="$1" type="$2" name
  name="$(basename "$file")"
  github POST "$GITHUB_UPLOADS_URL/repos/$GITHUB_REPO/releases/$release_id/assets?name=$name" \
    --header "Content-Type: $type" --data-binary "@$file" >/dev/null
  [[ "$(status)" == 201 ]] || unfinished "uploading $name failed (HTTP $(status))"
  ok "uploaded $name"
}
upload "$out_dir/$apk_name" application/vnd.android.package-archive
upload "$out_dir/$apk_name.sha256" text/plain

published="$(github PATCH "$GITHUB_API_URL/repos/$GITHUB_REPO/releases/$release_id" \
  --header 'Content-Type: application/json' --data-binary '{"draft":false}')"
[[ "$(status)" == 200 ]] || unfinished "publishing the draft failed (HTTP $(status))"
printf '\nPublished build %s: %s\n' "$build" "$(jq -r '.html_url // empty' <<<"$published")"

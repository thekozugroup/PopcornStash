#!/usr/bin/env bash
# Release PopcornStash from this machine. There is no CI service: this script is the
# whole release.
#
#   scripts/release.sh --bump 0.2.0      set every version number to 0.2.0 and stop
#   scripts/release.sh --dry-run 0.2.0   do everything except log in, push and tag
#   scripts/release.sh 0.2.0             release 0.2.0
#
# 1. Preflight: VERSION is MAJOR.MINOR.PATCH (optionally -prerelease); the branch is
#    main with no uncommitted or untracked files and equals REMOTE/main; tag vVERSION
#    does not exist yet; CHANGELOG.md has a "## [VERSION]" section; the Cargo
#    workspace (and Cargo.lock), web/ and extension/ package.json and
#    package-lock.json, and extension/static/manifest.json all say VERSION; arm64
#    emulation is available; Docker is logged in to the registry.
# 2. scripts/check.sh: Rust, web UI, extension, Docker image and smoke test.
# 3. Images for linux/amd64 and linux/arm64. The Rust code is cross-compiled; only
#    the runtime stage's apt-get runs under emulation. The amd64 image must have the
#    same layers as the one the smoke test passed.
# 4. Pushes IMAGE_REPO:VERSION-amd64 and IMAGE_REPO:VERSION-arm64 and joins them into
#    one multi-arch manifest tagged VERSION, MAJOR.MINOR and latest (a pre-release
#    gets VERSION only).
# 5. extension/popcornstash-extension.zip, for the GitHub release.
# 6. Annotated tag vVERSION, pushed to REMOTE.
# 7. Prints the release notes from CHANGELOG.md and what to attach to the release.
#
# Registry login: an existing `docker login ghcr.io`, or GHCR_USER and GHCR_TOKEN in
# the environment. The token needs the write:packages scope.
#
# Environment:
#   IMAGE_REPO         where images go (default ghcr.io/thekozugroup/popcornstash)
#   REMOTE             git remote to compare with and push the tag to (default origin)
#   GHCR_USER, GHCR_TOKEN  registry login, when Docker is not logged in already
#   IMAGE              image scripts/check.sh builds and smoke-tests (default popcornstash:dev)
#   REGISTRY, DOCKER_HUB_MIRROR=1, BUILD_CA  base images and proxy CA, as in scripts/docker-smoke.sh
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  sed -n '2,/^set -Eeuo/{/^set -Eeuo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

dry_run=0
bump=0
version=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --bump) bump=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      printf 'release.sh: unknown option %s (see --help)\n' "$arg" >&2
      exit 2
      ;;
    *)
      if [[ -n "$version" ]]; then
        printf 'release.sh: one version only (see --help)\n' >&2
        exit 2
      fi
      version="${arg#v}"
      ;;
  esac
done
if [[ -z "$version" ]]; then
  printf 'release.sh: which version? (see --help)\n' >&2
  exit 2
fi
if ((bump && dry_run)); then
  printf 'release.sh: --bump only edits files; run --dry-run after committing them\n' >&2
  exit 2
fi
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
if ! [[ "$version" =~ $semver ]]; then
  printf 'release.sh: %s is not a version like 1.2.3 or 1.2.3-rc.1\n' "$version" >&2
  exit 2
fi
major="${BASH_REMATCH[1]}"
minor="${BASH_REMATCH[2]}"
prerelease="${BASH_REMATCH[4]}"

IMAGE_REPO="${IMAGE_REPO:-ghcr.io/thekozugroup/popcornstash}"
REGISTRY_HOST="${IMAGE_REPO%%/*}"
REMOTE="${REMOTE:-origin}"
CHECK_IMAGE="${IMAGE:-popcornstash:dev}"
TAG="v$version"
ARCHES=(amd64 arm64)
if [[ -z "${REGISTRY:-}" ]]; then
  if [[ "${DOCKER_HUB_MIRROR:-0}" == "1" ]]; then
    REGISTRY="mirror.gcr.io/library"
  else
    REGISTRY="docker.io/library"
  fi
fi
NOTES_FILE="target/release-notes/$TAG.md"
TAG_MESSAGE="target/release-notes/$TAG.tag-message"
ZIP="extension/popcornstash-extension.zip"

# ------------------------------------------------------------- reporting ----
problems=()

say() {
  printf '\n==> %s\n' "$1"
}

ok() {
  printf 'ok    %s\n' "$1"
}

# A problem stops a release before anything is built or pushed. A dry run lists it
# and carries on, so one run shows everything that needs fixing.
problem() {
  problems+=("$1")
  printf 'FAIL  %s\n' "$1"
}

die() {
  printf '\nFAIL  %s\n' "$1" >&2
  exit 1
}

# run command...: runs it, or in a dry run prints it.
run() {
  if ((dry_run)); then
    printf 'would run:'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

need() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || die "this script needs '$tool' on the PATH"
  done
}

# ------------------------------------------------------------- versions ----
# The version in Cargo.toml's [workspace.package] section.
cargo_version() {
  awk '
    /^\[/ { section = $0 }
    section == "[workspace.package]" && /^version *= *"/ {
      sub(/^version *= *"/, ""); sub(/".*$/, ""); print; exit
    }' Cargo.toml
}

# "file: version" for every version number that must equal the release version.
version_list() {
  local name meta lock
  printf 'Cargo.toml [workspace.package]: %s\n' "$(cargo_version)"
  meta="$(cargo metadata --no-deps --format-version 1)"
  lock="$(<Cargo.lock)"
  for name in $(jq -r '.packages[].name' <<<"$meta"); do
    printf 'Cargo.lock %s: %s\n' "$name" "$(awk -v n="$name" '
      $0 == "name = \"" n "\"" { found = 1; next }
      found && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' <<<"$lock")"
  done
  for dir in web extension; do
    printf '%s/package.json: %s\n' "$dir" "$(jq -r .version "$dir/package.json")"
    printf '%s/package-lock.json: %s\n' "$dir" "$(jq -r '.version' "$dir/package-lock.json")"
    printf '%s/package-lock.json (root package): %s\n' "$dir" "$(jq -r '.packages[""].version' "$dir/package-lock.json")"
  done
  printf 'extension/static/manifest.json: %s\n' "$(jq -r .version extension/static/manifest.json)"
}

# The CHANGELOG.md section for the version ("## [1.2.3] - date" or "## 1.2.3"),
# without its heading and leading blank lines.
changelog_section() {
  [[ -f CHANGELOG.md ]] || return 0
  awk -v v="$version" '
    function is_heading(line, rest) {
      if (substr(line, 1, 3) != "## ") return 0
      rest = substr(line, 4)
      if (substr(rest, 1, 1) == "[") rest = substr(rest, 2)
      if (substr(rest, 1, length(v)) != v) return 0
      rest = substr(rest, length(v) + 1)
      return rest == "" || rest ~ /^[] ]/
    }
    on && /^## / { exit }
    is_heading($0) { on = 1; next }
    on { print }' CHANGELOG.md | sed '/./,$!d'
}

bump_versions() {
  local tmp
  say "setting every version number to $version"
  tmp="$(mktemp)"
  awk -v v="$version" '
    /^\[/ { section = $0 }
    section == "[workspace.package]" && !done && /^version *= *"/ { $0 = "version = \"" v "\""; done = 1 }
    { print }' Cargo.toml >"$tmp"
  cat "$tmp" >Cargo.toml
  # Rewrites only the workspace's own entries in Cargo.lock.
  cargo update --workspace
  for dir in web extension; do
    npm --prefix "$dir" version "$version" --no-git-tag-version --allow-same-version --ignore-scripts >/dev/null
  done
  sed -E 's/^(  "version": )"[^"]*"/\1"'"$version"'"/' extension/static/manifest.json >"$tmp"
  cat "$tmp" >extension/static/manifest.json
  rm -f "$tmp"
  version_list
  printf '\nChanged files:\n'
  git status --short
  if [[ -z "$(changelog_section)" ]]; then
    printf '\nCHANGELOG.md has no "## [%s]" section yet: write one.\n' "$version"
  fi
  printf '\nNext: review and commit these changes, push main, then run\n  scripts/release.sh --dry-run %s\n  scripts/release.sh %s\n' "$version" "$version"
}

if ((bump)); then
  need cargo npm jq awk sed git
  bump_versions
  exit 0
fi

# ----------------------------------------------------------------- login ----
login_help="Pushing to $REGISTRY_HOST needs a GitHub personal access token (classic) with the write:packages scope. Run \`docker login $REGISTRY_HOST -u <github-user>\` and paste the token as the password, or set GHCR_USER and GHCR_TOKEN, then run this again."

# True when Docker's client configuration holds credentials for the registry.
docker_has_login() {
  local config="${DOCKER_CONFIG:-$HOME/.docker}/config.json" store listed
  [[ -f "$config" ]] || return 1
  if jq -e --arg h "$REGISTRY_HOST" '((.auths // {}) | has($h) or has("https://" + $h)) or ((.credHelpers // {}) | has($h))' \
    "$config" >/dev/null 2>&1; then
    return 0
  fi
  store="$(jq -r '.credsStore // empty' "$config" 2>/dev/null)"
  [[ -n "$store" ]] && command -v "docker-credential-$store" >/dev/null 2>&1 || return 1
  listed="$("docker-credential-$store" list 2>/dev/null)" || return 1
  jq -e --arg h "$REGISTRY_HOST" 'has($h) or has("https://" + $h)' <<<"$listed" >/dev/null 2>&1
}

registry_login() {
  if [[ -n "${GHCR_TOKEN:-}" ]]; then
    if [[ -z "${GHCR_USER:-}" ]]; then
      problem "GHCR_TOKEN is set but GHCR_USER is not: set it to the GitHub user the token belongs to"
      return
    fi
    if ((dry_run)); then
      ok "would log in to $REGISTRY_HOST as $GHCR_USER with GHCR_TOKEN"
    elif printf '%s' "$GHCR_TOKEN" | docker login "$REGISTRY_HOST" -u "$GHCR_USER" --password-stdin >/dev/null; then
      ok "logged in to $REGISTRY_HOST as $GHCR_USER"
    else
      problem "docker login $REGISTRY_HOST failed. $login_help"
    fi
  elif docker_has_login; then
    ok "Docker already has a login for $REGISTRY_HOST"
  else
    problem "Docker has no login for $REGISTRY_HOST and GHCR_TOKEN is not set. $login_help"
  fi
}

# ------------------------------------------------------------- preflight ----
need git docker jq awk sed cargo npm node sha256sum
docker buildx version >/dev/null 2>&1 || die "this script needs docker buildx (for docker buildx imagetools)"
docker info >/dev/null 2>&1 || die "Docker is not answering (is the daemon running?)"

dry_note=""
if ((dry_run)); then
  dry_note=" (dry run: nothing is pushed or tagged)"
fi
say "preflight for $TAG$dry_note"

branch="$(git symbolic-ref --quiet --short HEAD || printf 'a detached HEAD')"
if [[ "$branch" == main ]]; then
  ok "on main"
else
  problem "on $branch, not main"
fi

dirty="$(git status --porcelain --untracked-files=normal)"
if [[ -z "$dirty" ]]; then
  ok "no uncommitted or untracked files"
else
  problem "uncommitted or untracked files (the image is built from the working tree): $(wc -l <<<"$dirty" | tr -d ' ') path(s), see git status"
fi

if git fetch --quiet "$REMOTE" main; then
  head="$(git rev-parse HEAD)"
  upstream="$(git rev-parse "$REMOTE/main")"
  if [[ "$head" == "$upstream" ]]; then
    ok "HEAD is $REMOTE/main (${head:0:12})"
  else
    counts="$(git rev-list --left-right --count "HEAD...$REMOTE/main")"
    problem "HEAD and $REMOTE/main differ ($(awk '{ print $1 " commit(s) only here, " $2 " only on the remote" }' <<<"$counts")): push or pull first"
  fi
else
  problem "could not fetch $REMOTE main"
fi

if git rev-parse --quiet --verify "refs/tags/$TAG" >/dev/null; then
  problem "tag $TAG already exists here"
else
  if ! remote_tag="$(git ls-remote --tags "$REMOTE" "refs/tags/$TAG")"; then
    problem "could not ask $REMOTE whether tag $TAG exists"
  elif [[ -n "$remote_tag" ]]; then
    problem "tag $TAG already exists on $REMOTE"
  else
    ok "tag $TAG is new"
  fi
fi

if [[ ! -f CHANGELOG.md ]]; then
  problem "CHANGELOG.md is missing"
elif [[ -z "$(changelog_section | tr -d '[:space:]')" ]]; then
  problem "CHANGELOG.md has no \"## [$version]\" section (or it is empty)"
else
  ok "CHANGELOG.md has a section for $version"
fi

versions="$(version_list)"
wrong="$(awk -v v="$version" -F': ' '$2 != v' <<<"$versions")"
if [[ -z "$wrong" ]]; then
  ok "all $(wc -l <<<"$versions" | tr -d ' ') version numbers are $version"
else
  problem "version numbers that are not $version (scripts/release.sh --bump $version sets them):"
  printf '        %s\n' "${wrong//$'\n'/$'\n'        }"
fi

# Each architecture other than Docker's own needs emulation for the runtime stage.
# A one-line build proves it (the platform list of `docker buildx inspect` can still
# show an emulator that was turned off); nothing is cached or kept.
native="$(docker version -f '{{.Server.Arch}}')"
can_run() {
  printf 'FROM %s/debian:trixie-slim\nRUN true\n' "$REGISTRY" |
    DOCKER_BUILDKIT=1 docker build --quiet --no-cache --platform "linux/$1" --output type=cacheonly - >/dev/null 2>&1
}
for arch in "${ARCHES[@]}"; do
  [[ "$arch" == "$native" ]] && continue
  if can_run "$arch"; then
    ok "linux/$arch images can be built here (emulation for the runtime stage is set up)"
  else
    qemu=aarch64
    [[ "$arch" == amd64 ]] && qemu=x86_64
    problem "Docker cannot run linux/$arch programs. The Rust code is cross-compiled, but the image's last stage runs apt-get for $arch, which needs QEMU emulation. Turn it on:
        sudo apt-get install qemu-user-static binfmt-support
        # only if /proc/sys/fs/binfmt_misc/qemu-$qemu does not exist now (no systemd; repeat after a reboot):
        sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc   # if it is not mounted yet
        sudo sh -c 'cat /usr/lib/binfmt.d/qemu-$qemu.conf > /proc/sys/fs/binfmt_misc/register'
      or: docker run --privileged --rm tonistiigi/binfmt --install $arch
      docs/deployment.md, \"Building the image yourself\", has the details."
  fi
done

registry_login

# A version that is already published is never overwritten.
if lookup="$(docker buildx imagetools inspect "$IMAGE_REPO:$version" 2>&1)"; then
  problem "$IMAGE_REPO:$version is already published"
elif [[ "$lookup" == *"not found"* ]]; then
  ok "$IMAGE_REPO:$version is not published yet"
else
  printf 'note  could not look up %s, so it is taken as not published: %s\n' "$IMAGE_REPO:$version" "$(sed -n 1p <<<"$lookup")"
fi

if ((${#problems[@]})); then
  if ((dry_run)); then
    printf '\nA release would stop here (%d problem(s) above). The dry run carries on.\n' "${#problems[@]}"
  else
    die "preflight found ${#problems[@]} problem(s); nothing was built or pushed"
  fi
fi

# ---------------------------------------------------------------- checks ----
say "scripts/check.sh (image $CHECK_IMAGE)"
IMAGE="$CHECK_IMAGE" scripts/check.sh || die "scripts/check.sh failed; nothing was pushed"

# ---------------------------------------------------------------- images ----
revision="$(git rev-parse HEAD)"
created="$(TZ=UTC0 git show -s --date=format-local:%Y-%m-%dT%H:%M:%SZ --format=%cd HEAD)"
build_args=(
  --build-arg "REGISTRY=$REGISTRY"
  --label "org.opencontainers.image.version=$version"
  --label "org.opencontainers.image.revision=$revision"
  --label "org.opencontainers.image.created=$created"
)
if [[ -n "${BUILD_CA:-}" ]]; then
  build_args+=(--secret "id=build_ca,src=$BUILD_CA")
fi

local_tag() {
  printf 'popcornstash:release-%s-%s' "$version" "$1"
}

layers() {
  docker image inspect -f '{{json .RootFS.Layers}}' "$1"
}

for arch in "${ARCHES[@]}"; do
  image="$(local_tag "$arch")"
  say "image for linux/$arch ($image)"
  DOCKER_BUILDKIT=1 docker build --platform "linux/$arch" "${build_args[@]}" -t "$image" . ||
    die "the linux/$arch image did not build"
  platform="$(docker image inspect -f '{{.Os}}/{{.Architecture}}' "$image")"
  [[ "$platform" == "linux/$arch" ]] || die "$image is $platform, not linux/$arch"
  reported="$(docker run --rm --platform "linux/$arch" "$image" --version)" ||
    die "popcornstash --version does not run in the linux/$arch image"
  [[ "$reported" == "popcornstash $version" ]] || die "the linux/$arch image reports '$reported', not popcornstash $version"
  wg="$(docker run --rm --platform "linux/$arch" --entrypoint boringtun-cli "$image" --version)" ||
    die "boringtun-cli does not run in the linux/$arch image"
  ok "linux/$arch: $reported, $wg"
  if [[ "$arch" == "$native" ]]; then
    if [[ "$(layers "$image")" == "$(layers "$CHECK_IMAGE")" ]]; then
      ok "linux/$arch has the same layers as $CHECK_IMAGE, which passed the smoke test"
    else
      die "linux/$arch ($image) has different layers from $CHECK_IMAGE, which passed the smoke test; run this again"
    fi
  fi
done

# ------------------------------------------------------------------ push ----
remote_tags=("$IMAGE_REPO:$version")
if [[ -z "$prerelease" ]]; then
  remote_tags+=("$IMAGE_REPO:$major.$minor" "$IMAGE_REPO:latest")
fi
tag_args=()
for t in "${remote_tags[@]}"; do
  tag_args+=(-t "$t")
done
sources=()

say "push to $REGISTRY_HOST$dry_note"
for arch in "${ARCHES[@]}"; do
  image="$(local_tag "$arch")"
  target="$IMAGE_REPO:$version-$arch"
  sources+=("$target")
  printf '%s (%s, %s) -> %s\n' "$image" "linux/$arch" "$(docker image inspect -f '{{.Id}}' "$image")" "$target"
  run docker tag "$image" "$target"
  run docker push "$target" || die "docker push $target failed. $login_help"
done
printf 'multi-arch manifest (linux/amd64, linux/arm64) -> %s\n' "${remote_tags[*]}"
run docker buildx imagetools create "${tag_args[@]}" "${sources[@]}" ||
  die "docker buildx imagetools create failed. $login_help"
run docker buildx imagetools inspect "$IMAGE_REPO:$version"

# ------------------------------------------------------------- extension ----
say "Chrome extension"
npm --prefix extension run zip || die "npm run zip failed"
zip_version="$(jq -r .version extension/dist/manifest.json)"
[[ "$zip_version" == "$version" ]] || die "the extension zip says version $zip_version, not $version"
zip_sum="$(sha256sum "$ZIP")"
ok "$ZIP (extension $zip_version, sha256 ${zip_sum%% *})"

# ------------------------------------------------------------------- tag ----
say "git tag $TAG$dry_note"
mkdir -p "$(dirname "$NOTES_FILE")"
changelog_section >"$NOTES_FILE"
{
  printf 'PopcornStash %s\n\n' "$version"
  cat "$NOTES_FILE"
} >"$TAG_MESSAGE"
if ((!dry_run)); then
  dirty="$(git status --porcelain --untracked-files=normal)"
  [[ -z "$dirty" ]] || die "the checks changed tracked files (see git status); the images are pushed but $TAG is not tagged"
fi
run git tag -a "$TAG" -F "$TAG_MESSAGE" "$revision"
run git push "$REMOTE" "refs/tags/$TAG"

# ---------------------------------------------------------------- report ----
say "GitHub release"
prerelease_flag=""
title_note=""
if [[ -n "$prerelease" ]]; then
  prerelease_flag=" --prerelease"
  title_note=" (pre-release)"
fi
cat <<EOF
Create the release for $TAG at https://github.com/thekozugroup/PopcornStash/releases/new?tag=$TAG
  title:   PopcornStash $version$title_note
  attach:  $ZIP (sha256 ${zip_sum%% *})
  notes:   below, also saved in $NOTES_FILE
or with the GitHub CLI:
  gh release create $TAG $ZIP --title "PopcornStash $version" --notes-file $NOTES_FILE$prerelease_flag

Image: ${remote_tags[*]} (linux/amd64, linux/arm64)

----------------------------------------------------------- release notes
$(<"$NOTES_FILE")
-------------------------------------------------------------------------
EOF

if ((dry_run)); then
  if ((${#problems[@]})); then
    printf '\nDry run finished. A release would have stopped at the preflight:\n'
    printf '  - %s\n' "${problems[@]%%$'\n'*}"
    exit 1
  fi
  printf '\nDry run finished: nothing was pushed or tagged. Run scripts/release.sh %s to release.\n' "$version"
else
  printf '\nReleased %s.\n' "$version"
fi

#!/usr/bin/env bash
# Release PopcornStash from this machine. There is no CI service: this script is the
# whole release. README.md ("Releasing") has the one-time setup.
#
#   scripts/release.sh --bump 0.2.0      set every version number to 0.2.0 and stop
#   scripts/release.sh --dry-run 0.2.0   everything except pushing, tagging and the GitHub release
#   scripts/release.sh 0.2.0             release 0.2.0
#
# 1. Preflight, before anything is built. VERSION is MAJOR.MINOR.PATCH, optionally
#    with a -pre-release. The branch is main, has no uncommitted or untracked files,
#    and equals REMOTE/main. Tag vVERSION is new here and on REMOTE. CHANGELOG.md has
#    a "## [VERSION] - YYYY-MM-DD" section dated today. Every version number agrees
#    (--bump sets them). Docker can run the other architecture (QEMU). GHCR_TOKEN is a
#    classic token with write:packages that ghcr.io accepts. The image package is
#    public, or does not exist yet, and has no VERSION tag. gh is logged in, can push
#    to the repository, and it has no vVERSION release.
# 2. scripts/check.sh --quick --e2e --audit on the working tree: fmt, clippy, the Rust
#    tests, the web and extension checks, the Playwright suite, cargo audit, npm audit.
# 3. linux/amd64 and linux/arm64 images, built from a copy of the commit, not the
#    working tree. The Rust code is cross-compiled; only the runtime stage's apt-get
#    runs under emulation. Both must report popcornstash VERSION.
# 4. scripts/docker-smoke.sh, scripts/vpn-e2e.sh and scripts/fullstack-e2e.sh on the
#    image for this machine's architecture: the image that is pushed.
# 5. The Chrome extension zip, from a clean npm ci in the copy of the commit.
# 6. Publish, in this order: push VERSION-amd64 and VERSION-arm64; join them into one
#    multi-arch tag VERSION, plus MAJOR.MINOR and latest when VERSION is the newest
#    release of its line and overall (a pre-release gets VERSION only); push the
#    annotated tag vVERSION; create the GitHub release with the CHANGELOG.md section as
#    notes and the zip attached; check that anyone can pull the image.
#
# A dry run reports every preflight problem and carries on, so one run shows all that
# needs fixing. It checks the token (logging in to a throwaway Docker configuration)
# and builds the working tree as it is when it has uncommitted changes. If a publish
# step fails, the script says what is published and prints the commands that finish
# the release.
#
# Environment:
#   GHCR_TOKEN         GitHub personal access token (classic) with write:packages. It
#                      logs in to a temporary Docker configuration that is deleted at
#                      the end, and is not passed on to the checks and builds.
#   GHCR_USER          the token's GitHub user (default: read from the token)
#   GH_TOKEN           token for gh, if gh is not logged in; also kept from the checks
#   IMAGE_REPO         where images go (default ghcr.io/thekozugroup/popcornstash)
#   GITHUB_REPO        where the release goes (default thekozugroup/PopcornStash)
#   REMOTE             git remote to compare with and push the tag to (default origin)
#   REGISTRY, DOCKER_HUB_MIRROR=1, BUILD_CA  base images and proxy CA, as in scripts/docker-smoke.sh
#   E2E_PORT, E2E_CHROMIUM, STASH_PORT, SUBNET_PREFIX  passed on to the tests (tests/*/README.md)
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  sed -n '2,/^set -Eeuo/{/^set -Eeuo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

# Tokens are kept in shell variables and taken out of the environment, so the checks,
# npm, cargo build scripts and Docker builds never see them.
ghcr_token="${GHCR_TOKEN:-}"
ghcr_user="${GHCR_USER:-}"
gh_token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
unset GHCR_TOKEN GH_TOKEN GITHUB_TOKEN

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
patch="${BASH_REMATCH[3]}"
prerelease="${BASH_REMATCH[4]}"
# The Chrome extension's version is the numbers alone, each at most 65535.
core="$major.$minor.$patch"
for part in "$major" "$minor" "$patch"; do
  if ((${#part} > 5 || 10#$part > 65535)); then
    printf 'release.sh: %s has a number above 65535, which Chrome does not accept as an extension version\n' "$version" >&2
    exit 2
  fi
done

IMAGE_REPO="${IMAGE_REPO:-ghcr.io/thekozugroup/popcornstash}"
REGISTRY_HOST="${IMAGE_REPO%%/*}"
GITHUB_REPO="${GITHUB_REPO:-thekozugroup/PopcornStash}"
REMOTE="${REMOTE:-origin}"
TAG="v$version"
ARCHES=(amd64 arm64)
if [[ -z "${REGISTRY:-}" ]]; then
  if [[ "${DOCKER_HUB_MIRROR:-0}" == "1" ]]; then
    REGISTRY="mirror.gcr.io/library"
  else
    REGISTRY="docker.io/library"
  fi
fi
export REGISTRY
OUT_DIR="target/releases/$TAG"
NOTES_FILE="$OUT_DIR/notes.md"
TAG_MESSAGE="$OUT_DIR/tag-message.txt"
ZIP="$OUT_DIR/popcornstash-extension.zip"
# Directories with a package.json whose version follows the release.
NPM_DIRS=(web extension tests/e2e tests/fullstack)

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

# A problem stops a release before anything is built or pushed. A dry run lists it
# and carries on.
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

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# gh with GH_TOKEN put back for it alone.
gh() {
  if [[ -n "$gh_token" ]]; then
    GH_TOKEN="$gh_token" command gh "$@"
  else
    command gh "$@"
  fi
}

# version_gt A B: true when MAJOR.MINOR.PATCH A is higher than B.
version_gt() {
  local -a a b
  local i
  IFS=. read -r -a a <<<"$1"
  IFS=. read -r -a b <<<"$2"
  for i in 0 1 2; do
    ((10#${a[i]} > 10#${b[i]})) && return 0
    ((10#${a[i]} < 10#${b[i]})) && return 1
  done
  return 1
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

# "kind<TAB>where<TAB>version" for every version number the release keeps in step.
# Kind "full" must be VERSION; "core" must be VERSION without its pre-release, because
# Chrome accepts only numbers in an extension's version (extension/scripts/build.mjs
# puts the full version in version_name).
version_list() {
  local name meta lock dir
  printf 'full\tCargo.toml [workspace.package]\t%s\n' "$(cargo_version)"
  meta="$(cargo metadata --no-deps --format-version 1)"
  lock="$(<Cargo.lock)"
  for name in $(jq -r '.packages[].name' <<<"$meta"); do
    printf 'full\tCargo.lock %s\t%s\n' "$name" "$(awk -v n="$name" '
      $0 == "name = \"" n "\"" { found = 1; next }
      found && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' <<<"$lock")"
  done
  for dir in "${NPM_DIRS[@]}"; do
    printf 'full\t%s/package.json\t%s\n' "$dir" "$(jq -r .version "$dir/package.json")"
    printf 'full\t%s/package-lock.json\t%s\n' "$dir" "$(jq -r '.version' "$dir/package-lock.json")"
    printf 'full\t%s/package-lock.json (root package)\t%s\n' "$dir" "$(jq -r '.packages[""].version' "$dir/package-lock.json")"
  done
  printf 'core\textension/static/manifest.json\t%s\n' "$(jq -r .version extension/static/manifest.json)"
}

# The CHANGELOG.md heading for the version ("## [1.2.3] - 2026-01-31" or "## 1.2.3").
changelog_heading() {
  [[ -f "$1" ]] || return 0
  awk -v v="$version" '
    substr($0, 1, 3) == "## " {
      rest = substr($0, 4)
      if (substr(rest, 1, 1) == "[") rest = substr(rest, 2)
      if (substr(rest, 1, length(v)) != v) next
      rest = substr(rest, length(v) + 1)
      if (rest == "" || rest ~ /^[] ]/) { print; exit }
    }' "$1"
}

# The section under that heading, without the heading and leading blank lines.
changelog_section() {
  local heading
  heading="$(changelog_heading "$1")"
  [[ -n "$heading" ]] || return 0
  awk -v h="$heading" '
    on && /^## / { exit }
    $0 == h { on = 1; next }
    on { print }' "$1" | sed '/./,$!d'
}

bump_versions() {
  local tmp dir
  say "setting every version number to $version"
  tmp="$(mktemp)"
  awk -v v="$version" '
    /^\[/ { section = $0 }
    section == "[workspace.package]" && !done && /^version *= *"/ { $0 = "version = \"" v "\""; done = 1 }
    { print }' Cargo.toml >"$tmp"
  cat "$tmp" >Cargo.toml
  # Rewrites only the workspace's own entries in Cargo.lock.
  cargo update --workspace
  for dir in "${NPM_DIRS[@]}"; do
    npm --prefix "$dir" version "$version" --no-git-tag-version --allow-same-version --ignore-scripts >/dev/null
  done
  sed -E 's/^(  "version": )"[^"]*"/\1"'"$core"'"/' extension/static/manifest.json >"$tmp"
  cat "$tmp" >extension/static/manifest.json
  rm -f "$tmp"
  version_list | awk -F'\t' '{ print $2 ": " $3 }'
  printf '\nChanged files:\n'
  git status --short
  if [[ -z "$(changelog_heading CHANGELOG.md)" ]]; then
    printf '\nCHANGELOG.md has no section for %s yet. Write one under this heading, dated the release day:\n  ## [%s] - %s\n' \
      "$version" "$version" "$(date +%F)"
  fi
  printf '\nNext: review and commit these changes, push main, then run\n  scripts/release.sh --dry-run %s\n  scripts/release.sh %s\n' "$version" "$version"
}

if ((bump)); then
  need cargo npm jq awk sed git
  bump_versions
  exit 0
fi

# ------------------------------------------------------- state and cleanup ----
need git docker jq awk sed cargo npm node curl tar
docker buildx version >/dev/null 2>&1 || die "this script needs docker buildx (for docker buildx imagetools)"
docker info >/dev/null 2>&1 || die "Docker is not answering (is the daemon running?)"
native="$(docker version -f '{{.Server.Arch}}')"
[[ " ${ARCHES[*]} " == *" $native "* ]] || die "Docker runs $native here; releases are made on amd64 or arm64 machines"

user_docker_config="${DOCKER_CONFIG:-$HOME/.docker}"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/popcornstash-release.XXXXXX")"
push_config=""     # Docker configuration logged in with GHCR_TOKEN
local_images=()    # images this run made, removed when it succeeds
stage=""           # the publish step running now, for the message if it fails
tag_created=0
joined=0
tag_pushed=0
gh_release_cmd=()

finish_commands() {
  if ((!tag_pushed)); then
    printf '  git push %q %q\n' "$REMOTE" "refs/tags/$TAG"
  fi
  printf '  '
  printf '%q ' "${gh_release_cmd[@]}"
  printf '\n'
}

cleanup() {
  local status=$?
  set +e
  if ((status != 0)); then
    case "$stage" in
      images)
        printf '\n%s is not published (the multi-arch tag was not made). Fix the cause and run scripts/release.sh %s again.\n' \
          "$IMAGE_REPO:$version" "$version" >&2
        ;;
      tag | release)
        printf '\n%s is published, but the release is not finished. From this folder, finish it with:\n' "$IMAGE_REPO:$version" >&2
        finish_commands >&2
        ;;
    esac
    if ((${#local_images[@]})); then
      printf '\nThe images this run built are kept for a look: %s\n(docker image rm %s)\n' "${local_images[*]}" "${local_images[*]}" >&2
    fi
  fi
  # A local tag is only kept when the images it describes are published.
  if ((tag_created && !joined)); then
    git tag -d "$TAG" >/dev/null 2>&1
  fi
  rm -rf "$tmp_root"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# A Docker client configuration with no credentials that shares the user's contexts,
# plugins and buildx state, so a temporary login never touches the user's own.
docker_config() {
  local dir="$1" sub
  mkdir -p "$dir"
  chmod 700 "$dir"
  if [[ -f "$user_docker_config/config.json" ]]; then
    jq 'del(.auths, .credsStore, .credHelpers)' "$user_docker_config/config.json" >"$dir/config.json"
  else
    printf '{}\n' >"$dir/config.json"
  fi
  for sub in contexts cli-plugins buildx; do
    if [[ -e "$user_docker_config/$sub" ]]; then
      ln -s "$user_docker_config/$sub" "$dir/$sub"
    fi
  done
}

# github_api PATH: GET https://api.github.com/PATH with GHCR_TOKEN. Sets api_status
# (000 when the request failed), api_body and api_headers. The token goes to curl on
# stdin, never on its command line.
github_api() {
  local out
  : >"$tmp_root/api-headers"
  if out="$(printf 'Authorization: Bearer %s\n' "$ghcr_token" |
    curl -sS -m 30 -H @- -H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28' \
      -D "$tmp_root/api-headers" -w '\n%{http_code}' "https://api.github.com/$1" 2>&1)"; then
    api_status="${out##*$'\n'}"
    api_body="${out%$'\n'*}"
  else
    api_status=000
    api_body="$out"
  fi
  api_headers="$(tr -d '\r' <"$tmp_root/api-headers")"
}

# --------------------------------------------------------------- preflight ----
dry_note=""
if ((dry_run)); then
  dry_note=" (dry run: nothing is pushed, tagged or released)"
fi
say "preflight for $TAG$dry_note"

branch="$(git symbolic-ref --quiet --short HEAD || printf 'a detached HEAD')"
if [[ "$branch" == main ]]; then
  ok "on main"
else
  problem "on $branch, not main"
fi

revision="$(git rev-parse HEAD)"
start_status="$(git status --porcelain --untracked-files=normal)"
if [[ -z "$start_status" ]]; then
  ok "no uncommitted or untracked files"
else
  problem "uncommitted or untracked files: $(wc -l <<<"$start_status" | tr -d ' ') path(s), see git status"
fi

if git fetch --quiet "$REMOTE" main; then
  upstream="$(git rev-parse "$REMOTE/main")"
  if [[ "$revision" == "$upstream" ]]; then
    ok "HEAD is $REMOTE/main (${revision:0:12})"
  else
    counts="$(git rev-list --left-right --count "HEAD...$REMOTE/main")"
    problem "HEAD and $REMOTE/main differ ($(awk '{ print $1 " commit(s) only here, " $2 " only on the remote" }' <<<"$counts")): push or pull first"
  fi
else
  problem "could not fetch $REMOTE main"
fi

# Released versions on the remote decide whether latest and MAJOR.MINOR move.
released=()
remote_tags_known=0
tag_taken=0
if git rev-parse --quiet --verify "refs/tags/$TAG" >/dev/null; then
  problem "tag $TAG already exists here"
  tag_taken=1
fi
if remote_refs="$(git ls-remote --tags --refs "$REMOTE" 'refs/tags/v*')"; then
  remote_tags_known=1
  while read -r _ ref; do
    [[ -n "${ref:-}" ]] || continue
    name="${ref#refs/tags/v}"
    if [[ "$name" == "$version" ]]; then
      problem "tag $TAG already exists on $REMOTE"
      tag_taken=1
    elif [[ "$name" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      released+=("$name")
    fi
  done <<<"$remote_refs"
  ((tag_taken)) || ok "tag $TAG is new (${#released[@]} earlier release(s) on $REMOTE)"
else
  problem "could not list the tags on $REMOTE"
fi

move_latest=0
move_minor=0
if [[ -z "$prerelease" ]]; then
  move_latest=1
  move_minor=1
  for name in "${released[@]}"; do
    if version_gt "$name" "$version"; then
      move_latest=0
      [[ "${name%.*}" == "$major.$minor" ]] && move_minor=0
    fi
  done
fi
image_tags=("$IMAGE_REPO:$version")
if ((move_minor)); then
  image_tags+=("$IMAGE_REPO:$major.$minor")
fi
if ((move_latest)); then
  image_tags+=("$IMAGE_REPO:latest")
fi
if [[ -n "$prerelease" ]]; then
  ok "a pre-release is tagged $version only; $major.$minor and latest stay where they are"
elif ((remote_tags_known)); then
  ok "image tags: ${image_tags[*]#"$IMAGE_REPO":}$( ((move_latest)) || printf ' (a newer release keeps latest)')"
fi

heading="$(changelog_heading CHANGELOG.md)"
today="$(date +%F)"
if [[ ! -f CHANGELOG.md ]]; then
  problem "CHANGELOG.md is missing"
elif [[ -z "$heading" || -z "$(changelog_section CHANGELOG.md | tr -d '[:space:]')" ]]; then
  problem "CHANGELOG.md has no \"## [$version] - $today\" section (or it is empty)"
elif [[ "$heading" != *"$today"* ]]; then
  problem "CHANGELOG.md's heading \"$heading\" should carry today's date, $today"
else
  ok "CHANGELOG.md has a section for $version dated $today"
fi

versions="$(version_list)"
wrong="$(awk -F'\t' -v full="$version" -v core="$core" '
  { want = ($1 == "core" ? core : full) }
  $3 != want { print $2 ": " $3 " (should be " want ")" }' <<<"$versions")"
if [[ -z "$wrong" ]]; then
  ok "all $(wc -l <<<"$versions" | tr -d ' ') version numbers say $version"
else
  problem "version numbers that do not say $version (scripts/release.sh --bump $version sets them):"
  printf '        %s\n' "${wrong//$'\n'/$'\n'        }"
fi

# The runtime stage runs apt-get for the other architecture, which needs emulation.
# A one-line build proves it: the platform list of `docker buildx inspect` can still
# show an emulator that was turned off. It pulls that architecture's base image (which
# the release build needs anyway) and leaves a small build-cache entry.
for arch in "${ARCHES[@]}"; do
  [[ "$arch" == "$native" ]] && continue
  if probe="$(printf 'FROM %s/debian:trixie-slim\nRUN true\n' "$REGISTRY" |
    DOCKER_BUILDKIT=1 docker build --no-cache --progress=plain --platform "linux/$arch" --output type=cacheonly - 2>&1)"; then
    ok "linux/$arch programs run here (QEMU emulation for the runtime stage)"
  elif grep -q -E 'exec format error|exec /bin/sh: no such file or directory' <<<"$probe"; then
    qemu=aarch64
    [[ "$arch" == amd64 ]] && qemu=x86_64
    problem "Docker cannot run linux/$arch programs. The Rust code is cross-compiled, but the image's last stage runs apt-get for $arch, which needs QEMU emulation. Turn it on:
        sudo apt-get install qemu-user-static binfmt-support
        # only if /proc/sys/fs/binfmt_misc/qemu-$qemu does not exist now (no systemd; repeat after a reboot):
        sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc   # if it is not mounted yet
        sudo sh -c 'cat /usr/lib/binfmt.d/qemu-$qemu.conf > /proc/sys/fs/binfmt_misc/register'
        # if it exists but its first line says disabled:
        echo 1 | sudo tee /proc/sys/fs/binfmt_misc/qemu-$qemu
      docs/deployment.md, \"Building the image yourself\", has the details."
  else
    problem "could not test linux/$arch emulation: the build of $REGISTRY/debian:trixie-slim failed for another reason (set REGISTRY, or DOCKER_HUB_MIRROR=1, if Docker Hub is out of reach). Its last lines:
$(grep -v '^[[:space:]]*$' <<<"$probe" | tail -n 4 | sed 's/^/        /')"
  fi
done

# The registry token. Only ghcr.io tokens can be checked with the GitHub API.
if [[ -z "$ghcr_token" ]]; then
  problem "GHCR_TOKEN is not set. Pushing to $REGISTRY_HOST needs a GitHub personal access token (classic) with the write:packages scope; fine-grained tokens do not work with ghcr.io. Set it for this command only, for example: read -rs GHCR_TOKEN && GHCR_TOKEN=\$GHCR_TOKEN scripts/release.sh $version"
else
  token_ok=1
  if [[ "$REGISTRY_HOST" == ghcr.io ]]; then
    github_api user
    scopes="$(sed -n 's/^[Xx]-[Oo][Aa]uth-[Ss]copes: *//p' <<<"$api_headers")"
    login="$(jq -r '.login // empty' <<<"$api_body" 2>/dev/null || true)"
    if [[ "$api_status" != 200 ]]; then
      problem "GitHub refused GHCR_TOKEN (HTTP $api_status): $(jq -r '.message // empty' <<<"$api_body" 2>/dev/null || printf '%s' "$api_body")"
      token_ok=0
    elif [[ ", $scopes," != *", write:packages,"* ]]; then
      problem "GHCR_TOKEN (user $login) has the scopes \"${scopes:-none}\" but needs write:packages, and must be a classic token (fine-grained tokens do not work with ghcr.io)"
      token_ok=0
    elif [[ -n "$ghcr_user" && "${ghcr_user,,}" != "${login,,}" ]]; then
      problem "GHCR_TOKEN belongs to $login, not GHCR_USER=$ghcr_user"
      token_ok=0
    else
      ghcr_user="$login"
      ok "GHCR_TOKEN is a classic token of $login with write:packages"
    fi
  elif [[ -z "$ghcr_user" ]]; then
    problem "GHCR_USER is not set (needed for $REGISTRY_HOST)"
    token_ok=0
  fi
  if ((token_ok)); then
    push_config="$tmp_root/docker-push"
    docker_config "$push_config"
    if login_out="$(printf '%s' "$ghcr_token" |
      DOCKER_CONFIG="$push_config" docker login "$REGISTRY_HOST" -u "$ghcr_user" --password-stdin 2>&1)"; then
      ok "$REGISTRY_HOST accepts the token (temporary Docker login, deleted at the end)"
    else
      problem "docker login $REGISTRY_HOST with GHCR_TOKEN failed: $(tail -n 1 <<<"$login_out")"
      push_config=""
    fi
  fi
fi

# The package: it must be public so that anyone can pull, and VERSION is never
# overwritten. ghcr.io makes a new package private; see README.md, "Releasing".
repo_path="${IMAGE_REPO#*/}"
pkg_owner="${repo_path%%/*}"
pkg_name="${repo_path#*/}"
owner_kind=users
if [[ -n "$push_config" && "$REGISTRY_HOST" == ghcr.io ]]; then
  # A user's private packages are visible only through /user/packages.
  if [[ "${login,,}" == "${pkg_owner,,}" ]]; then
    pkg_api="user/packages/container/${pkg_name//\//%2F}"
  else
    github_api "users/$pkg_owner"
    [[ "$(jq -r '.type // empty' <<<"$api_body" 2>/dev/null)" == Organization ]] && owner_kind=orgs
    pkg_api="$owner_kind/$pkg_owner/packages/container/${pkg_name//\//%2F}"
  fi
  github_api "$pkg_api"
  if [[ "$api_status" == 404 ]]; then
    note "$IMAGE_REPO does not exist yet. The push creates it as a private package: make it public right after (the script checks, and README.md, \"Releasing\", says how)"
  elif [[ "$api_status" != 200 ]]; then
    problem "could not read the $IMAGE_REPO package from GitHub (HTTP $api_status)"
  else
    visibility="$(jq -r '.visibility // "unknown"' <<<"$api_body")"
    if [[ "$visibility" == public ]]; then
      ok "$IMAGE_REPO is public"
    else
      problem "$IMAGE_REPO is $visibility, so nobody else can pull it: make it public at $(jq -r '.html_url // empty' <<<"$api_body") (Package settings, Change visibility)"
    fi
    if lookup="$(DOCKER_CONFIG="$push_config" docker buildx imagetools inspect "$IMAGE_REPO:$version" 2>&1)"; then
      problem "$IMAGE_REPO:$version is already published"
    elif grep -q -i -E 'not found|manifest unknown' <<<"$lookup"; then
      ok "$IMAGE_REPO:$version is not published yet"
    else
      problem "could not look up $IMAGE_REPO:$version: $(head -n 1 <<<"$lookup")"
    fi
  fi
elif [[ -n "$push_config" ]]; then
  if DOCKER_CONFIG="$push_config" docker buildx imagetools inspect "$IMAGE_REPO:$version" >/dev/null 2>&1; then
    problem "$IMAGE_REPO:$version is already published"
  fi
else
  note "without a working GHCR_TOKEN the package's visibility and versions are not checked"
fi

# The GitHub release, made with the GitHub CLI.
if ! command -v gh >/dev/null 2>&1; then
  problem "the GitHub CLI (gh) is not installed; it creates the GitHub release and uploads the extension zip (https://cli.github.com, then gh auth login)"
elif ! gh_out="$(gh api "repos/$GITHUB_REPO" --jq .permissions.push 2>&1)"; then
  problem "gh cannot read $GITHUB_REPO ($(tail -n 1 <<<"$gh_out")): run gh auth login, or set GH_TOKEN"
elif [[ "$gh_out" != true ]]; then
  problem "gh is logged in as a user who cannot push to $GITHUB_REPO"
elif gh_out="$(gh api "repos/$GITHUB_REPO/releases/tags/$TAG" --jq .html_url 2>&1)"; then
  problem "the GitHub release for $TAG already exists: $gh_out"
elif [[ "$gh_out" == *"HTTP 404"* ]]; then
  ok "gh can create the $TAG release on $GITHUB_REPO"
else
  problem "gh could not check for a $TAG release: $(tail -n 1 <<<"$gh_out")"
fi

if ((${#problems[@]})); then
  if ((dry_run)); then
    printf '\nA release would stop here (%d problem(s) above). The dry run carries on.\n' "${#problems[@]}"
  else
    die "preflight found ${#problems[@]} problem(s); nothing was built or pushed"
  fi
fi

# ----------------------------------------------------------------- checks ----
# The working tree must stay as the preflight saw it: the checks ran on it, and in a
# real release it is the commit that is tagged.
same_tree() {
  [[ "$(git rev-parse HEAD)" == "$revision" ]] || die "HEAD moved during the release (it was ${revision:0:12}); nothing was pushed"
  [[ "$(git status --porcelain --untracked-files=normal)" == "$start_status" ]] ||
    die "files changed during the release (see git status); nothing was pushed"
}

say "scripts/check.sh --quick --e2e --audit"
scripts/check.sh --quick --e2e --audit || die "scripts/check.sh failed; nothing was pushed"
same_tree

# ---------------------------------------------------------------- images ----
# The images and the zip are built from a copy of the commit, so nothing that changes
# in the working tree from here on can reach them. A dry run of a tree with
# uncommitted changes copies the files as they are instead.
src="$tmp_root/src"
mkdir -p "$src"
if [[ -z "$start_status" ]]; then
  git -c tar.umask=0022 archive --format=tar "$revision" | tar -x -m -C "$src"
  source_note="commit ${revision:0:12}"
else
  git ls-files -z --cached --others --exclude-standard --deduplicate |
    while IFS= read -r -d '' path; do
      if [[ -e "$path" || -L "$path" ]]; then
        printf '%s\0' "$path"
      fi
    done |
    tar --null --no-recursion -T - -cf - | tar -x -m -C "$src"
  source_note="the working tree with its uncommitted changes (dry run)"
fi

created="$(TZ=UTC0 git show -s --date=format-local:%Y-%m-%dT%H:%M:%SZ --format=%cd "$revision")"
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

for arch in "${ARCHES[@]}"; do
  image="$(local_tag "$arch")"
  say "image for linux/$arch ($image, from $source_note)"
  DOCKER_BUILDKIT=1 docker build --platform "linux/$arch" "${build_args[@]}" -t "$image" "$src" ||
    die "the linux/$arch image did not build"
  local_images+=("$image")
  platform="$(docker image inspect -f '{{.Os}}/{{.Architecture}}' "$image")"
  [[ "$platform" == "linux/$arch" ]] || die "$image is $platform, not linux/$arch"
  reported="$(docker run --rm --platform "linux/$arch" "$image" --version)" ||
    die "popcornstash --version does not run in the linux/$arch image"
  [[ "$reported" == "popcornstash $version" ]] || die "the linux/$arch image reports '$reported', not popcornstash $version"
  wg="$(docker run --rm --platform "linux/$arch" --entrypoint boringtun-cli "$image" --version)" ||
    die "boringtun-cli does not run in the linux/$arch image"
  ok "linux/$arch: $reported, $wg"
done

# ---------------------------------------------------------- Docker tests ----
# On the very image that is pushed. QEMU cannot run the VPN tests (nftables needs
# netlink), so they run on this machine's own architecture.
tested="$(local_tag "$native")"
say "scripts/docker-smoke.sh on $tested"
IMAGE="$tested" SKIP_BUILD=1 scripts/docker-smoke.sh || die "scripts/docker-smoke.sh failed on $tested; nothing was pushed"
say "scripts/vpn-e2e.sh on $tested"
IMAGE="$tested" SKIP_BUILD=1 scripts/vpn-e2e.sh || die "scripts/vpn-e2e.sh failed on $tested; nothing was pushed"
say "scripts/fullstack-e2e.sh on $tested"
IMAGE="$tested" SKIP_BUILD=1 CI=1 scripts/fullstack-e2e.sh || die "scripts/fullstack-e2e.sh failed on $tested; nothing was pushed"

# ------------------------------------------------------------- extension ----
say "Chrome extension (clean npm ci in the copy of $source_note)"
npm --prefix "$src/extension" ci --ignore-scripts --no-audit --no-fund || die "npm ci for the extension failed"
npm --prefix "$src/extension" run zip || die "npm run zip failed"
mkdir -p "$OUT_DIR"
cp "$src/extension/popcornstash-extension.zip" "$ZIP"
manifest_version="$(jq -r '.version' "$src/extension/dist/manifest.json")"
manifest_name="$(jq -r '.version_name // ""' "$src/extension/dist/manifest.json")"
want_name=""
[[ -n "$prerelease" ]] && want_name="$version"
[[ "$manifest_version" == "$core" && "$manifest_name" == "$want_name" ]] ||
  die "the extension's manifest says version '$manifest_version', version_name '$manifest_name'; it should say '$core' and '$want_name'"
zip_sum="$(sha256 "$ZIP")"
ok "$ZIP (manifest version $manifest_version${want_name:+, version_name $want_name}; sha256 $zip_sum)"

# ------------------------------------------------------- notes and tag ----
changelog_section "$src/CHANGELOG.md" >"$NOTES_FILE"
{
  printf 'PopcornStash %s\n\n' "$version"
  cat "$NOTES_FILE"
} >"$TAG_MESSAGE"
same_tree

say "git tag $TAG$dry_note"
# verbatim: git's default cleanup would drop the notes' "### " headings as comments.
run git tag -a --cleanup=verbatim -F "$TAG_MESSAGE" "$TAG" "$revision"
((dry_run)) || tag_created=1

# ---------------------------------------------------------------- publish ----
title="PopcornStash $version"
gh_release_cmd=(gh release create "$TAG" "$ZIP" --repo "$GITHUB_REPO" --title "$title" --notes-file "$NOTES_FILE" --verify-tag)
if [[ -n "$prerelease" ]]; then
  gh_release_cmd+=(--prerelease)
elif ((move_latest)); then
  gh_release_cmd+=(--latest=true)
else
  gh_release_cmd+=(--latest=false)
fi
if [[ -n "$push_config" ]]; then
  export DOCKER_CONFIG="$push_config"
fi

say "publish$dry_note"
stage=images
sources=()
for arch in "${ARCHES[@]}"; do
  image="$(local_tag "$arch")"
  target="$IMAGE_REPO:$version-$arch"
  sources+=("$target")
  printf '%s (linux/%s, %s) -> %s\n' "$image" "$arch" "$(docker image inspect -f '{{.Id}}' "$image")" "$target"
  run docker tag "$image" "$target"
  ((dry_run)) || local_images+=("$target")
  run docker push "$target" || die "docker push $target failed"
done
tag_args=()
for t in "${image_tags[@]}"; do
  tag_args+=(-t "$t")
done
printf 'multi-arch manifest (linux/amd64, linux/arm64) -> %s\n' "${image_tags[*]}"
run docker buildx imagetools create "${tag_args[@]}" "${sources[@]}" || die "docker buildx imagetools create failed"
((dry_run)) || joined=1
stage=tag
run docker buildx imagetools inspect "$IMAGE_REPO:$version" || die "docker buildx imagetools inspect $IMAGE_REPO:$version failed"
run git push "$REMOTE" "refs/tags/$TAG" || die "git push $REMOTE refs/tags/$TAG failed"
((dry_run)) || tag_pushed=1
stage=release
run "${gh_release_cmd[@]}" || die "gh release create failed"
stage=""

# Anyone must be able to pull the image without logging in.
public=1
if ((dry_run)); then
  printf 'would check: docker buildx imagetools inspect %s with no login\n' "$IMAGE_REPO:$version"
else
  anon_config="$tmp_root/docker-anonymous"
  docker_config "$anon_config"
  if DOCKER_CONFIG="$anon_config" docker buildx imagetools inspect "$IMAGE_REPO:$version" >/dev/null 2>&1; then
    ok "anyone can pull $IMAGE_REPO:$version"
  else
    public=0
  fi
fi

# ---------------------------------------------------------------- report ----
cat <<EOF

Images:    ${image_tags[*]} (linux/amd64, linux/arm64)
Extension: $ZIP (sha256 $zip_sum)
Notes:     $NOTES_FILE

----------------------------------------------------------- release notes
$(<"$NOTES_FILE")
-------------------------------------------------------------------------
EOF

if ((${#local_images[@]})); then
  docker image rm "${local_images[@]}" >/dev/null 2>&1 || true
  printf '\nRemoved the local images this run built (%s).\n' "${local_images[*]}"
  local_images=()
fi

if ((dry_run)); then
  if ((${#problems[@]})); then
    printf '\nDry run finished. A release would have stopped at the preflight:\n'
    printf '  - %s\n' "${problems[@]%%$'\n'*}"
    exit 1
  fi
  printf '\nDry run finished: nothing was pushed, tagged or released. Run scripts/release.sh %s to release.\n' "$version"
elif ((!public)); then
  printf '\nFAIL  Released %s, but %s cannot be pulled without a login.\n' "$version" "$IMAGE_REPO:$version"
  if [[ "$REGISTRY_HOST" == ghcr.io ]]; then
    printf '      ghcr.io makes a new package private. Open\n'
    printf '        https://github.com/%s/%s/packages/container/package/%s\n' "$owner_kind" "$pkg_owner" "$pkg_name"
    printf '      then Package settings, Change visibility, Public (it cannot be made private again).\n'
  fi
  printf '      Then check with no login:\n'
  # shellcheck disable=SC2016 # printed for the reader to run, not expanded here
  printf '        DOCKER_CONFIG=$(mktemp -d) docker buildx imagetools inspect %s\n' "$IMAGE_REPO:$version"
  exit 1
else
  printf '\nReleased %s: https://github.com/%s/releases/tag/%s\n' "$version" "$GITHUB_REPO" "$TAG"
fi

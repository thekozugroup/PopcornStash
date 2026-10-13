#!/usr/bin/env bash
# Docker smoke test for the single PopcornStash container.
#
# Builds the image, runs it with the same security settings as compose.yaml and
# checks the web app, the folder layout, the process users, the torrent kill
# switch with no VPN configured, the internal engine API, and the degraded mode
# without NET_ADMIN. Prints PASS/FAIL lines and exits non-zero on any failure.
#
# Environment:
#   IMAGE              image tag to build and test (default popcornstash:smoke)
#   SKIP_BUILD=1       test an existing IMAGE instead of building it
#   REGISTRY           base image registry (default docker.io/library)
#   DOCKER_HUB_MIRROR=1  use mirror.gcr.io/library when REGISTRY is not set
#   BUILD_CA           optional CA bundle for builds behind a TLS-intercepting proxy
#   EVIDENCE_DIR       where container logs are saved (default evidence/docker-smoke)
#   KEEP=1             leave containers and folders in place for debugging
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${IMAGE:-popcornstash:smoke}"
if [[ -z "${REGISTRY:-}" ]]; then
  if [[ "${DOCKER_HUB_MIRROR:-0}" == "1" ]]; then
    REGISTRY="mirror.gcr.io/library"
  else
    REGISTRY="docker.io/library"
  fi
fi
EVIDENCE_DIR="${EVIDENCE_DIR:-evidence/docker-smoke}"
PUID=1000
PGID=1000
ENGINE_UID=6881
RUN_ID="pst-smoke-$$-${RANDOM}"
NET="${RUN_ID}-net"
ECHO="${RUN_ID}-echo"
MAIN="${RUN_ID}-main"
NOCAP="${RUN_ID}-nocap"
WORK=""

failures=0
passes=0

pass() {
  passes=$((passes + 1))
  printf 'PASS  %s\n' "$1"
}

fail() {
  failures=$((failures + 1))
  printf 'FAIL  %s\n' "$1"
  if [[ -n "${2:-}" ]]; then
    printf '      %s\n' "$2"
  fi
}

note() {
  printf '....  %s\n' "$1"
}

die() {
  printf 'FAIL  %s\n' "$1"
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "this test needs '$1' on the PATH"
}

# check "description" command...: PASS when the command succeeds.
check() {
  local what="$1"
  shift
  local out
  if out="$("$@" 2>&1)"; then
    pass "$what"
  else
    fail "$what" "$(printf '%s' "$out" | tail -n 3 | tr '\n' ' ')"
  fi
}

# check_not "description" command...: PASS when the command fails.
check_not() {
  local what="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    fail "$what" "the command succeeded but should have failed"
  else
    pass "$what"
  fi
}

save_logs() {
  local name
  mkdir -p "$EVIDENCE_DIR"
  for name in "$MAIN" "$NOCAP"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" >"$EVIDENCE_DIR/${name#"${RUN_ID}"-}.log" 2>&1 || true
    fi
  done
}

cleanup() {
  local status=$?
  save_logs
  if [[ "${KEEP:-0}" == "1" ]]; then
    note "KEEP=1: leaving $MAIN, $NOCAP, $ECHO, $NET and $WORK in place"
    exit "$status"
  fi
  docker rm -f "$MAIN" "$NOCAP" "$ECHO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  if [[ -n "$WORK" && -d "$WORK" ]]; then
    # Files belong to PUID and the engine user; remove them from inside a container.
    docker run --rm --user 0:0 --entrypoint /bin/rm -v "$WORK:/work" "$IMAGE" \
      -rf /work/main /work/nocap >/dev/null 2>&1 || true
    rm -rf "$WORK" 2>/dev/null || true
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

need docker
need curl
need jq

# ------------------------------------------------------------------ build ----
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  note "building $IMAGE (base images from $REGISTRY)"
  build_args=(--build-arg "REGISTRY=$REGISTRY" -t "$IMAGE")
  if [[ -n "${BUILD_CA:-}" ]]; then
    build_args+=(--secret "id=build_ca,src=$BUILD_CA")
  fi
  if DOCKER_BUILDKIT=1 docker build "${build_args[@]}" .; then
    pass "image builds"
  else
    die "image builds"
  fi
fi
docker image inspect "$IMAGE" >/dev/null 2>&1 || die "image $IMAGE is not available"

# ---------------------------------------------------------- environment ----
WORK="$(mktemp -d "${TMPDIR:-/tmp}/popcornstash-smoke.XXXXXX")"
chmod 0755 "$WORK"
mkdir -p "$WORK/main/config" "$WORK/main/stash" "$WORK/nocap/config" "$WORK/nocap/stash"
# Like a real share, the library folder already belongs to PUID:PGID. App data is
# left as Docker would create it, so the supervisor has to take ownership of it.
docker run --rm --user 0:0 --entrypoint /bin/chown -v "$WORK:/work" "$IMAGE" \
  "$PUID:$PGID" /work/main/stash /work/nocap/stash >/dev/null

docker network create "$NET" >/dev/null

# A tiny HTTP server on the same Docker network that answers with the caller's IP.
# shellcheck disable=SC2016 # expanded by the echo container's shell
docker run -d --name "$ECHO" --network "$NET" "$REGISTRY/busybox:1.37" sh -c '
  mkdir -p /www/cgi-bin
  printf "%s\n" "#!/bin/sh" "echo Content-Type: text/plain" "echo" "echo \$REMOTE_ADDR" > /www/cgi-bin/ip
  chmod 0755 /www/cgi-bin/ip
  exec httpd -f -p 80 -h /www' >/dev/null
ECHO_IP="$(docker inspect -f "{{(index .NetworkSettings.Networks \"$NET\").IPAddress}}" "$ECHO")"
ECHO_URL="http://$ECHO_IP/cgi-bin/ip"

# The settings from compose.yaml. Extra arguments add or override capabilities.
run_stash() {
  local name="$1" dir="$2"
  shift 2
  docker run -d --name "$name" --network "$NET" \
    -p 127.0.0.1::8788 \
    -e PUID="$PUID" -e PGID="$PGID" \
    -e STASH_PUBLIC_URL=http://localhost:8788 \
    -e STASH_LOG=info \
    -v "$dir/config:/config" -v "$dir/stash:/stash" \
    --cap-drop=ALL \
    --cap-add=CHOWN --cap-add=DAC_OVERRIDE --cap-add=FOWNER \
    --cap-add=SETUID --cap-add=SETGID --cap-add=KILL \
    --sysctl net.ipv4.conf.all.rp_filter=2 \
    --read-only --tmpfs /tmp --tmpfs /run \
    --security-opt=no-new-privileges:true \
    "$@" "$IMAGE" >/dev/null
}

# Wait until Docker reports the container healthy; stop early if it exits.
wait_healthy() {
  local name="$1" status
  for _ in $(seq 1 90); do
    status="$(docker inspect -f '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}' "$name" 2>/dev/null || echo missing)"
    case "$status" in
      "running healthy") return 0 ;;
      running*) sleep 2 ;;
      *)
        printf '      container state: %s\n' "$status"
        return 1
        ;;
    esac
  done
  printf '      container state after 180s: %s\n' "$status"
  return 1
}

base_url() {
  printf 'http://%s' "$(docker port "$1" 8788/tcp | head -n 1)"
}

# stat(1) on the host folders, through a throwaway container so that folders
# owned by PUID or the engine user can be inspected whoever runs this script.
host_stat() {
  docker run --rm --user 0:0 --network none --entrypoint /usr/bin/stat \
    -v "$WORK:/work:ro" "$IMAGE" -c "$1" "/work/$2"
}

# Users and command lines of every process, read from /proc inside the container.
# shellcheck disable=SC2016 # expanded inside the container
processes() {
  docker exec "$1" bash -c '
    for d in /proc/[0-9]*; do
      [ -r "$d/cmdline" ] || continue
      cmd=$(tr "\0" " " < "$d/cmdline" 2>/dev/null)
      [ -n "$cmd" ] || continue
      uid=$(awk "/^Uid:/ {print \$2}" "$d/status" 2>/dev/null)
      printf "%s %s %s\n" "${d#/proc/}" "$uid" "$cmd"
    done'
}

# PID of the first process whose command line contains "$2" and runs as uid "$3".
pid_of() {
  processes "$1" | awk -v want="$2" -v uid="$3" 'index($0, want) && $2 == uid { print $1; exit }'
}

json_field_is() {
  local url="$1" filter="$2"
  curl -fsS -m 10 "$url" | jq -e "$filter" >/dev/null
}

# owner_is <path under $WORK> <uid> [type]
owner_is() {
  local path="$1" want="$2" type="${3:-directory}" got
  got="$(host_stat '%u %F' "$path" 2>&1)" || {
    echo "$path: $got"
    return 1
  }
  [[ "$got" == "$want $type" ]] || {
    echo "$path: owner and type are $got"
    return 1
  }
}

# ------------------------------------------------------- full container ----
note "starting $MAIN with NET_ADMIN and /dev/net/tun"
run_stash "$MAIN" "$WORK/main" --cap-add=NET_ADMIN --device=/dev/net/tun
if wait_healthy "$MAIN"; then
  pass "container becomes healthy (popcornstash healthcheck)"
else
  fail "container becomes healthy (popcornstash healthcheck)"
  docker logs --tail 60 "$MAIN" 2>&1 | sed 's/^/      /'
  exit 1
fi
URL="$(base_url "$MAIN")"

check "/api/health answers ok" json_field_is "$URL/api/health" '.status == "ok"'
check "/api/auth/state: setup_required is true" json_field_is "$URL/api/auth/state" '.setup_required == true'
check "/api/auth/state: passkeys_ready is true" json_field_is "$URL/api/auth/state" '.passkeys_ready == true'

# Logs are captured first: grep -q stops reading early, which would fail the pipe.
logs_of() { docker logs "$1" 2>&1; }

setup_code_logged() {
  local logs
  logs="$(logs_of "$MAIN")"
  grep -qi 'setup code' <<<"$logs" && grep -Eq '\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b' <<<"$logs"
}
check "a setup code appears in the logs" setup_code_logged

serves_ui() {
  local headers body
  headers="$(curl -fsS -m 10 -D - -o /dev/null "$URL/")" &&
    body="$(curl -fsS -m 10 "$URL/")" &&
    grep -qi '^content-type: text/html' <<<"$headers" && grep -qi '<div id="root"' <<<"$body"
}
check "GET / serves the web UI" serves_ui

layout_ok() {
  local dir
  for dir in Movies Shows "Home Videos" Photos Other Inbox Downloads "Downloads/In Progress" "Downloads/Completed"; do
    owner_is "main/stash/$dir" "$PUID" || return 1
  done
}
check "stash layout exists and belongs to PUID $PUID" layout_ok

in_progress_mode() {
  local mode
  mode="$(host_stat '%a %g' "main/stash/Downloads/In Progress")"
  [[ "$mode" == "2775 $PGID" ]] || {
    echo "Downloads/In Progress is mode/group $mode"
    return 1
  }
}
check "Downloads/In Progress is group $PGID, mode 2775" in_progress_mode

config_owned() {
  owner_is "main/config" "$PUID" &&
    owner_is "main/config/popcornstash.db" "$PUID" "regular file" &&
    owner_is "main/config/torrent/engine" "$ENGINE_UID" &&
    [[ "$(host_stat '%a' "main/config/torrent/engine")" == "700" ]]
}
check "app data belongs to PUID; engine state to the engine user (0700)" config_owned

check "serve runs as uid $PUID" test -n "$(pid_of "$MAIN" "popcornstash serve" "$PUID")"
check "torrent-engine runs as uid $ENGINE_UID" test -n "$(pid_of "$MAIN" "popcornstash torrent-engine" "$ENGINE_UID")"
check "the supervisor runs as root" test -n "$(pid_of "$MAIN" "popcornstash run" 0)"

check "nftables table inet popcornstash exists" docker exec "$MAIN" nft list table inet popcornstash
uid_rules() {
  local rules
  rules="$(docker exec "$MAIN" ip -4 rule show)"
  grep -q "uidrange $ENGINE_UID-$ENGINE_UID lookup 51820" <<<"$rules" &&
    grep -q "uidrange $ENGINE_UID-$ENGINE_UID prohibit" <<<"$rules"
}
check "uidrange ip rules route the engine user to table 51820" uid_rules

STASH_IP="$(docker inspect -f "{{(index .NetworkSettings.Networks \"$NET\").IPAddress}}" "$MAIN")"
echo_sees_stash() {
  local seen
  seen="$(docker exec --user "$PUID:$PGID" "$MAIN" curl -fsS -m 5 "$ECHO_URL")" || return 1
  [[ "$seen" == "$STASH_IP" ]] || {
    echo "echo server saw $seen, expected $STASH_IP"
    return 1
  }
}
check "uid $PUID reaches a local echo server (it sees $STASH_IP)" echo_sees_stash
check_not "uid $ENGINE_UID has no network without a VPN" \
  docker exec --user "$ENGINE_UID:$PGID" "$MAIN" curl -fsS -m 5 "$ECHO_URL"
check_not "uid $ENGINE_UID cannot resolve names without a VPN" \
  docker exec --user "$ENGINE_UID:$PGID" "$MAIN" getent hosts "$ECHO"

check "engine /livez answers without a token" \
  docker exec "$MAIN" curl -fsS -m 5 http://127.0.0.1:9091/livez

# The token lives only in the environment of serve and the engine. It is read and
# used inside the container and never printed.
# shellcheck disable=SC2016 # expanded inside the container
engine_status() {
  local token_mode="$1" pid
  pid="$(pid_of "$MAIN" "popcornstash serve" "$PUID")"
  [[ -n "$pid" ]] || return 1
  docker exec --user "$PUID:$PGID" -e MODE="$token_mode" -e PID="$pid" "$MAIN" bash -c '
    token=$(tr "\0" "\n" < "/proc/$PID/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
    [ -n "$token" ] || { echo missing; exit 0; }
    case "$MODE" in
      right) auth=(-H "Authorization: Bearer $token") ;;
      wrong) auth=(-H "Authorization: Bearer 0000$token") ;;
      none) auth=() ;;
    esac
    curl -sS -m 5 -o /dev/null -w "%{http_code}" "${auth[@]}" http://127.0.0.1:9091/health'
}
expect_status() {
  local mode="$1" want="$2" got
  got="$(engine_status "$mode")"
  [[ "$got" == "$want" ]] || {
    echo "got $got"
    return 1
  }
}
check "internal API: no token is refused (401)" expect_status none 401
check "internal API: a wrong token is refused (401)" expect_status wrong 401
check "internal API: the internal token is accepted (200)" expect_status right 200
no_token_file() {
  if host_stat '%n' "main/config/torrent/internal.token" >/dev/null 2>&1; then
    echo "config/torrent/internal.token exists"
    return 1
  fi
}
check "the internal token is never written to disk" no_token_file
engine_accepts_serve() {
  local logs
  sleep 3 # serve checks the engine every few seconds
  logs="$(logs_of "$MAIN")"
  if grep -Eiq 'internal token is missing or wrong|did not accept the internal token' <<<"$logs"; then
    grep -Ei 'internal token' <<<"$logs" | tail -n 1
    return 1
  fi
}
check "the web server's requests to the engine are accepted" engine_accepts_serve
check_not "internal API is not reachable from the network" \
  docker exec "$ECHO" wget -q -T 5 -O - "http://$STASH_IP:9091/livez"
check "web app is reachable from the network" \
  docker exec "$ECHO" wget -q -T 5 -O - "http://$STASH_IP:8788/api/health"

# ------------------------------------------------------- no NET_ADMIN ----
note "starting $NOCAP without NET_ADMIN"
run_stash "$NOCAP" "$WORK/nocap" --device=/dev/net/tun
explains_torrents() { grep -Eiq 'torrent.*NET_ADMIN' <<<"$(logs_of "$NOCAP")"; }
if wait_healthy "$NOCAP"; then
  pass "without NET_ADMIN: container becomes healthy"
  NOCAP_URL="$(base_url "$NOCAP")"
  check "without NET_ADMIN: /api/health answers ok" json_field_is "$NOCAP_URL/api/health" '.status == "ok"'
  nocap_ui() {
    local body
    body="$(curl -fsS -m 10 "$NOCAP_URL/")" && grep -qi '<div id="root"' <<<"$body"
  }
  check "without NET_ADMIN: GET / serves the web UI" nocap_ui
  check "without NET_ADMIN: the logs explain torrents are unavailable" explains_torrents
  check "without NET_ADMIN: serve still runs as uid $PUID" test -n "$(pid_of "$NOCAP" "popcornstash serve" "$PUID")"
  check_not "without NET_ADMIN: no torrent engine is running" test -n "$(pid_of "$NOCAP" "popcornstash torrent-engine" "$ENGINE_UID")"
else
  fail "without NET_ADMIN: container becomes healthy (web UI checks skipped)"
  docker logs --tail 60 "$NOCAP" 2>&1 | sed 's/^/      /'
  check "without NET_ADMIN: the logs explain torrents are unavailable" explains_torrents
fi

# ---------------------------------------------------------------- result ----
echo
if ((failures > 0)); then
  printf 'FAIL  docker smoke: %d passed, %d failed (logs in %s)\n' "$passes" "$failures" "$EVIDENCE_DIR"
  exit 1
fi
printf 'PASS  docker smoke: %d checks passed\n' "$passes"

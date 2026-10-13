#!/usr/bin/env bash
# Docker smoke test for the single PopcornStash container.
#
# Builds the image, runs it with the security settings read from compose.yaml
# and checks the web app, the folder layout, the process users and their
# capabilities, /run, the torrent kill switch with no VPN configured, the
# internal engine API, the admin commands, a clean shutdown, and the degraded
# modes without NET_ADMIN and as a non-root user. It also checks that
# compose.yaml, the Unraid template and the `docker run` example in
# docs/deployment.md ask for the same settings. Prints PASS/FAIL lines and
# exits non-zero on any failure.
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
USERMODE="${RUN_ID}-user"
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
  for name in "$MAIN" "$NOCAP" "$USERMODE"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" >"$EVIDENCE_DIR/${name#"${RUN_ID}"-}.log" 2>&1 || true
    fi
  done
}

cleanup() {
  local status=$?
  save_logs
  if [[ "${KEEP:-0}" == "1" ]]; then
    note "KEEP=1: leaving $MAIN, $NOCAP, $USERMODE, $ECHO, $NET and $WORK in place"
    exit "$status"
  fi
  docker rm -f "$MAIN" "$NOCAP" "$USERMODE" "$ECHO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  if [[ -n "$WORK" && -d "$WORK" ]]; then
    # Files belong to PUID and the engine user; remove them from inside a container.
    docker run --rm --user 0:0 --entrypoint /bin/rm -v "$WORK:/work" "$IMAGE" \
      -rf /work/main /work/nocap /work/user >/dev/null 2>&1 || true
    rm -rf "$WORK" 2>/dev/null || true
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

need docker
need curl
need jq

# ------------------------------------------------------ packaging files ----
# The container settings come from compose.yaml, so this test runs what users run.
COMPOSE_JSON="$(STASH_PUBLIC_URL=http://localhost:8788 APPDATA_PATH=/ STASH_PATH=/ \
  docker compose --env-file /dev/null -f compose.yaml config --format json)" ||
  die "docker compose config could not read compose.yaml"

# The security-relevant `docker run` flags of the compose service, one per line.
compose_flags() {
  jq -r '.services.popcornstash
    | ((.cap_drop // [])[] | "--cap-drop=\(.)"),
      ((.cap_add // [])[] | "--cap-add=\(.)"),
      ((.devices // [])[]
        | (if type == "string" then (split(":") | {source: .[0], target: (.[1] // .[0])}) else . end)
        | if .source == .target then "--device=\(.source)" else "--device=\(.source):\(.target)" end),
      ((.tmpfs // [])[] | "--tmpfs=\(.)"),
      ((.sysctls // {}) | to_entries[] | "--sysctl=\(.key)=\(.value)"),
      ((.security_opt // [])[] | "--security-opt=\(.)"),
      (if .read_only then "--read-only" else empty end),
      (.stop_grace_period // empty | capture("^(?<s>[0-9]+)s$") | "--stop-timeout=\(.s)")' \
    <<<"$COMPOSE_JSON" | sort
}

# The same flags from a `docker run` command line, normalized to --flag=value.
normalize_flags() {
  local -a words
  local i=0 w
  read -r -a words <<<"$1"
  while ((i < ${#words[@]})); do
    w="${words[i]}"
    case "$w" in
      --cap-add=* | --cap-drop=* | --device=* | --sysctl=* | --tmpfs=* | --security-opt=* | --stop-timeout=*)
        printf '%s\n' "$w"
        ;;
      --cap-add | --cap-drop | --device | --sysctl | --tmpfs | --security-opt | --stop-timeout)
        i=$((i + 1))
        printf '%s=%s\n' "$w" "${words[i]:-}"
        ;;
      --read-only | --read-only=true) printf '%s\n' --read-only ;;
    esac
    i=$((i + 1))
  done | sort
}

unraid_flags() {
  normalize_flags "$(sed -n 's:.*<ExtraParams>\(.*\)</ExtraParams>.*:\1:p' unraid/popcornstash.xml)"
}

docs_flags() {
  normalize_flags "$(awk '/^docker run -d --name popcornstash/ { on = 1 } on { print } on && /popcornstash:latest/ { exit }' \
    docs/deployment.md | sed 's/\\$//' | tr '\n' ' ')"
}

same_flags() {
  local other="$1" diff_out
  if diff_out="$(diff <(compose_flags) <("$other"))"; then
    return 0
  fi
  echo "differences (< compose.yaml, > $2): $(grep '^[<>]' <<<"$diff_out" | tr '\n' ' ')"
  return 1
}
check "compose.yaml and the Unraid template ask for the same container settings" same_flags unraid_flags unraid/popcornstash.xml
check "compose.yaml and the docker run example in docs/deployment.md agree" same_flags docs_flags docs/deployment.md

mapfile -t FULL_FLAGS < <(compose_flags)
mapfile -t NOCAP_FLAGS < <(compose_flags | grep -vx -e '--cap-add=NET_ADMIN')
((${#FULL_FLAGS[@]} > 0)) || die "no container settings found in compose.yaml"

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
mkdir -p "$WORK/main/config" "$WORK/main/stash" "$WORK/nocap/config" "$WORK/nocap/stash" \
  "$WORK/user/config" "$WORK/user/stash"
# Like a real share, the library folder already belongs to PUID:PGID. App data is
# left as Docker would create it, so the supervisor has to take ownership of it.
# Started with --user, PopcornStash cannot change owners, so both folders are given
# to that user up front.
docker run --rm --user 0:0 --entrypoint /bin/chown -v "$WORK:/work" "$IMAGE" \
  "$PUID:$PGID" /work/main/stash /work/nocap/stash /work/user/config /work/user/stash >/dev/null

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

# run_stash <name> <dir> <docker run flags...>: the environment from compose.yaml
# with a test address, the two folders, and the given container settings.
run_stash() {
  local name="$1" dir="$2"
  shift 2
  docker run -d --name "$name" --network "$NET" \
    -p 127.0.0.1::8788 \
    -e PUID="$PUID" -e PGID="$PGID" \
    -e STASH_PUBLIC_URL=http://localhost:8788 \
    -e STASH_LOG=info \
    -v "$dir/config:/config" -v "$dir/stash:/stash" \
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

# Logs are captured first: grep -q stops reading early, which would fail the pipe.
logs_of() { docker logs "$1" 2>&1; }

# ------------------------------------------------------- full container ----
note "starting $MAIN with the settings from compose.yaml"
run_stash "$MAIN" "$WORK/main" "${FULL_FLAGS[@]}"
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

private_modes() {
  local path want got
  for path in "config/popcornstash.db 600" "config/cache 700" "config/secrets 700" \
    "config/torrent/vpn 700" "config/pending-torrents 700" "stash/.popcornstash/uploads 700" \
    "stash/Downloads 755"; do
    want="${path##* }"
    got="$(host_stat '%a' "main/${path% *}")"
    [[ "$got" == "$want" ]] || {
      echo "${path% *} is mode $got, not $want"
      return 1
    }
  done
}
check "the database, caches, VPN files and partial uploads are private to PUID" private_modes

# Even without Landlock (docker exec does not apply it), the engine's user and group
# cannot open the web server's private data.
engine_user_shut_out() {
  local path
  for path in /config/popcornstash.db /config/torrent/vpn /config/cache /config/secrets \
    /config/pending-torrents /stash/.popcornstash/uploads; do
    if docker exec -u "$ENGINE_UID:$PGID" "$MAIN" sh -c "if [ -d '$path' ]; then ls '$path'; else head -c 1 '$path'; fi" >/dev/null 2>&1; then
      echo "the engine's user can read $path"
      return 1
    fi
  done
}
check "the engine's user cannot read the database, caches, VPN files or uploads" engine_user_shut_out

# libvips may hand camera RAW files to ImageMagick; the image's policy keeps every
# other ImageMagick coder off, so a disguised PCX under a RAW name is refused.
imagemagick_locked_down() {
  docker exec -u "$PUID:$PGID" "$MAIN" sh -c '
    ffmpeg -nostdin -loglevel error -f lavfi -i color=red:s=16x16 -frames:v 1 -c:v pcx -y /tmp/smoke.pcx &&
    cp /tmp/smoke.pcx /tmp/smoke.raf &&
    if vipsheader /tmp/smoke.raf >/dev/null 2>&1; then rm -f /tmp/smoke.pcx /tmp/smoke.raf; exit 1; fi
    rm -f /tmp/smoke.pcx /tmp/smoke.raf'
}
check "ImageMagick reads only camera RAW coders (policy installed)" imagemagick_locked_down

SERVE_PID="$(pid_of "$MAIN" "popcornstash serve" "$PUID")"
ENGINE_PID="$(pid_of "$MAIN" "popcornstash torrent-engine" "$ENGINE_UID")"
check "serve runs as uid $PUID" test -n "$SERVE_PID"
check "torrent-engine runs as uid $ENGINE_UID" test -n "$ENGINE_PID"
check "the supervisor runs as root" test -n "$(pid_of "$MAIN" "popcornstash run" 0)"

# no_privileges <pid>: no capabilities at all, and no way to gain any.
no_privileges() {
  local status
  [[ -n "$1" ]] || return 1
  status="$(docker exec "$MAIN" cat "/proc/$1/status")" || return 1
  if grep -Eq '^CapPrm:[[:space:]]+0+$' <<<"$status" &&
    grep -Eq '^CapEff:[[:space:]]+0+$' <<<"$status" &&
    grep -Eq '^CapAmb:[[:space:]]+0+$' <<<"$status" &&
    grep -Eq '^NoNewPrivs:[[:space:]]+1$' <<<"$status"; then
    return 0
  fi
  grep -E '^(Cap(Prm|Eff|Amb)|NoNewPrivs):' <<<"$status" | tr '\n' ' '
  return 1
}
check "serve has no capabilities and no_new_privs" no_privileges "$SERVE_PID"
check "torrent-engine has no capabilities and no_new_privs" no_privileges "$ENGINE_PID"

# /run holds the supervisor's socket and status file and boringtun's control
# socket (/var/run/wireguard). Only root may create anything there: an entry made
# by the engine user (for example a fake /run/wireguard/psw0.sock) would receive
# the WireGuard private key from `wg setconf`.
run_is_root_only() {
  local got
  got="$(docker exec "$MAIN" stat -c '%a %u' /run)" || return 1
  [[ "$got" == "755 0" ]] || {
    echo "/run is mode $got (mode, owner)"
    return 1
  }
}
check "/run belongs to root, mode 755" run_is_root_only
check_not "uid $ENGINE_UID cannot create anything in /run" \
  docker exec --user "$ENGINE_UID:$PGID" "$MAIN" mkdir /run/smoke-engine
check_not "uid $PUID cannot create anything in /run" \
  docker exec --user "$PUID:$PGID" "$MAIN" mkdir /run/smoke-serve
# OpenVPN runs its --up/--down hook from /run/popcornstash/openvpn.
# shellcheck disable=SC2016 # expanded inside the container
run_hooks_execute() {
  docker exec "$MAIN" sh -c '
    hook=/run/popcornstash/.smoke-hook
    printf "#!/bin/sh\necho hook-ran\n" > "$hook" && chmod 0700 "$hook"
    out=$("$hook" 2>&1); rm -f "$hook"
    [ "$out" = hook-ran ] || { echo "a root-owned hook in /run/popcornstash cannot run: $out"; exit 1; }'
}
check "the supervisor can run hooks from /run/popcornstash (OpenVPN --up/--down)" run_hooks_execute

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
check "uid $PUID resolves the echo server's name" \
  docker exec --user "$PUID:$PGID" "$MAIN" getent hosts "$ECHO"
check_not "uid $ENGINE_UID has no network without a VPN" \
  docker exec --user "$ENGINE_UID:$PGID" "$MAIN" curl -fsS -m 5 "$ECHO_URL"
check_not "uid $ENGINE_UID cannot resolve names without a VPN" \
  docker exec --user "$ENGINE_UID:$PGID" "$MAIN" getent hosts "$ECHO"

check "engine /livez answers without a token" \
  docker exec "$MAIN" curl -fsS -m 5 http://127.0.0.1:9091/livez

# The token lives only in the environment of serve and the engine. It is read and
# used inside the container and never printed.
# engine_request <none|wrong|right> <code|body>: GET /health on the engine as serve.
# shellcheck disable=SC2016 # expanded inside the container
engine_request() {
  local token_mode="$1" output="$2"
  [[ -n "$SERVE_PID" ]] || return 1
  docker exec --user "$PUID:$PGID" -e MODE="$token_mode" -e OUT="$output" -e PID="$SERVE_PID" "$MAIN" bash -c '
    token=$(tr "\0" "\n" < "/proc/$PID/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
    [ -n "$token" ] || { echo missing; exit 0; }
    case "$MODE" in
      right) auth=(-H "Authorization: Bearer $token") ;;
      wrong) auth=(-H "Authorization: Bearer 0000$token") ;;
      none) auth=() ;;
    esac
    if [ "$OUT" = body ]; then
      curl -fsS -m 5 "${auth[@]}" http://127.0.0.1:9091/health
    else
      curl -sS -m 5 -o /dev/null -w "%{http_code}" "${auth[@]}" http://127.0.0.1:9091/health
    fi'
}
expect_status() {
  local mode="$1" want="$2" got
  got="$(engine_request "$mode" code)"
  [[ "$got" == "$want" ]] || {
    echo "got $got"
    return 1
  }
}
check "internal API: no token is refused (401)" expect_status none 401
check "internal API: a wrong token is refused (401)" expect_status wrong 401
check "internal API: the internal token is accepted (200)" expect_status right 200
engine_blocked() {
  local health
  health="$(engine_request right body)" || return 1
  jq -e '.engine.state == "blocked" and .engine.allow_no_vpn == false
    and .vpn.state == "not_configured" and .vpn.kill_switch == true' <<<"$health" >/dev/null || {
    jq -c '{engine, vpn: {state: .vpn.state, kill_switch: .vpn.kill_switch}}' <<<"$health" 2>/dev/null || echo "$health"
    return 1
  }
}
check "engine /health: blocked, VPN not configured, kill switch on" engine_blocked
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
check_not "internal CONNECT proxy is not reachable from the network" \
  docker exec "$ECHO" nc -w 5 "$STASH_IP" 9092 </dev/null
check "web app is reachable from the network" \
  docker exec "$ECHO" wget -q -T 5 -O - "http://$STASH_IP:8788/api/health"

no_readonly_errors() {
  local logs
  logs="$(logs_of "$MAIN")"
  if grep -Eiq 'read-only file system|os error 30' <<<"$logs"; then
    grep -Ei 'read-only file system|os error 30' <<<"$logs" | tail -n 2
    return 1
  fi
}
check "nothing tried to write to the read-only image" no_readonly_errors

# The documented recovery commands run as root through `docker exec`.
admin_setup_code() {
  local out
  out="$(docker exec "$MAIN" popcornstash admin setup-code 2>&1)" || {
    tail -n 2 <<<"$out"
    return 1
  }
  grep -Eq '\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b' <<<"$out" || {
    echo "no setup code in: $out"
    return 1
  }
}
check "docker exec ... popcornstash admin setup-code prints a setup code" admin_setup_code
config_still_owned() {
  local stray
  stray="$(docker run --rm --user 0:0 --network none --entrypoint /usr/bin/find \
    -v "$WORK:/work:ro" "$IMAGE" /work/main/config \
    -path /work/main/config/torrent/engine -prune -o ! -user "$PUID" -print)" || return 1
  [[ -z "$stray" ]] || {
    echo "not owned by $PUID: $(head -n 3 <<<"$stray" | tr '\n' ' ')"
    return 1
  }
}
check "after the admin command every app data file still belongs to PUID $PUID" config_still_owned

stops_cleanly() {
  local started code
  started=$SECONDS
  docker stop -t 30 "$MAIN" >/dev/null || return 1
  code="$(docker inspect -f '{{.State.ExitCode}}' "$MAIN")"
  # Children get 10s to stop before the supervisor kills them; a clean stop is quicker.
  [[ "$code" == 0 && $((SECONDS - started)) -lt 10 ]] || {
    echo "exit code $code after $((SECONDS - started))s"
    return 1
  }
  grep -q 'stopped cleanly' <<<"$(logs_of "$MAIN")" || {
    echo "the supervisor did not log a clean shutdown"
    return 1
  }
}
check "docker stop: the supervisor stops everything within 10s and exits 0" stops_cleanly

# ------------------------------------------------------- no NET_ADMIN ----
note "starting $NOCAP without NET_ADMIN"
run_stash "$NOCAP" "$WORK/nocap" "${NOCAP_FLAGS[@]}"
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
  nocap_owned() { owner_is "nocap/config" "$PUID" && owner_is "nocap/config/popcornstash.db" "$PUID" "regular file"; }
  check "without NET_ADMIN: app data still belongs to PUID $PUID" nocap_owned
else
  fail "without NET_ADMIN: container becomes healthy (web UI checks skipped)"
  docker logs --tail 60 "$NOCAP" 2>&1 | sed 's/^/      /'
  check "without NET_ADMIN: the logs explain torrents are unavailable" explains_torrents
fi

# ------------------------------------------------------- --user (non-root) ----
note "starting $USERMODE with --user $PUID:$PGID"
run_stash "$USERMODE" "$WORK/user" "${FULL_FLAGS[@]}" --user "$PUID:$PGID"
explains_user() { grep -q 'non-root user, so torrents are turned off' <<<"$(logs_of "$USERMODE")"; }
if wait_healthy "$USERMODE"; then
  pass "with --user: container becomes healthy"
  USER_URL="$(base_url "$USERMODE")"
  check "with --user: /api/health answers ok" json_field_is "$USER_URL/api/health" '.status == "ok"'
  check "with --user: the logs explain torrents are unavailable" explains_user
  check "with --user: serve runs as uid $PUID" test -n "$(pid_of "$USERMODE" "popcornstash serve" "$PUID")"
  check_not "with --user: nothing runs as root" test -n "$(processes "$USERMODE" | awk '$2 == 0')"
else
  fail "with --user: container becomes healthy (checks skipped)"
  docker logs --tail 60 "$USERMODE" 2>&1 | sed 's/^/      /'
fi

# ---------------------------------------------------------------- result ----
echo
if ((failures > 0)); then
  printf 'FAIL  docker smoke: %d passed, %d failed (logs in %s)\n' "$passes" "$failures" "$EVIDENCE_DIR"
  exit 1
fi
printf 'PASS  docker smoke: %d checks passed\n' "$passes"

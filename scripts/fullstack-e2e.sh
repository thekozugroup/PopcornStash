#!/usr/bin/env bash
# Full-stack end-to-end proof in Docker: a torrent added in the web UI downloads
# only through the VPN and lands sorted in the library. See tests/fullstack/README.md.
#
# Six containers on one private Docker network, no internet needed:
#   echo      busybox (tests/vpn/echo.sh): answers with the caller's IP; DNS for the
#             VPN's DNS server
#   tracker   tests/fullstack/tracker.py: HTTP tracker with compact peer lists that
#             logs the address every announce came from
#   seeder    the PopcornStash image running `popcornstash torrent-engine
#             --allow-no-vpn` as root, seeding a small VP9/Opus film named like a
#             release, at 16 KiB/s so progress can be watched
#   wgserver  tests/vpn/wgserver.sh: the VPN provider (WireGuard, NAT to the network)
#   main      PopcornStash started like compose.yaml with an empty stash and config
#
# Playwright (tests/fullstack/specs) then drives the real UI: first-run setup,
# Settings -> VPN with a WireGuard file, Downloads with the magnet, the film in
# Movies, playback, the tracker's and the stash's evidence, and the VPN peer
# stopping and starting again. Prints PASS/FAIL lines and exits non-zero on any
# failure.
#
# Environment:
#   IMAGE              image to build and test (default popcornstash:dev)
#   SKIP_BUILD=1       test an existing IMAGE instead of building it
#   REGISTRY           base image registry (default docker.io/library)
#   DOCKER_HUB_MIRROR=1  use mirror.gcr.io/library when REGISTRY is not set
#   BUILD_CA           optional CA bundle for builds behind a TLS-intercepting proxy
#   SUBNET_PREFIX      first three octets of the test network (default 172.31.67)
#   STASH_PORT         host port on 127.0.0.1 for the web UI (default 8788; the
#                      browser opens http://localhost:<port>, which is STASH_PUBLIC_URL)
#   EVIDENCE_DIR       screenshots, logs and records (default evidence/fullstack)
#   FS_CHROMIUM        another Chromium-based browser binary for Playwright
#   KEEP=1             leave containers and folders in place for debugging
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${IMAGE:-popcornstash:dev}"
if [[ -z "${REGISTRY:-}" ]]; then
  if [[ "${DOCKER_HUB_MIRROR:-0}" == "1" ]]; then
    REGISTRY="mirror.gcr.io/library"
  else
    REGISTRY="docker.io/library"
  fi
fi
EVIDENCE_DIR="${EVIDENCE_DIR:-evidence/fullstack}"
PORT="${STASH_PORT:-8788}"
PUID=1000
PGID=1000
PREFIX="${SUBNET_PREFIX:-172.31.67}"
SUBNET="$PREFIX.0/24"
WG_IP="$PREFIX.10"
ECHO_IP="$PREFIX.20"
STASH_IP="$PREFIX.30"
TRACKER_IP="$PREFIX.40"
SEEDER_IP="$PREFIX.50"
TRACKER_PORT=6969
RUN_ID="pst-fullstack-$$-${RANDOM}"
NET="${RUN_ID}-net"
WG="${RUN_ID}-wgserver"
ECHO="${RUN_ID}-echo"
TRACKER="${RUN_ID}-tracker"
SEEDER="${RUN_ID}-seeder"
MAIN="${RUN_ID}-main"
# The film: a VP9/Opus clip from scripts/dev-fixture.sh, named like a release.
RELEASE="Lantern.Bay.2018.720p.WEB"
FILM="$RELEASE.webm"
TITLE="Lantern Bay"
YEAR=2018
# The seeder's upload limit. librqbit sends 16 KiB blocks and needs at least that.
SEED_KIBPS=16
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

# eventually <seconds> <command...>: retry every 2 s until the command succeeds.
eventually() {
  local deadline=$((SECONDS + $1))
  shift
  local out=""
  while ((SECONDS < deadline)); do
    if out="$("$@" 2>&1)"; then
      return 0
    fi
    sleep 2
  done
  printf '%s\n' "$out" | tail -n 3
  return 1
}

# Logs are captured before grep -q: it stops reading early, which fails the pipe.
logs_of() { docker logs "$1" 2>&1; }

save_evidence() {
  mkdir -p "$EVIDENCE_DIR"
  local name
  for name in "$MAIN" "$WG" "$ECHO" "$TRACKER" "$SEEDER"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" >"$EVIDENCE_DIR/${name#"${RUN_ID}"-}.log" 2>&1 || true
    fi
  done
}

cleanup() {
  local status=$?
  save_evidence
  if [[ "${KEEP:-0}" == "1" ]]; then
    note "KEEP=1: leaving $MAIN, $WG, $ECHO, $TRACKER, $SEEDER, $NET and $WORK in place"
    exit "$status"
  fi
  docker rm -f "$MAIN" "$WG" "$ECHO" "$TRACKER" "$SEEDER" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  if [[ -n "$WORK" && -d "$WORK" ]]; then
    # Files belong to PUID, the engine user and root; remove them from inside a container.
    docker run --rm --user 0:0 --network none --entrypoint /bin/rm -v "$WORK:/work" "$IMAGE" \
      -rf /work/config /work/stash /work/seed /work/keys /work/tracker /work/media /work/upload >/dev/null 2>&1 || true
    rm -rf "$WORK" 2>/dev/null || true
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

need docker
need curl
need jq
need node

if [[ ! -d tests/fullstack/node_modules/@playwright/test ]]; then
  note "installing the Playwright test runner (tests/fullstack)"
  npm --prefix tests/fullstack ci --no-audit --no-fund >/dev/null || die "npm --prefix tests/fullstack ci failed"
fi
if curl -s -m 2 -o /dev/null "http://127.0.0.1:$PORT/"; then
  die "something already listens on 127.0.0.1:$PORT; stop it or set STASH_PORT"
fi

# ------------------------------------------------------- compose settings ----
# The container settings come from compose.yaml, so this test runs what users run.
COMPOSE_JSON="$(STASH_PUBLIC_URL="http://localhost:$PORT" APPDATA_PATH=/ STASH_PATH=/ PUID="$PUID" PGID="$PGID" \
  docker compose --env-file /dev/null -f compose.yaml config --format json)" ||
  die "docker compose config could not read compose.yaml"
mapfile -t COMPOSE_FLAGS < <(jq -r '.services.popcornstash
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
  <<<"$COMPOSE_JSON")
mapfile -t COMPOSE_ENV < <(jq -r '.services.popcornstash.environment // {} | to_entries[] | "\(.key)=\(.value)"' \
  <<<"$COMPOSE_JSON")
((${#COMPOSE_FLAGS[@]} > 0)) || die "no container settings found in compose.yaml"
ENGINE_UID="$(jq -r '.services.popcornstash.environment.STASH_TORRENT_UID // "6881"' <<<"$COMPOSE_JSON")"

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
docker run --rm --network none --entrypoint python3 "$IMAGE" -c 'import http.server' >/dev/null 2>&1 ||
  die "the tracker needs python3 in $IMAGE"
mkdir -p "$EVIDENCE_DIR"

# ---------------------------------------------------------- environment ----
WORK="$(mktemp -d "${TMPDIR:-/tmp}/popcornstash-fullstack.XXXXXX")"
chmod 0755 "$WORK"
mkdir -p "$WORK/config" "$WORK/stash" "$WORK/seed/state" "$WORK/seed/downloads" "$WORK/keys" \
  "$WORK/tracker" "$WORK/media" "$WORK/upload"

in_image() {
  docker run --rm -i --network none --entrypoint "$1" "$IMAGE" "${@:2}"
}

# The film: the 45 s VP9/Opus clip of the sample stash, remuxed to WebM under a
# release name. Made in the image, so the host needs no ffmpeg.
note "making the sample film $FILM"
docker run --rm --network none --entrypoint bash \
  -v "$PWD/scripts/dev-fixture.sh:/dev-fixture.sh:ro" -v "$WORK/media:/out" "$IMAGE" -c '
    set -e
    /dev-fixture.sh /tmp/fixture >/dev/null
    ffmpeg -nostdin -hide_banner -loglevel error -i "/tmp/fixture/Movies/Tin Lighthouse (2014)/Tin Lighthouse (2014).mp4" \
      -map 0 -c copy -f webm "/out/$1"' sh "$FILM" || die "could not make the sample film"
TORRENT_JSON="$(docker run --rm --network none --entrypoint python3 \
  -v "$PWD/tests/fullstack/mktorrent.py:/mktorrent.py:ro" -v "$WORK/media:/media" "$IMAGE" \
  /mktorrent.py "/media/$FILM" "http://$TRACKER_IP:$TRACKER_PORT/announce" "/media/$RELEASE.torrent")" ||
  die "could not make the .torrent"
printf '%s\n' "$TORRENT_JSON" >"$EVIDENCE_DIR/torrent.json"
INFO_HASH="$(jq -r .info_hash <<<"$TORRENT_JSON")"
MAGNET="$(jq -r .magnet <<<"$TORRENT_JSON")"
FILM_BYTES="$(jq -r .length <<<"$TORRENT_JSON")"
note "torrent $INFO_HASH, $FILM_BYTES bytes in $(jq -r .pieces <<<"$TORRENT_JSON") pieces"

# WireGuard keys from `wg` in the image, so the host needs no WireGuard tools.
server_key="$(in_image wg genkey)"
client_key="$(in_image wg genkey)"
server_pub="$(printf '%s' "$server_key" | in_image wg pubkey)"
client_pub="$(printf '%s' "$client_key" | in_image wg pubkey)"
printf '%s\n' "$server_key" >"$WORK/keys/server.key"
printf '%s\n' "$client_pub" >"$WORK/keys/client.pub"
chmod 0600 "$WORK/keys/server.key"
unset server_key
# The file the owner uploads in Settings -> VPN, as a provider would hand it out.
WG_FILE="$WORK/upload/lantern-vpn.conf"
cat >"$WG_FILE" <<CONF
[Interface]
PrivateKey = $client_key
Address = 10.66.0.2/32
DNS = $ECHO_IP

[Peer]
PublicKey = $server_pub
Endpoint = $WG_IP:51820
AllowedIPs = 0.0.0.0/0
CONF
chmod 0600 "$WG_FILE"

# An empty stash that already belongs to PUID:PGID, like a share prepared for the
# container; app data is left as Docker would create it.
docker run --rm --user 0:0 --network none --entrypoint /bin/chown -v "$WORK:/work" "$IMAGE" \
  "$PUID:$PGID" /work/stash >/dev/null

docker network create --subnet "$SUBNET" "$NET" >/dev/null

docker run -d --name "$ECHO" --network "$NET" --ip "$ECHO_IP" -e ECHO_IP="$ECHO_IP" \
  -v "$PWD/tests/vpn/echo.sh:/echo.sh:ro" "$REGISTRY/busybox:1.37" sh /echo.sh >/dev/null

# The tracker and the seeder resolve names only inside the test (echo's DNS), so
# the seeder's DHT never finds the real internet.
docker run -d --name "$TRACKER" --network "$NET" --ip "$TRACKER_IP" --dns "$ECHO_IP" \
  -v "$PWD/tests/fullstack/tracker.py:/tracker.py:ro" -v "$WORK/tracker:/log" \
  --entrypoint python3 "$IMAGE" /tracker.py --port "$TRACKER_PORT" --log /log/announces.jsonl >/dev/null
tracker_ready() { grep -q 'tracker: ready' <<<"$(logs_of "$TRACKER")"; }
eventually 20 tracker_ready || die "the tracker did not start: $(logs_of "$TRACKER" | tail -n 3 | tr '\n' ' ')"

# ---------------------------------------------------------------- seeder ----
SEED_TOKEN="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
start_seeder() {
  docker run -d --name "$SEEDER" --network "$NET" --ip "$SEEDER_IP" --dns "$ECHO_IP" \
    -e STASH_INTERNAL_TOKEN="$SEED_TOKEN" -e STASH_LOG=info \
    -v "$WORK/seed:/seed" "$IMAGE" \
    torrent-engine --allow-no-vpn --state-dir /seed/state --downloads-dir /seed/downloads >/dev/null
}
# seed_api <method> <path> [json body on stdin]: the seeder's internal API, called inside it.
seed_api() {
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec -i -e T="$SEED_TOKEN" -e METHOD="$1" -e API_PATH="$2" "$SEEDER" sh -c '
    if [ "$METHOD" = GET ]; then
      curl -fsS -m 10 -H "Authorization: Bearer $T" "http://127.0.0.1:9091$API_PATH"
    else
      curl -fsS -m 10 -X "$METHOD" -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
        --data-binary @- "http://127.0.0.1:9091$API_PATH"
    fi'
}
seeder_running() { seed_api GET /health | jq -e '.engine.state == "running"' >/dev/null; }
seed_torrent() { seed_api GET /torrents | jq -e --arg id "$INFO_HASH" '.[] | select(.id == $id)'; }
seed_state_is() { seed_torrent | jq -e --arg s "$1" '.state == $s' >/dev/null; }

note "starting the seeder"
start_seeder
eventually 30 seeder_running >/dev/null || die "the seeder's engine did not start: $(logs_of "$SEEDER" | tail -n 3 | tr '\n' ' ')"
# The engine never reuses an existing folder for a new torrent (it would pick
# "Name (2)"), so the seeder is primed in three steps: add the torrent (librqbit
# makes the empty folder and file), stop, put the film in place and drop
# librqbit's fast-resume bitfield, then start again: librqbit re-checks every
# piece and seeds.
jq -n --arg b "$(base64 -w0 "$WORK/media/$RELEASE.torrent")" '{torrent_base64: $b}' | seed_api POST /torrents >/dev/null ||
  die "the seeder refused the torrent"
eventually 30 seed_state_is downloading >/dev/null || die "the seeder did not take the torrent"
docker stop "$SEEDER" >/dev/null
docker run --rm --user 0:0 --network none --entrypoint /bin/sh -v "$WORK:/work" "$IMAGE" -c '
  cp "/work/media/$1" "/work/seed/downloads/$2/$1" && rm -f "/work/seed/state/session/$3.bitv"' \
  sh "$FILM" "$RELEASE" "$INFO_HASH" || die "could not put the film in the seeder's folder"
docker start "$SEEDER" >/dev/null
check "the seeder re-checks the film and seeds it" eventually 60 seed_state_is seeding
printf '{"down_kbps":0,"up_kbps":%s}' "$SEED_KIBPS" | seed_api PUT /limits >/dev/null ||
  die "the seeder refused its upload limit"
seeder_announced() {
  jq -e --arg ip "$SEEDER_IP" --arg h "$INFO_HASH" 'select(.ip == $ip and .info_hash == $h and .left == 0)' \
    "$WORK/tracker/announces.jsonl" >/dev/null
}
check "the seeder announces itself to the tracker as complete" eventually 30 seeder_announced

# -------------------------------------------------------------- wgserver ----
docker run -d --name "$WG" --network "$NET" --ip "$WG_IP" \
  --cap-add NET_ADMIN --device /dev/net/tun --sysctl net.ipv4.ip_forward=1 \
  --stop-timeout 3 \
  -v "$PWD/tests/vpn/wgserver.sh:/wgserver.sh:ro" -v "$WORK/keys:/keys:ro" \
  --entrypoint /bin/sh "$IMAGE" /wgserver.sh >/dev/null
wg_ready() { grep -q 'wgserver: ready' <<<"$(logs_of "$WG")"; }
eventually 30 wg_ready || die "the WireGuard peer did not start: $(logs_of "$WG" | tail -n 3 | tr '\n' ' ')"
note "WireGuard peer is up at $WG_IP ($(logs_of "$WG" | head -n 1))"

# ------------------------------------------------------------ popcornstash ----
note "starting PopcornStash with the settings from compose.yaml"
env_args=()
for kv in "${COMPOSE_ENV[@]}"; do env_args+=(-e "$kv"); done
docker run -d --name "$MAIN" --network "$NET" --ip "$STASH_IP" \
  -p "127.0.0.1:$PORT:8788" \
  "${env_args[@]}" \
  -v "$WORK/config:/config" -v "$WORK/stash:/stash" \
  "${COMPOSE_FLAGS[@]}" \
  "$IMAGE" >/dev/null
printf '%s\n' "${COMPOSE_FLAGS[@]}" "${COMPOSE_ENV[@]}" >"$EVIDENCE_DIR/main-settings.txt"

web_health() { curl -fsS -m 5 "http://127.0.0.1:$PORT/api/health" | jq -e '.status == "ok"' >/dev/null; }
check "PopcornStash answers on http://localhost:$PORT" eventually 90 web_health
setup_code() {
  logs_of "$MAIN" | sed 's/\x1b\[[0-9;]*m//g' | grep -Eo '\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b' | tail -n 1
}
has_setup_code() { [[ -n "$(setup_code)" ]]; }
eventually 30 has_setup_code >/dev/null || true
SETUP_CODE="$(setup_code)"
[[ -n "$SETUP_CODE" ]] || die "no setup code in the container log"
pass "the first-run setup code is in the container log"

# ------------------------------------------------------------- playwright ----
note "driving the web UI with Playwright"
rm -f "$EVIDENCE_DIR"/*.png
pw_status=0
FS_BASE_URL="http://localhost:$PORT" \
  FS_SETUP_CODE="$SETUP_CODE" \
  FS_WG_FILE="$WG_FILE" \
  FS_MAGNET="$MAGNET" \
  FS_INFO_HASH="$INFO_HASH" \
  FS_TORRENT_NAME="$FILM" \
  FS_TORRENT_BYTES="$FILM_BYTES" \
  FS_TITLE="$TITLE" \
  FS_YEAR="$YEAR" \
  FS_STASH="$WORK/stash" \
  FS_EVIDENCE="$PWD/$EVIDENCE_DIR" \
  FS_TRACKER_LOG="$WORK/tracker/announces.jsonl" \
  FS_MAIN="$MAIN" FS_WG="$WG" FS_SEEDER="$SEEDER" \
  FS_MAIN_IP="$STASH_IP" FS_WG_IP="$WG_IP" FS_SEEDER_IP="$SEEDER_IP" \
  FS_PUID="$PUID" FS_PGID="$PGID" FS_ENGINE_UID="$ENGINE_UID" \
  npm --prefix tests/fullstack test || pw_status=$?
if ((pw_status == 0)); then
  pass "the journey in the web UI (Playwright, tests/fullstack/specs)"
else
  fail "the journey in the web UI (Playwright, tests/fullstack/specs)" "see the list above and test-results/fullstack/"
fi

# ---------------------------------------------------------------- after ----
cp "$WORK/tracker/announces.jsonl" "$EVIDENCE_DIR/tracker-announces-all.jsonl" 2>/dev/null || true
seed_api GET /torrents >"$EVIDENCE_DIR/seeder-torrents.json" 2>/dev/null || true
seeder_uploaded_all() {
  seed_torrent | jq -e --argjson n "$FILM_BYTES" '.uploaded_bytes >= $n' >/dev/null
}
check "the seeder uploaded the whole film" seeder_uploaded_all
logs_have_no_secrets() {
  local logs
  logs="$(docker logs "$MAIN" 2>&1)"
  if grep -qF "$client_key" <<<"$logs" || grep -Eq 'PrivateKey|STASH_INTERNAL_TOKEN=[0-9a-f]{8}' <<<"$logs"; then
    echo "the log mentions the WireGuard private key or the internal token"
    return 1
  fi
}
check "the PopcornStash log contains no private key or internal token" logs_have_no_secrets

printf '\n%d passed, %d failed\n' "$passes" "$failures"
((failures == 0))

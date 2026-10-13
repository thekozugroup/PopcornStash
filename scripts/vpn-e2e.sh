#!/usr/bin/env bash
# VPN end-to-end test for the single PopcornStash container (docs/architecture.md
# section 5, "VPN"). See tests/vpn/README.md.
#
# Three containers on a private Docker network:
#   echo       busybox: HTTP server that answers with the caller's IP, and a DNS
#              server for *.test names
#   wgserver   the PopcornStash image used as a WireGuard peer (kernel module or
#              boringtun) that masquerades tunnel traffic toward the network
#   main       PopcornStash itself, with a pre-written WireGuard configuration
#
# It proves that only the engine user's traffic and DNS go through the tunnel,
# that the kill switch blocks the engine when the tunnel is down while the web
# app keeps working, that the tunnel comes back on its own, and that a reload
# from the web server re-applies the VPN. Prints PASS/FAIL lines and exits
# non-zero on any failure.
#
# Environment:
#   IMAGE              image to build and test (default popcornstash:vpn-e2e)
#   SKIP_BUILD=1       test an existing IMAGE instead of building it
#   REGISTRY           base image registry (default docker.io/library)
#   DOCKER_HUB_MIRROR=1  use mirror.gcr.io/library when REGISTRY is not set
#   BUILD_CA           optional CA bundle for builds behind a TLS-intercepting proxy
#   SUBNET_PREFIX      first three octets of the test network (default 172.31.66)
#   EVIDENCE_DIR       where logs and command output are saved (default evidence/vpn-e2e)
#   KEEP=1             leave containers and folders in place for debugging
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${IMAGE:-popcornstash:vpn-e2e}"
if [[ -z "${REGISTRY:-}" ]]; then
  if [[ "${DOCKER_HUB_MIRROR:-0}" == "1" ]]; then
    REGISTRY="mirror.gcr.io/library"
  else
    REGISTRY="docker.io/library"
  fi
fi
EVIDENCE_DIR="${EVIDENCE_DIR:-evidence/vpn-e2e}"
PUID=1000
PGID=1000
ENGINE_UID=6881
PREFIX="${SUBNET_PREFIX:-172.31.66}"
SUBNET="$PREFIX.0/24"
WG_IP="$PREFIX.10"
ECHO_IP="$PREFIX.20"
STASH_IP="$PREFIX.30"
RUN_ID="pst-vpn-$$-${RANDOM}"
NET="${RUN_ID}-net"
WG="${RUN_ID}-wgserver"
ECHO="${RUN_ID}-echo"
MAIN="${RUN_ID}-main"
WORK=""
ECHO_URL="http://$ECHO_IP/cgi-bin/ip"

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

check_not() {
  local what="$1"
  shift
  local out
  if out="$("$@" 2>&1)"; then
    fail "$what" "the command succeeded but should have failed: $(printf '%s' "$out" | tail -n 1)"
  else
    pass "$what"
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

save_evidence() {
  mkdir -p "$EVIDENCE_DIR"
  local name
  for name in "$MAIN" "$WG" "$ECHO"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" >"$EVIDENCE_DIR/${name#"${RUN_ID}"-}.log" 2>&1 || true
    fi
  done
}

cleanup() {
  local status=$?
  save_evidence
  if [[ "${KEEP:-0}" == "1" ]]; then
    note "KEEP=1: leaving $MAIN, $WG, $ECHO, $NET and $WORK in place"
    exit "$status"
  fi
  docker rm -f "$MAIN" "$WG" "$ECHO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  if [[ -n "$WORK" && -d "$WORK" ]]; then
    docker run --rm --user 0:0 --network none --entrypoint /bin/rm -v "$WORK:/work" "$IMAGE" \
      -rf /work/config /work/stash /work/keys >/dev/null 2>&1 || true
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
mkdir -p "$EVIDENCE_DIR"

# ---------------------------------------------------------- environment ----
WORK="$(mktemp -d "${TMPDIR:-/tmp}/popcornstash-vpn.XXXXXX")"
chmod 0755 "$WORK"
mkdir -p "$WORK/config/torrent/vpn" "$WORK/stash" "$WORK/keys"

# Keys come from `wg` in the image, so the host needs no WireGuard tools.
in_image() {
  docker run --rm -i --network none --entrypoint "$1" "$IMAGE" "${@:2}"
}
server_key="$(in_image wg genkey)"
client_key="$(in_image wg genkey)"
server_pub="$(printf '%s' "$server_key" | in_image wg pubkey)"
client_pub="$(printf '%s' "$client_key" | in_image wg pubkey)"
printf '%s\n' "$server_key" >"$WORK/keys/server.key"
printf '%s\n' "$client_pub" >"$WORK/keys/client.pub"
chmod 0600 "$WORK/keys/server.key"

# What the web server writes after a WireGuard upload (section 3).
cat >"$WORK/config/torrent/vpn/wireguard.conf" <<CONF
[Interface]
PrivateKey = $client_key
Address = 10.66.0.2/32
DNS = $ECHO_IP

[Peer]
PublicKey = $server_pub
Endpoint = $WG_IP:51820
AllowedIPs = 0.0.0.0/0
CONF
printf '{"provider":"wireguard","protocol":"wireguard","label":"Local test peer","server":"%s","country":null,"created_at":%s}\n' \
  "$WG_IP" "$(date +%s)" >"$WORK/config/torrent/vpn/vpn.json"
chmod 0600 "$WORK/config/torrent/vpn/wireguard.conf" "$WORK/config/torrent/vpn/vpn.json"
unset server_key
docker run --rm --user 0:0 --network none --entrypoint /bin/chown -v "$WORK:/work" "$IMAGE" \
  -R "$PUID:$PGID" /work/stash /work/config >/dev/null

docker network create --subnet "$SUBNET" "$NET" >/dev/null

docker run -d --name "$ECHO" --network "$NET" --ip "$ECHO_IP" -e ECHO_IP="$ECHO_IP" \
  -v "$PWD/tests/vpn/echo.sh:/echo.sh:ro" "$REGISTRY/busybox:1.37" sh /echo.sh >/dev/null

start_wgserver() {
  docker run -d --name "$WG" --network "$NET" --ip "$WG_IP" \
    --cap-add NET_ADMIN --device /dev/net/tun --sysctl net.ipv4.ip_forward=1 \
    --stop-timeout 3 \
    -v "$PWD/tests/vpn/wgserver.sh:/wgserver.sh:ro" -v "$WORK/keys:/keys:ro" \
    --entrypoint /bin/sh "$IMAGE" /wgserver.sh >/dev/null
}
start_wgserver
wg_ready() { docker logs "$WG" 2>&1 | grep -q 'wgserver: ready'; }
eventually 30 wg_ready || die "the WireGuard peer did not start: $(docker logs "$WG" 2>&1 | tail -n 3 | tr '\n' ' ')"
note "WireGuard peer is up at $WG_IP ($(docker logs "$WG" 2>&1 | head -n 1))"

# The settings from compose.yaml.
docker run -d --name "$MAIN" --network "$NET" --ip "$STASH_IP" \
  -p 127.0.0.1::8788 \
  -e PUID="$PUID" -e PGID="$PGID" -e STASH_TORRENT_UID="$ENGINE_UID" \
  -e STASH_PUBLIC_URL=http://localhost:8788 \
  -e STASH_LOG=info \
  -v "$WORK/config:/config" -v "$WORK/stash:/stash" \
  --cap-drop=ALL \
  --cap-add=NET_ADMIN --cap-add=CHOWN --cap-add=DAC_OVERRIDE --cap-add=FOWNER \
  --cap-add=SETUID --cap-add=SETGID --cap-add=KILL \
  --device=/dev/net/tun \
  --sysctl net.ipv4.conf.all.rp_filter=2 \
  --read-only --tmpfs /tmp --tmpfs /run \
  --security-opt=no-new-privileges:true \
  --stop-timeout 30 \
  "$IMAGE" >/dev/null

base_url() {
  printf 'http://%s' "$(docker port "$MAIN" 8788/tcp | head -n 1)"
}

as_engine() { docker exec --user "$ENGINE_UID:$PGID" "$MAIN" "$@"; }
as_puid() { docker exec --user "$PUID:$PGID" "$MAIN" "$@"; }

# The internal token lives only in the children's environment; it is read and
# used inside the container and never printed.
engine_api() {
  local method="$1" path="$2"
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec --user "$PUID:$PGID" -e METHOD="$method" -e API_PATH="$path" "$MAIN" bash -c '
    pid=""
    for d in /proc/[0-9]*; do
      [ "$(stat -c %u "$d" 2>/dev/null)" = "$(id -u)" ] || continue
      if tr "\0" " " < "$d/cmdline" 2>/dev/null | grep -q "popcornstash serve"; then pid="${d#/proc/}"; break; fi
    done
    [ -n "$pid" ] || { echo "serve is not running" >&2; exit 1; }
    token=$(tr "\0" "\n" < "/proc/$pid/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
    curl -fsS -m 10 -X "$METHOD" -H "Authorization: Bearer $token" "http://127.0.0.1:9091$API_PATH"'
}

engine_connected() {
  engine_api GET /health | jq -e '.vpn.state == "connected" and .engine.state == "running"' >/dev/null
}

engine_sees() {
  local want="$1" seen
  seen="$(as_engine curl -fsS -m 8 "$ECHO_URL")" || return 1
  [[ "$seen" == "$want" ]] || {
    echo "echo server saw $seen, expected $want"
    return 1
  }
}

puid_sees_stash() {
  local seen
  seen="$(as_puid curl -fsS -m 8 "$ECHO_URL")" || return 1
  [[ "$seen" == "$STASH_IP" ]] || {
    echo "echo server saw $seen, expected $STASH_IP"
    return 1
  }
}

web_health() {
  curl -fsS -m 8 "$(base_url)/api/health" | jq -e '.status == "ok"' >/dev/null
}

# ---------------------------------------------------------------- tunnel ----
note "waiting for PopcornStash to connect the tunnel"
if eventually 120 engine_connected; then
  pass "the supervisor connects the tunnel; the engine reports connected/running"
else
  fail "the supervisor connects the tunnel; the engine reports connected/running"
  docker logs --tail 60 "$MAIN" 2>&1 | sed 's/^/      /'
  exit 1
fi
engine_api GET /health >"$EVIDENCE_DIR/health-connected.json" 2>&1 || true

check "the web app answers /api/health" eventually 60 web_health
check "engine uid $ENGINE_UID reaches the echo server through the tunnel (it sees the VPN peer $WG_IP)" engine_sees "$WG_IP"
check "uid $PUID reaches the echo server directly (it sees the container's own $STASH_IP)" puid_sees_stash
check_not "engine uid cannot bypass the tunnel by binding to eth0" \
  as_engine curl -fsS -m 5 --interface eth0 "$ECHO_URL"

engine_resolves() {
  local got
  got="$(as_engine getent hosts echo.test | awk '{print $1}')" || return 1
  [[ "$got" == "$ECHO_IP" ]] || {
    echo "echo.test resolved to '$got'"
    return 1
  }
}
check "engine DNS goes through the forwarder and the tunnel (echo.test resolves to $ECHO_IP)" engine_resolves
check_not "the web server's DNS does not use the tunnel (echo.test is unknown to Docker's resolver)" \
  as_puid getent hosts echo.test

# ---------------------------------------------------------------- rules ----
docker exec "$MAIN" nft list table inet popcornstash >"$EVIDENCE_DIR/nft.txt" 2>&1 || true
docker exec "$MAIN" ip -4 rule show >"$EVIDENCE_DIR/ip-rule.txt" 2>&1 || true
docker exec "$MAIN" ip -4 route show table 51820 >"$EVIDENCE_DIR/route-51820.txt" 2>&1 || true
nft_ok() {
  local t
  t="$(docker exec "$MAIN" nft list table inet popcornstash)"
  grep -q "meta skuid $ENGINE_UID udp dport 53 dnat ip to 127.0.0.54:5354" <<<"$t" &&
    grep -q "meta skuid $ENGINE_UID oifname \"psw0\" accept" <<<"$t" &&
    grep -Eq "meta skuid $ENGINE_UID counter packets [0-9]+ bytes [0-9]+ drop" <<<"$t" &&
    grep -q 'iifname "psw0" ct state established,related accept' <<<"$t"
}
check "nft table inet popcornstash holds the DNS DNAT and the engine-only kill switch" nft_ok
rules_ok() {
  local r
  r="$(docker exec "$MAIN" ip -4 rule show)"
  grep -q "^99:.*fwmark 0x5150 lookup 51820" <<<"$r" &&
    grep -q "^100:.*uidrange $ENGINE_UID-$ENGINE_UID lookup 51820" <<<"$r" &&
    grep -q "^101:.*uidrange $ENGINE_UID-$ENGINE_UID prohibit" <<<"$r" &&
    grep -q "^102:.*fwmark 0x5150 prohibit" <<<"$r"
}
check "ip rules: fwmark and uidrange lookups into table 51820, then prohibit" rules_ok
check "table 51820 routes everything through psw0" bash -c "docker exec '$MAIN' ip -4 route show table 51820 | grep -q '^default dev psw0'"
docker_tables_intact() {
  docker exec "$MAIN" nft list tables | grep -qv 'inet popcornstash'
}
check "Docker's own nftables tables are still there" docker_tables_intact

# -------------------------------------------------------------- processes ----
processes() {
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec "$MAIN" bash -c '
    for d in /proc/[0-9]*; do
      [ -r "$d/cmdline" ] || continue
      cmd=$(tr "\0" " " < "$d/cmdline" 2>/dev/null)
      [ -n "$cmd" ] || continue
      uid=$(awk "/^Uid:/ {print \$2}" "$d/status" 2>/dev/null)
      printf "%s %s %s\n" "${d#/proc/}" "$uid" "$cmd"
    done'
}
processes >"$EVIDENCE_DIR/processes.txt" 2>&1 || true
runs_as() {
  processes | awk -v want="$1" -v uid="$2" 'index($0, want) && $2 == uid { found = 1 } END { exit !found }'
}
check "serve runs as uid $PUID" runs_as "popcornstash serve" "$PUID"
check "torrent-engine runs as uid $ENGINE_UID" runs_as "popcornstash torrent-engine" "$ENGINE_UID"
check "the supervisor runs as root" runs_as "popcornstash run" 0
no_caps() {
  local pid caps
  for pid in $(processes | awk -v p="$PUID" -v e="$ENGINE_UID" '($2 == p || $2 == e) && /popcornstash (serve|torrent-engine)/ {print $1}'); do
    caps="$(docker exec "$MAIN" awk '/^CapEff:|^NoNewPrivs:/ {printf "%s ", $2}' "/proc/$pid/status")"
    [[ "$caps" == "0000000000000000 1 " ]] || {
      echo "pid $pid: CapEff/NoNewPrivs $caps"
      return 1
    }
  done
}
check "serve and the engine have no capabilities and no_new_privs set" no_caps

# ----------------------------------------------------------------- proxy ----
proxy_status() {
  local auth="$1"
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec --user "$PUID:$PGID" -e AUTH="$auth" -e URL="$ECHO_URL" "$MAIN" bash -c '
    pid=$(for d in /proc/[0-9]*; do tr "\0" " " < "$d/cmdline" 2>/dev/null | grep -q "popcornstash serve" && { echo "${d#/proc/}"; break; }; done)
    token=$(tr "\0" "\n" < "/proc/$pid/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
    if [ "$AUTH" = yes ]; then user=(--proxy-user "stash:$token"); else user=(); fi
    curl -sS -m 10 -o /dev/null -w "%{http_code}" -x http://127.0.0.1:9092 "${user[@]}" "$URL"'
}
expect_proxy() {
  local got
  got="$(proxy_status "$1")"
  [[ "$got" == "$2" ]] || {
    echo "got HTTP $got"
    return 1
  }
}
check "CONNECT proxy asks for credentials (407)" expect_proxy no 407
check "CONNECT proxy refuses private addresses (403)" expect_proxy yes 403

# ---------------------------------------------------------- tunnel down ----
note "stopping the WireGuard peer"
docker stop "$WG" >/dev/null
sleep 3
check_not "with the VPN peer gone, the engine uid has no connectivity (kill switch)" \
  as_engine curl -fsS -m 6 "$ECHO_URL"
check_not "with the VPN peer gone, the engine uid cannot reach the echo server over eth0 either" \
  as_engine curl -fsS -m 6 --interface eth0 "$ECHO_URL"
dns_fails_fast() {
  local start=$SECONDS
  if as_engine getent hosts echo.test >/dev/null 2>&1; then
    echo "echo.test still resolves"
    return 1
  fi
  ((SECONDS - start <= 12)) || {
    echo "DNS took $((SECONDS - start)) s to fail"
    return 1
  }
}
check "with the VPN peer gone, engine DNS fails (no leak to Docker's resolver)" dns_fails_fast
check "with the VPN peer gone, uid $PUID still reaches the network directly" puid_sees_stash
check "with the VPN peer gone, the web app still answers /api/health" web_health

note "starting the WireGuard peer again"
docker start "$WG" >/dev/null
check "the tunnel recovers: the engine uid reaches the echo server through the VPN peer again" \
  eventually 150 engine_sees "$WG_IP"

# ---------------------------------------------------------------- reload ----
note "asking the engine to reload the VPN (what the web server does after a VPN change)"
reload_accepted() {
  engine_api POST /vpn/reload >/dev/null
}
check "POST /vpn/reload is accepted" reload_accepted
sleep 5
check "after a reload the engine comes back and reconnects" eventually 120 engine_connected
check "after a reload the engine uid is tunneled again" eventually 60 engine_sees "$WG_IP"
reload_logged() { docker logs "$MAIN" 2>&1 | grep -q 'reload requested; re-applying the VPN'; }
check "the supervisor logged the reload" reload_logged

# --------------------------------------------------------------- shutdown ----
note "stopping PopcornStash"
docker stop "$MAIN" >/dev/null
stopped_cleanly() {
  local code
  code="$(docker inspect -f '{{.State.ExitCode}}' "$MAIN")"
  [[ "$code" == "0" ]] || {
    echo "exit code $code"
    return 1
  }
  docker logs "$MAIN" 2>&1 | grep -q 'stopped cleanly'
}
check "SIGTERM stops children, tunnel and rules, and exits 0" stopped_cleanly
logs_have_no_secrets() {
  local logs
  logs="$(docker logs "$MAIN" 2>&1)"
  if grep -qF "$client_key" <<<"$logs" || grep -Eq 'PrivateKey|STASH_INTERNAL_TOKEN=[0-9a-f]{8}' <<<"$logs"; then
    echo "the log mentions the WireGuard private key or the internal token"
    return 1
  fi
}
check "the logs contain no private key or internal token" logs_have_no_secrets

printf '\n%d passed, %d failed\n' "$passes" "$failures"
((failures == 0))

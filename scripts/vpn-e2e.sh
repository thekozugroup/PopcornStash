#!/usr/bin/env bash
# VPN end-to-end test for the single PopcornStash container (docs/architecture.md
# section 5, "VPN"). See tests/vpn/README.md.
#
# Four containers on a private Docker network:
#   echo       busybox: HTTP server that answers with the caller's IP, and a DNS
#              server for *.test names
#   wgserver   the PopcornStash image used as a WireGuard peer (kernel module or
#              boringtun) that masquerades tunnel traffic toward the network
#   ovpnserver the PopcornStash image used as an OpenVPN server (username and
#              password, topology subnet /24, pushed DNS)
#   main       PopcornStash itself, with a pre-written WireGuard configuration
#
# It proves that only the engine user's traffic and DNS go through the tunnel,
# that the tunnel never adds routes to the main table (the web server's
# traffic stays on the normal network), that the kill switch blocks the engine
# when the tunnel is down while the web app keeps working, that the tunnel
# comes back on its own, that a reload from the web server re-applies the VPN,
# and the same for OpenVPN. Prints PASS/FAIL lines and exits non-zero on any
# failure.
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
OVPN_IP="$PREFIX.11"
ECHO_IP="$PREFIX.20"
STASH_IP="$PREFIX.30"
RUN_ID="pst-vpn-$$-${RANDOM}"
NET="${RUN_ID}-net"
WG="${RUN_ID}-wgserver"
OVPN="${RUN_ID}-ovpnserver"
ECHO="${RUN_ID}-echo"
OVPN_USER="stash"
OVPN_PASS="test-password-$$-${RANDOM}${RANDOM}"
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

skips=0
skip() {
  skips=$((skips + 1))
  printf 'SKIP  %s\n' "$1"
  if [[ -n "${2:-}" ]]; then
    printf '      %s\n' "$2"
  fi
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
  for name in "$MAIN" "$WG" "$OVPN" "$ECHO"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" >"$EVIDENCE_DIR/${name#"${RUN_ID}"-}.log" 2>&1 || true
    fi
  done
}

cleanup() {
  local status=$?
  save_evidence
  if [[ "${KEEP:-0}" == "1" ]]; then
    note "KEEP=1: leaving $MAIN, $WG, $OVPN, $ECHO, $NET and $WORK in place"
    exit "$status"
  fi
  docker rm -f "$MAIN" "$WG" "$OVPN" "$ECHO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  if [[ -n "$WORK" && -d "$WORK" ]]; then
    docker run --rm --user 0:0 --network none --entrypoint /bin/rm -v "$WORK:/work" "$IMAGE" \
      -rf /work/config /work/stash /work/keys /work/ovpn >/dev/null 2>&1 || true
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
mkdir -p "$WORK/config/torrent/vpn" "$WORK/stash" "$WORK/keys" "$WORK/ovpn"

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

# What the web server writes after a WireGuard upload (section 3). The /24 is
# how many providers write it; the tunnel must still add no route to the main
# table.
cat >"$WORK/config/torrent/vpn/wireguard.conf" <<CONF
[Interface]
PrivateKey = $client_key
Address = 10.66.0.2/24
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

# OpenVPN test PKI and the server's password check (keys stay in $WORK).
docker run --rm --network none --entrypoint /bin/sh -v "$WORK/ovpn:/ovpn" "$IMAGE" -c '
  set -e
  cd /ovpn
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -keyout ca.key -out ca.crt -days 2 -subj /CN=popcornstash-test-ca 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -keyout server.key -out server.csr -subj /CN=ovpnserver 2>/dev/null
  printf "basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyAgreement\nextendedKeyUsage=serverAuth\n" >ext.cnf
  openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial \
    -out server.crt -days 2 -extfile ext.cnf 2>/dev/null
  rm -f ca.key server.csr ext.cnf ca.srl
  chmod 0644 ca.crt server.crt
  chmod 0600 server.key' || die "could not create the OpenVPN test certificates"
printf '%s\n%s\n' "$OVPN_USER" "$OVPN_PASS" >"$WORK/ovpn/credentials"
chmod 0600 "$WORK/ovpn/credentials"
cat >"$WORK/ovpn/auth.sh" <<'AUTH'
#!/bin/sh
# OpenVPN auth-user-pass-verify via-file: $1 holds username and password.
[ "$(sed -n 1p "$1")" = "$(sed -n 1p /ovpn/credentials)" ] &&
  [ "$(sed -n 2p "$1")" = "$(sed -n 2p /ovpn/credentials)" ]
AUTH
chmod 0755 "$WORK/ovpn/auth.sh"
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
# Logs are captured before grep -q: it stops reading early, which fails the pipe.
logs_of() { docker logs "$1" 2>&1; }
wg_ready() { grep -q 'wgserver: ready' <<<"$(logs_of "$WG")"; }
eventually 30 wg_ready || die "the WireGuard peer did not start: $(logs_of "$WG" | tail -n 3 | tr '\n' ' ')"
note "WireGuard peer is up at $WG_IP ($(logs_of "$WG" | sed -n 1p))"

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
  --entrypoint /bin/sh "$IMAGE" -c '
    set -eu
    nft add table inet pst_test_preserve
    nft list tables > /run/pst-vpn-tables-before
    exec /usr/bin/tini -- popcornstash run' >/dev/null

base_url() {
  printf 'http://%s' "$(docker port "$MAIN" 8788/tcp | sed -n 1p)"
}

as_engine() { docker exec --user "$ENGINE_UID:$PGID" "$MAIN" "$@"; }
as_puid() { docker exec --user "$PUID:$PGID" "$MAIN" "$@"; }

# The internal token lives only in the children's environment; it is read and
# used inside the container and never printed. SERVE_TOKEN is shell code run
# inside the container: it finds `popcornstash serve` by its executable and
# arguments (not by a text match, which would also find this very shell).
# shellcheck disable=SC2016 # expanded inside the container
SERVE_TOKEN='
    token=""
    for d in /proc/[0-9]*; do
      [ "$(readlink "$d/exe" 2>/dev/null)" = /usr/local/bin/popcornstash ] || continue
      [ "$(tr "\0" "\n" < "$d/cmdline" 2>/dev/null | sed -n 2p)" = serve ] || continue
      token=$(tr "\0" "\n" < "$d/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
      break
    done
    [ -n "$token" ] || { echo "serve is not running" >&2; exit 1; }'
engine_api() {
  local method="$1" path="$2"
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec --user "$PUID:$PGID" -e METHOD="$method" -e API_PATH="$path" "$MAIN" bash -c "$SERVE_TOKEN"'

    curl -fsS -m 10 -X "$METHOD" -H "Authorization: Bearer $token" "http://127.0.0.1:9091$API_PATH"'
}

tunnel_connected() {
  engine_api GET /health | jq -e '.vpn.state == "connected" and .vpn.interface == "psw0"' >/dev/null
}

engine_running() {
  engine_api GET /health | jq -e '.engine.state == "running"' >/dev/null
}

# On a kernel booted with ipv6.disable=1 the engine's sockets fall back to IPv4
# (vendor/librqbit-dualstack-sockets), so the session starts there too.
kernel_has_ipv6() {
  docker exec "$MAIN" test -e /proc/net/if_inet6
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
if eventually 120 tunnel_connected; then
  pass "the supervisor connects the tunnel; the engine reports vpn.state connected on psw0"
else
  fail "the supervisor connects the tunnel; the engine reports vpn.state connected on psw0"
  docker logs --tail 60 "$MAIN" 2>&1 | sed 's/^/      /'
  exit 1
fi
session_note="(engine.state running)"
kernel_has_ipv6 || session_note="on IPv4 sockets (this kernel has no IPv6; engine.state running)"
check "the torrent engine starts its session $session_note" eventually 60 engine_running
engine_api GET /health >"$EVIDENCE_DIR/health-connected.json" 2>&1 || true

check "the web app answers /api/health" eventually 60 web_health
check "engine uid $ENGINE_UID reaches the echo server through the tunnel (it sees the VPN peer $WG_IP)" engine_sees "$WG_IP"
check "uid $PUID reaches the echo server directly (it sees the container's own $STASH_IP)" puid_sees_stash
check_not "engine uid cannot bypass the tunnel by binding to eth0" \
  as_engine curl -fsS -m 5 --interface eth0 "$ECHO_URL"

check_not "the engine cannot forge WireGuard's transport mark" \
  as_engine python3 -c 'import socket; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).setsockopt(socket.SOL_SOCKET, 36, 0x5151)'

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
other_tables_intact() {
  local before tables table
  before="$(docker exec "$MAIN" cat /run/pst-vpn-tables-before)" || return 1
  tables="$(docker exec "$MAIN" nft list tables)" || return 1
  [[ -n "$before" ]] || return 1
  while IFS= read -r table; do
    grep -Fxq "$table" <<<"$tables" || return 1
  done <<<"$before"
}
check "preexisting nftables tables survive initial VPN setup" other_tables_intact

# The tunnel adds no route to the main table, whatever prefix its address has:
# a `10.66.0.0/24 dev psw0` route would pull the web server's traffic for that
# subnet (often the home LAN) into the tunnel.
main_table_clean() {
  local routes
  routes="$(docker exec "$MAIN" ip -4 route show table main)"
  if grep -q psw0 <<<"$routes"; then
    echo "main table: $(grep psw0 <<<"$routes" | tr '\n' ' ')"
    return 1
  fi
}
# web_route_not_tunneled <ip>: the web server's route to <ip> does not use psw0.
web_route_not_tunneled() {
  local route
  route="$(docker exec "$MAIN" ip -4 route get "$1" uid "$PUID")" || return 1
  if grep -q psw0 <<<"$route"; then
    echo "uid $PUID routes $1 through the tunnel: $route"
    return 1
  fi
}
docker exec "$MAIN" ip -4 route show table main >"$EVIDENCE_DIR/route-main.txt" 2>&1 || true
check "the tunnel adds no route to the main table (Address = 10.66.0.2/24)" main_table_clean
check "the web server's traffic to the tunnel's subnet stays on the normal network" web_route_not_tunneled 10.66.0.1
check "psw0 carries the tunnel address as a host address (/32)" \
  bash -c "docker exec '$MAIN' ip -4 -o address show dev psw0 | grep -q 'inet 10.66.0.2/32'"
check_not "the engine user cannot read the WireGuard settings" as_engine wg showconf psw0
check_not "the web server's user cannot read the WireGuard settings" as_puid wg showconf psw0

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
# A socket the root supervisor opened (DNS forwarder, adopt socket, netlink,
# OpenVPN or boringtun control) must never be inherited by a child: compare
# the socket inodes the children hold with the supervisor's. Each process's
# fds are listed as its own user (the container has no CAP_SYS_PTRACE).
pid_of() {
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec "$MAIN" sh -c '
    for d in /proc/[0-9]*; do
      a0=$(tr "\0" "\n" < "$d/cmdline" 2>/dev/null | sed -n 1p)
      a1=$(tr "\0" "\n" < "$d/cmdline" 2>/dev/null | sed -n 2p)
      case "$a0" in
        popcornstash | */popcornstash) [ "$a1" = "$1" ] && { echo "${d#/proc/}"; exit 0; } ;;
      esac
    done
    exit 1' sh "$1"
}
sockets_of() {
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec --user "$1" "$MAIN" sh -c \
    'for fd in /proc/"$1"/fd/*; do readlink "$fd"; done 2>/dev/null | grep "^socket:" | sort -u' sh "$2"
}
no_inherited_sockets() {
  local sup engine serve s e w shared
  if ! sup="$(pid_of run)" || ! engine="$(pid_of torrent-engine)" || ! serve="$(pid_of serve)"; then
    echo "processes not found"
    return 1
  fi
  s="$(sockets_of 0 "$sup")"
  e="$(sockets_of "$ENGINE_UID:$PGID" "$engine")"
  w="$(sockets_of "$PUID:$PGID" "$serve")"
  [[ -n "$s" && -n "$e" && -n "$w" ]] || {
    echo "could not list the sockets (supervisor ${s:+ok}, engine ${e:+ok}, serve ${w:+ok})"
    return 1
  }
  shared="$(comm -12 <(printf '%s\n' "$s") <(printf '%s\n' "$e" "$w" | sort -u))"
  [[ -z "$shared" ]] || {
    echo "shared with the supervisor: $shared"
    return 1
  }
}
check "serve and the engine hold no socket inherited from the root supervisor" no_inherited_sockets

# ----------------------------------------------------------------- proxy ----
proxy_status() {
  local auth="$1"
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec --user "$PUID:$PGID" -e AUTH="$auth" -e URL="$ECHO_URL" "$MAIN" bash -c '
    '"$SERVE_TOKEN"'
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
  ((SECONDS - start <= 20)) || {
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
# put_vpn_file <name>: write stdin to /config/torrent/vpn/<name> as the web
# server's user, as the web server does after an upload.
put_vpn_file() {
  # shellcheck disable=SC2016 # expanded inside the container
  docker exec -i --user "$PUID:$PGID" "$MAIN" sh -c 'umask 077; cat >"/config/torrent/vpn/$1"' sh "$1"
}
# The reload re-reads the files. This WireGuard file names Docker's own resolver
# first: the forwarder must never use it (it answers from the host network).
sed "s/^DNS = .*/DNS = 127.0.0.11, $ECHO_IP/" "$WORK/config/torrent/vpn/wireguard.conf" | put_vpn_file wireguard.conf
note "asking the engine to reload the VPN (what the web server does after a VPN change)"
psw0_index() { docker exec "$MAIN" cat /sys/class/net/psw0/ifindex; }
index_before="$(psw0_index 2>/dev/null || echo none)"
reload_accepted() {
  engine_api POST /vpn/reload >/dev/null
}
check "POST /vpn/reload is accepted" reload_accepted
reload_logged() { grep -q 'reload requested; re-applying the VPN' <<<"$(logs_of "$MAIN")"; }
check "the engine exits for the reload and the supervisor re-applies the VPN" eventually 30 reload_logged
check "after a reload the engine comes back and the tunnel reconnects" eventually 120 tunnel_connected
check "preexisting nftables tables survive a WireGuard reload" other_tables_intact
new_tunnel() {
  local index
  index="$(psw0_index)" || return 1
  [[ "$index" != "$index_before" ]] || {
    echo "psw0 still has ifindex $index"
    return 1
  }
}
check "the reload created a new tunnel interface" new_tunnel
check "after a reload the engine uid is tunneled again" eventually 60 engine_sees "$WG_IP"
check "with DNS = 127.0.0.11 first, engine DNS still goes through the tunnel (echo.test resolves)" \
  eventually 30 engine_resolves
check_not "engine DNS never reaches Docker's resolver (the echo container's name does not resolve)" \
  as_engine getent hosts "$ECHO"

# ---------------------------------------------------------------- OpenVPN ----
note "starting the OpenVPN server"
start_ovpnserver() {
  docker run -d --name "$OVPN" --network "$NET" --ip "$OVPN_IP" \
    --cap-add NET_ADMIN --device /dev/net/tun --sysctl net.ipv4.ip_forward=1 \
    --stop-timeout 3 -e PUSH_DNS="$ECHO_IP" \
    -v "$PWD/tests/vpn/ovpnserver.sh:/ovpnserver.sh:ro" -v "$WORK/ovpn:/ovpn:ro" \
    --entrypoint /bin/sh "$IMAGE" /ovpnserver.sh >/dev/null
}
start_ovpnserver
ovpn_ready() { grep -q 'Initialization Sequence Completed' <<<"$(logs_of "$OVPN")"; }
eventually 30 ovpn_ready || die "the OpenVPN server did not start: $(logs_of "$OVPN" | tail -n 3 | tr '\n' ' ')"

# What the web server writes after an OpenVPN upload: the provider's client
# file, the credentials, and vpn.json selecting OpenVPN.
{
  printf 'client\ndev tun\nproto udp\nremote %s 1194\nnobind\nremote-cert-tls server\nauth-user-pass\nverb 3\n<ca>\n' "$OVPN_IP"
  cat "$WORK/ovpn/ca.crt"
  printf '</ca>\n'
} | put_vpn_file openvpn.ovpn
printf '%s\n%s\n' "$OVPN_USER" "$OVPN_PASS" | put_vpn_file openvpn.auth
printf '{"provider":"openvpn","protocol":"openvpn","label":"Local OpenVPN test server","server":"%s","country":null,"created_at":%s}\n' \
  "$OVPN_IP" "$(date +%s)" | put_vpn_file vpn.json
check "POST /vpn/reload switches to OpenVPN" reload_accepted
openvpn_connected() {
  engine_api GET /health | jq -e '.vpn.state == "connected" and .vpn.protocol == "openvpn" and .vpn.interface == "psw0"' >/dev/null
}
if eventually 120 openvpn_connected; then
  pass "OpenVPN: the supervisor connects the tunnel; the engine reports vpn.state connected"
else
  fail "OpenVPN: the supervisor connects the tunnel; the engine reports vpn.state connected" \
    "$(engine_api GET /health 2>&1 | jq -c .vpn 2>/dev/null)"
  docker logs --tail 40 "$MAIN" 2>&1 | sed 's/^/      /'
fi
docker exec "$MAIN" ip -4 route show table main >"$EVIDENCE_DIR/route-main-openvpn.txt" 2>&1 || true
docker exec "$MAIN" ip -4 address show dev psw0 >"$EVIDENCE_DIR/psw0-openvpn.txt" 2>&1 || true
check "preexisting nftables tables survive switching to OpenVPN" other_tables_intact
check "OpenVPN: engine uid reaches the echo server through the tunnel (it sees $OVPN_IP)" \
  eventually 60 engine_sees "$OVPN_IP"
check "OpenVPN: uid $PUID still reaches the echo server directly" puid_sees_stash
check "OpenVPN: engine DNS uses the pushed server through the tunnel (echo.test resolves)" \
  eventually 30 engine_resolves
check "OpenVPN: the pushed /24 adds no route to the main table" main_table_clean
check "OpenVPN: the web server's traffic to the VPN subnet stays on the normal network" \
  web_route_not_tunneled 10.67.0.1
check "OpenVPN: psw0 carries the pushed address as a host address (/32)" \
  bash -c "docker exec '$MAIN' ip -4 -o address show dev psw0 | grep -q 'inet 10.67.0.2/32'"
openvpn_reads_root_copy() {
  local procs mode
  procs="$(processes)"
  grep -q '^[0-9]* 0 /usr/sbin/openvpn --config /run/popcornstash/openvpn/client.ovpn --auth-user-pass /run/popcornstash/openvpn/auth ' <<<"$procs" || {
    echo "openvpn: $(grep openvpn <<<"$procs")"
    return 1
  }
  mode="$(docker exec "$MAIN" stat -c '%u %a' /run/popcornstash/openvpn /run/popcornstash/openvpn/client.ovpn /run/popcornstash/openvpn/auth | tr '\n' ' ')"
  [[ "$mode" == "0 700 0 600 0 600 " ]] || {
    echo "modes: $mode"
    return 1
  }
}
check "OpenVPN runs as root on a root-only copy of the checked files, not the web server's" openvpn_reads_root_copy
check_not "OpenVPN: the engine user cannot read the staged credentials" \
  as_engine cat /run/popcornstash/openvpn/auth

note "stopping the OpenVPN server"
docker stop "$OVPN" >/dev/null
sleep 3
check_not "OpenVPN down: the engine uid has no connectivity (kill switch)" \
  as_engine curl -fsS -m 6 "$ECHO_URL"
check_not "OpenVPN down: the engine uid cannot reach the echo server over eth0 either" \
  as_engine curl -fsS -m 6 --interface eth0 "$ECHO_URL"
check "OpenVPN down: uid $PUID still reaches the network directly" puid_sees_stash
check "OpenVPN down: the web app still answers /api/health" web_health
note "starting the OpenVPN server again"
docker start "$OVPN" >/dev/null
check "OpenVPN recovers: the engine uid reaches the echo server through the server again" \
  eventually 180 engine_sees "$OVPN_IP"
check "OpenVPN after recovery: still no route in the main table" main_table_clean
no_zombies() {
  local z
  z="$(docker exec "$MAIN" sh -c 'grep -l "^State:.*Z" /proc/[0-9]*/status 2>/dev/null || true')"
  [[ -z "$z" ]] || {
    echo "zombies: $z"
    return 1
  }
}
check "no zombie processes after reloads, tunnel restarts and protocol changes" no_zombies

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
  grep -q 'stopped cleanly' <<<"$(logs_of "$MAIN")"
}
check "SIGTERM stops children, tunnel and rules, and exits 0" stopped_cleanly
logs_have_no_secrets() {
  local logs
  logs="$(docker logs "$MAIN" 2>&1)"
  if grep -qF "$client_key" <<<"$logs" || grep -qF "$OVPN_PASS" <<<"$logs" ||
    grep -Eq 'PrivateKey|STASH_INTERNAL_TOKEN=[0-9a-f]{8}' <<<"$logs"; then
    echo "the log mentions the WireGuard private key, the OpenVPN password or the internal token"
    return 1
  fi
}
check "the logs contain no private key, VPN password or internal token" logs_have_no_secrets

printf '\n%d passed, %d failed, %d skipped\n' "$passes" "$failures" "$skips"
((failures == 0))

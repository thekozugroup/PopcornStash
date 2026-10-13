#!/bin/sh
# WireGuard peer for the VPN end-to-end test (scripts/vpn-e2e.sh).
#
# Runs inside a container started from the PopcornStash image (it already has
# wireguard-tools, nftables, iproute2 and boringtun-cli). It plays the VPN
# provider: one peer, 10.66.0.2/32, and masquerade toward the test network, so
# the echo server sees this container's address for tunneled traffic.
#
# Mounts: /keys/server.key and /keys/client.pub (read-only).
# Needs: NET_ADMIN, /dev/net/tun, sysctl net.ipv4.ip_forward=1.
set -eu

TUN_ADDR="${TUN_ADDR:-10.66.0.1/24}"
CLIENT_ADDR="${CLIENT_ADDR:-10.66.0.2/32}"
BT_PID=""

stop() {
  if [ -n "$BT_PID" ]; then
    kill "$BT_PID" 2>/dev/null || true
  fi
  exit 0
}
trap stop TERM INT

# A restarted container keeps its filesystem; drop a stale control socket.
rm -f /var/run/wireguard/wg0.sock
if ip link add wg0 type wireguard 2>/dev/null; then
  echo "wgserver: kernel WireGuard"
else
  echo "wgserver: kernel WireGuard unavailable, using boringtun"
  boringtun-cli --foreground --disable-drop-privileges wg0 &
  BT_PID=$!
  i=0
  until [ -S /var/run/wireguard/wg0.sock ]; do
    i=$((i + 1))
    if [ "$i" -gt 100 ]; then
      echo "wgserver: boringtun did not start" >&2
      exit 1
    fi
    sleep 0.1
  done
fi

wg set wg0 private-key /keys/server.key listen-port 51820 \
  peer "$(cat /keys/client.pub)" allowed-ips "$CLIENT_ADDR"
ip address add "$TUN_ADDR" dev wg0
ip link set dev wg0 mtu 1420 up

nft -f - <<'NFT'
table ip vpn_e2e
delete table ip vpn_e2e
table ip vpn_e2e {
  chain postrouting {
    type nat hook postrouting priority 100; policy accept;
    ip saddr 10.66.0.0/24 oifname != "wg0" masquerade
  }
}
NFT

echo "wgserver: ready"
while :; do
  if [ -n "$BT_PID" ] && ! kill -0 "$BT_PID" 2>/dev/null; then
    echo "wgserver: boringtun exited" >&2
    exit 1
  fi
  sleep 1 &
  wait $! || true
done

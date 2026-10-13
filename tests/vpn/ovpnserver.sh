#!/bin/sh
# OpenVPN server for the VPN end-to-end test (scripts/vpn-e2e.sh).
#
# Runs inside a container started from the PopcornStash image (it already has
# openvpn, nftables and iproute2). It plays an OpenVPN provider the way most
# do it: username/password logins, `topology subnet` with a /24, a pushed DNS
# server, and masquerade toward the test network, so the echo server sees this
# container's address for tunneled traffic.
#
# Mounts: /ovpn (read-only) with ca.crt, server.crt, server.key, auth.sh.
# Env: PUSH_DNS (the DNS server to push).
# Needs: NET_ADMIN, /dev/net/tun, sysctl net.ipv4.ip_forward=1.
set -eu

cat > /tmp/server.conf <<CONF
port 1194
proto udp
dev tun0
topology subnet
server 10.67.0.0 255.255.255.0
ca /ovpn/ca.crt
cert /ovpn/server.crt
key /ovpn/server.key
dh none
verify-client-cert none
username-as-common-name
script-security 2
auth-user-pass-verify /ovpn/auth.sh via-file
tmp-dir /tmp
push "dhcp-option DNS ${PUSH_DNS}"
keepalive 5 20
verb 3
CONF

nft -f - <<'NFT'
table ip vpn_e2e
delete table ip vpn_e2e
table ip vpn_e2e {
  chain postrouting {
    type nat hook postrouting priority 100; policy accept;
    ip saddr 10.67.0.0/24 oifname != "tun0" masquerade
  }
}
NFT

echo "ovpnserver: starting"
exec openvpn --config /tmp/server.conf

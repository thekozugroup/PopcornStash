#!/bin/sh
# "Internet" for the VPN end-to-end test: an HTTP server that answers with the
# caller's IP address (/cgi-bin/ip) and a DNS server that knows *.test names
# the container's normal resolver does not. Runs in busybox.
#
# Env: ECHO_IP (this container's address).
set -eu
mkdir -p /www/cgi-bin
# shellcheck disable=SC2016 # $REMOTE_ADDR is expanded by the CGI script
printf '%s\n' '#!/bin/sh' 'echo Content-Type: text/plain' 'echo' 'echo $REMOTE_ADDR' > /www/cgi-bin/ip
chmod 0755 /www/cgi-bin/ip
printf 'echo.test %s\n' "$ECHO_IP" > /etc/dnsd.conf
dnsd -c /etc/dnsd.conf -p 53 -i 0.0.0.0 &
trap 'exit 0' TERM INT
httpd -f -p 0.0.0.0:80 -h /www &
wait

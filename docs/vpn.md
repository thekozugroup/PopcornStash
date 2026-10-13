# Torrents and your VPN

PopcornStash only downloads torrents through a VPN. If the VPN is not connected, torrents wait. They never fall back to your home connection.

## How torrent traffic is kept inside the VPN

Everything runs in one container, as three programs:

- **The supervisor** starts first, as root. It sets up the VPN tunnel and the rules below, then starts the other two programs and watches them. It does not serve web pages or download anything.
- **The web app** runs as your `PUID:PGID` user. It holds your library, sign-in and web downloads, and it uses your normal connection, so you can always reach PopcornStash on your network, whatever the VPN is doing.
- **The torrent engine** runs as its own user (`STASH_TORRENT_UID`, 6881 by default). It is the only program whose traffic goes into the VPN.

Several independent protections keep the engine's traffic in the tunnel. Any one of them is enough to stop a leak.

1. **Its own user.** The engine runs without any special permissions. It cannot read the database, your passkeys or the VPN files.
2. **Routing by user.** Every packet from the engine user is sent to a separate routing table that contains only the tunnel (`psw0`). When the tunnel is down that table is empty, and the engine has no route anywhere.
3. **Kill switch.** A firewall rule (nftables) lets the engine user send only through the tunnel, plus the local connections it needs to talk to the web app. Everything else is dropped, even if a route existed.
4. **DNS through the tunnel.** Name lookups from the engine are redirected to a small DNS forwarder in the supervisor, which asks the VPN's DNS servers through the tunnel and nowhere else. When the tunnel is down, lookups fail instead of reaching your internet provider.
5. **Sockets tied to the tunnel.** The torrent library binds every connection to the tunnel interface, so even a routing mistake cannot send torrent traffic out of your normal connection.
6. **Limited file access.** Where the Linux kernel supports Landlock, the engine can only write to `Downloads/In Progress` and its own state folder.

Traffic coming in through the VPN can only reach the engine's listening port. Nothing on the VPN side can reach the web app.

The supervisor needs the `NET_ADMIN` capability and the `/dev/net/tun` device to do this. Both are set in `compose.yaml` and in the Unraid template. Without them PopcornStash still runs, and torrents are switched off. The same files also set `net.ipv4.conf.all.rp_filter=2` for the container, so replies that come back through the tunnel are accepted on hosts that filter incoming traffic strictly. It only affects the container's own network.

WireGuard uses the host's kernel module when it exists (Unraid has it built in). Otherwise PopcornStash uses a bundled userspace WireGuard (boringtun). Nothing needs to be installed on the host.

### Web downloads

Web links can also go through the VPN. The web downloads setting has three choices: through the VPN when it is connected (the default), always through the VPN, or never. With **Always**, a web download fails while the VPN is down instead of using your home connection, and only ordinary web addresses (ports 80 and 443) can be downloaded.

## NordVPN quick setup (access token)

This takes about two minutes and uses NordVPN's fast WireGuard protocol (NordLynx).

1. Sign in to your Nord Account at [my.nordaccount.com](https://my.nordaccount.com).
2. Open **NordVPN**, then **Set up NordVPN manually** (manual setup). Nord sends a verification code to your email.
3. Choose **Generate new token**. Pick a short expiry such as 30 days; PopcornStash only needs it once. Copy the token.
4. In PopcornStash, open **Settings**, then **VPN**. Under **Connect NordVPN**, paste the token into **Access token**. If a VPN is already set up, the group is called **Use a different VPN**; open **Connect NordVPN with a new token** first.
5. Pick a **Country** (or keep **Fastest available**), keep **WireGuard (recommended)** as the **Protocol**, and choose **Connect NordVPN**.

PopcornStash uses the token once to fetch your NordLynx key, picks a server, saves a WireGuard configuration in app data (readable only by PopcornStash), and forgets the token. Choosing **OpenVPN** as the protocol works the same way; PopcornStash downloads NordVPN's OpenVPN file for the chosen server.

## NordVPN with OpenVPN (upload)

Use this if you prefer to pick the server file yourself.

1. In your Nord Account, open **NordVPN**, then **Set up NordVPN manually**, and copy the **service credentials**: a username and password. They are not your email and account password.
2. Download the `.ovpn` file for a server from NordVPN's server list (UDP is faster, TCP passes more firewalls).
3. In PopcornStash, open **Settings**, then **VPN**, and open **Use a WireGuard or OpenVPN file instead**. Choose **OpenVPN file**, pick the `.ovpn` file, enter the **Service username** and **Service password**, and choose **Use this file**.

## Any WireGuard provider

Most VPN providers (Mullvad, Proton VPN, IVPN, AirVPN, Surfshark and others) let you download a WireGuard configuration file.

1. Create a WireGuard configuration in your provider's account page and download the `.conf` file.
2. In PopcornStash, open **Settings**, then **VPN**, and open **Use a WireGuard or OpenVPN file instead**. Keep **WireGuard file** selected, pick the `.conf` file, and choose **Use this file**.

The file needs an `[Interface]` with `PrivateKey` and `Address`, and a `[Peer]` with `PublicKey`, `Endpoint` and `AllowedIPs = 0.0.0.0/0`. If it lists `DNS` servers, PopcornStash uses them for the engine's lookups. A valid `FwMark` line is ignored, because PopcornStash manages routing itself. For the same reason a file with a `Table` or `SaveConfig` line is refused: delete those lines and upload it again. A file with script lines (`PreUp`, `PostUp`, `PreDown` or `PostDown`) is refused: PopcornStash never runs commands from a VPN file, so delete those lines and upload it again.

Any provider with OpenVPN files works the same way through the **OpenVPN file** choice. The `.ovpn` file must be self-contained, with its certificates and keys inside the file (`<ca>`, `<cert>`, `<key>`, `<tls-auth>` blocks) rather than in separate files. Lines that run programs or write files, such as `up`, `down`, `script-security` or `plugin`, are refused. Delete those lines and upload the file again.

## Checking that it works

**Settings**, then **VPN**, shows:

- whether the tunnel is **Connected**, **Connecting** or **Disconnected**, and since when;
- the provider, server and country;
- the **public address** the torrent engine is seen from on the internet, checked through the tunnel every 10 minutes. It should be the VPN's address, not your home address. With NordVPN, PopcornStash also shows whether NordVPN considers the connection protected; if it says no, torrents stop until a later check (every minute) no longer says so;
- that the kill switch is on.

When the VPN is not connected, new torrents are added and wait with the message **Waiting for the VPN to connect**. Torrents beyond the **Torrents at once** setting (on the same page) wait with **Waiting for a free slot**.

You can also check from the server. These commands should show the kill switch table and the routing rules for the engine user:

```sh
docker exec popcornstash nft list table inet popcornstash
docker exec popcornstash ip rule show
```

## When the VPN drops

- The rules stay in place. The engine has no route and the kill switch drops its traffic, so nothing leaks while the tunnel is down.
- Torrents stop moving until the tunnel is back. Nothing is deleted.
- PopcornStash checks the tunnel every 5 seconds and reconnects by itself, waiting a little longer between attempts each time (from 5 seconds up to 5 minutes).
- The web app, playback and the library keep working, because they do not use the VPN.
- **Settings**, then **VPN**, shows the error. An owner can also reconnect from there.

## Running without a VPN

`STASH_ALLOW_NO_VPN=true` runs the torrent engine without a tunnel, routing rules or kill switch. It exists only for networks where the router already sends all traffic through a VPN.

It is off by default because the risk is silent: if the router's VPN ever drops, torrents keep running over your home connection and nothing inside PopcornStash can notice. With the built-in VPN, a drop stops torrents instead. If you turn it on, PopcornStash shows a permanent warning on the VPN and Downloads pages, in the sidebar, on the share page for links and in the extension, even if a VPN is saved. A saved VPN is not used, and the VPN page hides its setup forms.

To turn it on, add `STASH_ALLOW_NO_VPN: "true"` to the `environment` section of `compose.yaml` (on Unraid, add a variable with that name and the value `true`), then recreate the container.

## Not yet tested with real services or hardware

These parts have not been verified against the real services or hardware yet. Each was tested with local stand-ins instead:

- **The NordVPN access token.** The exchange of a token for your service credentials and NordLynx key, and the server choice after it, has only run against a local stand-in for NordVPN's API, never with a real token. The answers of NordVPN's live country list and server recommendations were checked against the code.
- **A real connection to a VPN provider.** No WireGuard or OpenVPN connection to NordVPN or any other commercial provider has been made yet. The tunnel, the kill switch, DNS through the tunnel and a whole torrent download were tested in Docker against local WireGuard and OpenVPN servers (`scripts/vpn-e2e.sh` and `scripts/fullstack-e2e.sh`). OpenVPN 2.6 read real NordVPN, PIA and Surfshark `.ovpn` files without errors, but whether NordVPN's servers agree on a cipher with it is not known yet.
- **The public-address proof through a real tunnel.** The test machines could not reach NordVPN's check or ipify through a tunnel. The check is tested against local stand-ins for both services, including a `protected: false` answer that stops torrents.
- **The VPN on real ARM hardware.** The arm64 image has only run under QEMU emulation, which cannot run the kill switch, so torrents stayed off there.

If you use one of these, check the public address on the VPN page and the commands under [Checking that it works](#checking-that-it-works).

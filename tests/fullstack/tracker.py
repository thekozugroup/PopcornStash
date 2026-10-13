#!/usr/bin/env python3
"""A tiny HTTP BitTorrent tracker for the full-stack test (scripts/fullstack-e2e.sh).

It answers `GET /announce` (BEP 3) with compact IPv4 peer lists (BEP 23) and
logs every announce as one JSON line: the address the request really came
from (the TCP peer, never the client's own `ip=` parameter), the port the
client says it listens on, its event and how much it still needs. That log is
the proof of where a torrent client's traffic came from.

Usage: tracker.py [--port 6969] [--log /log/announces.jsonl] [--interval 10]

Standard library only, so it runs in any image with Python 3 (the PopcornStash
image has it).
"""

import argparse
import json
import signal
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from ipaddress import IPv4Address
from urllib.parse import parse_qs, urlsplit

# Peers that have not announced for this long are dropped from the swarm.
PEER_TTL_S = 300


def bencode(value) -> bytes:
    if isinstance(value, bool):
        raise TypeError("bencode has no booleans")
    if isinstance(value, int):
        return b"i%de" % value
    if isinstance(value, str):
        value = value.encode()
    if isinstance(value, (bytes, bytearray)):
        return b"%d:%s" % (len(value), bytes(value))
    if isinstance(value, list):
        return b"l" + b"".join(bencode(v) for v in value) + b"e"
    if isinstance(value, dict):
        items = sorted((k.encode() if isinstance(k, str) else k, v) for k, v in value.items())
        return b"d" + b"".join(bencode(k) + bencode(v) for k, v in items) + b"e"
    raise TypeError(f"cannot bencode {type(value).__name__}")


class Swarms:
    """info_hash -> peer_id -> (ip, port, left, last_seen)."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._swarms: dict[bytes, dict[bytes, tuple[str, int, int, float]]] = {}

    def announce(self, info_hash: bytes, peer_id: bytes, ip: str, port: int, left: int, event: str):
        now = time.monotonic()
        with self._lock:
            swarm = self._swarms.setdefault(info_hash, {})
            for pid in [p for p, (_, _, _, seen) in swarm.items() if now - seen > PEER_TTL_S]:
                del swarm[pid]
            # A restarted client comes back with a new peer id from the same address.
            for pid in [p for p, (pip, pport, _, _) in swarm.items() if (pip, pport) == (ip, port)]:
                del swarm[pid]
            if event != "stopped":
                swarm[peer_id] = (ip, port, left, now)
            others = [(pip, pport) for pid, (pip, pport, _, _) in swarm.items() if pid != peer_id]
            complete = sum(1 for (_, _, pleft, _) in swarm.values() if pleft == 0)
            incomplete = len(swarm) - complete
        return others, complete, incomplete


class Handler(BaseHTTPRequestHandler):
    server_version = "fullstack-tracker/1"
    swarms: Swarms
    log_file = None
    interval = 10

    def log_message(self, fmt, *args):  # quiet default access log; announces are logged as JSON
        pass

    def _reply(self, status: int, body: bytes, content_type="text/plain"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _failure(self, reason: str):
        self._reply(200, bencode({"failure reason": reason}))

    def do_GET(self):
        url = urlsplit(self.path)
        if url.path == "/health":
            self._reply(200, b"ok\n")
            return
        if url.path != "/announce":
            self._reply(404, b"not found\n")
            return
        # info_hash and peer_id are raw bytes, percent-encoded: decode as latin-1.
        query = parse_qs(url.query, encoding="latin-1", keep_blank_values=True)

        def raw(name: str) -> bytes | None:
            values = query.get(name)
            return values[0].encode("latin-1") if values else None

        info_hash, peer_id = raw("info_hash"), raw("peer_id")
        if info_hash is None or len(info_hash) != 20 or peer_id is None:
            self._failure("info_hash and peer_id are required")
            return
        try:
            port = int(query.get("port", ["0"])[0])
            left = int(query.get("left", ["0"])[0])
        except ValueError:
            self._failure("port and left must be numbers")
            return
        event = query.get("event", [""])[0]
        ip = self.client_address[0]
        others, complete, incomplete = self.swarms.announce(info_hash, peer_id, ip, port, left, event)
        entry = {
            "at": round(time.time(), 3),
            "ip": ip,
            "port": port,
            "event": event or "update",
            "left": left,
            "info_hash": info_hash.hex(),
            "peer_id": peer_id.decode("latin-1").encode("unicode_escape").decode("ascii"),
            "claimed_ip": query.get("ip", [None])[0],
            "peers_returned": [f"{pip}:{pport}" for pip, pport in others],
        }
        line = json.dumps(entry, sort_keys=True)
        print(line, flush=True)
        if self.log_file is not None:
            self.log_file.write(line + "\n")
            self.log_file.flush()
        compact = b""
        for pip, pport in others:
            try:
                compact += IPv4Address(pip).packed + pport.to_bytes(2, "big")
            except ValueError:
                continue  # not IPv4 (BEP 23 compact lists are IPv4 only)
        self._reply(
            200,
            bencode(
                {
                    "interval": self.interval,
                    "min interval": max(1, self.interval // 2),
                    "complete": complete,
                    "incomplete": incomplete,
                    "peers": compact,
                }
            ),
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=6969)
    parser.add_argument("--log", default=None, help="append one JSON line per announce to this file")
    parser.add_argument("--interval", type=int, default=10, help="seconds between announces")
    args = parser.parse_args()
    Handler.swarms = Swarms()
    Handler.interval = args.interval
    if args.log:
        Handler.log_file = open(args.log, "a", encoding="utf-8")  # noqa: SIM115 (open for the process lifetime)
    server = ThreadingHTTPServer((args.bind, args.port), Handler)
    # PID 1 in a container ignores SIGTERM unless it handles it.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    print(f"tracker: ready on {args.bind}:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())

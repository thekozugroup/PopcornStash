#!/usr/bin/env python3
"""Make a single-file BitTorrent v1 .torrent and its magnet link (scripts/fullstack-e2e.sh).

Usage: mktorrent.py <file> <announce-url> <out.torrent> [--piece-length 16384]

Prints one JSON object: {"name", "length", "piece_length", "pieces", "info_hash",
"magnet"}. The magnet carries the info hash, the display name (dn) and the
tracker (tr), like the links people paste from a torrent site.

Standard library only.
"""

import argparse
import hashlib
import json
import os
import sys
from urllib.parse import quote


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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("file")
    parser.add_argument("announce")
    parser.add_argument("out")
    # Small pieces give the progress bar many steps for a small sample file.
    parser.add_argument("--piece-length", type=int, default=16 * 1024)
    args = parser.parse_args()
    if args.piece_length < 16 * 1024 or args.piece_length & (args.piece_length - 1):
        parser.error("--piece-length must be a power of two of at least 16 KiB")

    name = os.path.basename(args.file)
    pieces = bytearray()
    length = 0
    with open(args.file, "rb") as f:
        while chunk := f.read(args.piece_length):
            pieces += hashlib.sha1(chunk).digest()
            length += len(chunk)
    if length == 0:
        parser.error("the file is empty")

    info = {"length": length, "name": name, "piece length": args.piece_length, "pieces": bytes(pieces)}
    info_hash = hashlib.sha1(bencode(info)).hexdigest()
    torrent = {"announce": args.announce, "created by": "popcornstash fullstack-e2e", "info": info}
    with open(args.out, "wb") as f:
        f.write(bencode(torrent))

    magnet = f"magnet:?xt=urn:btih:{info_hash}&dn={quote(name, safe='')}&tr={quote(args.announce, safe='')}"
    json.dump(
        {
            "name": name,
            "length": length,
            "piece_length": args.piece_length,
            "pieces": len(pieces) // 20,
            "info_hash": info_hash,
            "magnet": magnet,
        },
        sys.stdout,
    )
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

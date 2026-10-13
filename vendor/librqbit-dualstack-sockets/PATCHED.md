# Local patch

Vendored from crates.io `librqbit-dualstack-sockets` 0.7.0 (Apache-2.0) for PopcornStash.

Change: `MaybeDualstackSocket::bind` falls back to `0.0.0.0` when binding the
unspecified IPv6 address fails with `EAFNOSUPPORT` (kernels booted with
`ipv6.disable=1`). Without it, librqbit's UDP tracker client and listeners
fail and torrents never start on such hosts. Remove once upstream handles it.

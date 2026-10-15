# Local patch

Vendored from crates.io `librqbit` 9.0.1 (Apache-2.0, upstream commit
a499d2f243d124e144aef137afe7cb304a6e3f36) for PopcornStash. `webui/` (the
TypeScript UI behind the `webui` feature, unused here) and `resources/` (test
fixtures) are left out; `LICENSE` is upstream's notice.

Change: a dead peer that a tracker, the DHT or PEX lists again is reconnected.

Upstream, a peer whose connection fails waits out an exponential backoff (10 s,
1 min, 6 min, 36 min, then hourly, each up to twice as long with jitter) before
the next try, and `add_peer_if_not_seen` ignores every address already in the
peer table, so the tracker announcing the peer again changes nothing. A lone
seeder that paused or went offline for a few minutes was retried only an hour or
more later; the download sat at zero peers until it was paused and resumed.

Now `add_peer_if_not_seen` re-queues a known peer in the `Dead` state at once
when it has been dead for at least 10 s (`REQUEUE_LISTED_DEAD_PEER_AFTER`, the
shortest upstream backoff). Sources stop listing peers that stop announcing, so
only peers still in the swarm keep being retried: at most once per announce, and
never sooner than 10 s after the peer last failed. Peers that are queued,
connecting, live or not needed are untouched, and so is the backoff timer (it
still retries the peer between announces, as upstream does).

Files: `src/torrent_state/live/mod.rs` (`REQUEUE_LISTED_DEAD_PEER_AFTER`,
`add_peer_if_not_seen`), `src/torrent_state/live/peers/mod.rs`
(`PeerStates::requeue_if_dead`), `src/torrent_state/live/peer/mod.rs`
(`Peer::dead_since`, `Peer::requeue_if_dead`, `set_state`). Each change is
marked "PopcornStash patch"; `diff -ru` against the crates.io 9.0.1 source shows
the whole change.

Test: `a_seeder_that_comes_back_is_reconnected_while_the_tracker_lists_it` in
`crates/stash-torrentd/src/engine.rs` (fails without this patch). Remove the
patch once upstream reconnects re-announced peers.

Metadata recovery (2026-10-15): `src/dht_utils.rs` also retries a re-announced
metadata peer 10 seconds after its failed attempt, deduplicating concurrent
attempts. Upstream's permanent `seen` set ignored that peer for the entire
metadata lookup, so a magnet could wait indefinitely after one failed
connection. Cancellation still drops every in-flight attempt with the lookup.

`src/session.rs` advertises an unresolved magnet with one byte left and the
initializing state, replaced by real stats once metadata is registered. The
upstream zero-byte default told trackers the magnet was already complete;
trackers that filter seed-to-seed connections could withhold its seeders.

Tests: `metadata_lookup_announces_missing_bytes_to_trackers` uses a tracker
that withholds peers from completed clients; `metadata_lookup_retries_a_peer_that_comes_back`
lists a paused seeder repeatedly, then resumes it. Both exercise real metadata
exchange with DHT and local discovery disabled, and fail without these fixes.

## Private torrents and session trackers

`session.rs::make_peer_rx` excludes session-wide public trackers for every private torrent,
including private torrents with zero or one original tracker. Previously only the case
with multiple original trackers skipped the global list. The engine regression
`private_torrents_never_announce_to_the_session_public_tracker` observes real local tracker
requests for public/private metadata and asserts the private torrent only announces to
its original tracker.

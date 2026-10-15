use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use buffers::ByteBufOwned;
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use librqbit_core::torrent_metainfo::TorrentMetaV1Info;
use tracing::{Instrument, debug, debug_span};

use crate::{
    peer_connection::PeerConnectionOptions, peer_info_reader, spawn_utils::BlockingSpawner,
    stream_connect::StreamConnector,
};
use librqbit_core::hash_id::Id20;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ReadMetainfoResult<Rx> {
    Found {
        info: TorrentMetaV1Info<ByteBufOwned>,
        info_bytes: ByteBufOwned,
        rx: Rx,
        seen: HashSet<SocketAddr>,
    },
    ChannelClosed {
        #[allow(dead_code)]
        seen: HashSet<SocketAddr>,
    },
}

pub async fn read_metainfo_from_peer_receiver<A: Stream<Item = SocketAddr> + Unpin>(
    peer_id: Id20,
    info_hash: Id20,
    initial_addrs: Vec<SocketAddr>,
    addrs_stream: A,
    peer_connection_options: Option<PeerConnectionOptions>,
    connector: Arc<StreamConnector>,
    client_name_and_version: String,
) -> ReadMetainfoResult<A> {
    let mut seen = HashSet::<SocketAddr>::new();
    // PopcornStash patch: metadata peers can disappear and return just like data
    // peers. Deduplicate active attempts, but retry a re-announced failed peer.
    let mut failed_at = HashMap::<SocketAddr, Instant>::new();
    let mut in_flight = HashSet::<SocketAddr>::new();
    const RETRY_LISTED_PEER_AFTER: Duration = Duration::from_secs(10);
    let mut addrs = addrs_stream;

    let semaphore = tokio::sync::Semaphore::new(128);

    let read_info_guarded = |addr| {
        let semaphore = &semaphore;
        let connector = connector.clone();
        let client_name_and_version = client_name_and_version.clone();
        async move {
            let ret = async {
                let _token = semaphore.acquire().await?;
                peer_info_reader::read_metainfo_from_peer(
                    addr,
                    peer_id,
                    info_hash,
                    peer_connection_options,
                    // This shouldn't be called anyway as we aren't reading/writing to disk, so it's
                    // ok not to use a shared one.
                    BlockingSpawner::new(1),
                    connector,
                    client_name_and_version,
                )
                .instrument(debug_span!("read_metainfo_from_peer", ?addr))
                .await
                .with_context(|| format!("error reading metainfo from {addr}"))
            }
            .await;
            (addr, ret)
        }
    };

    let mut unordered = FuturesUnordered::new();

    for a in initial_addrs {
        if seen.insert(a) {
            in_flight.insert(a);
            unordered.push(read_info_guarded(a));
        }
    }

    let mut addrs_completed = false;

    loop {
        if addrs_completed && unordered.is_empty() {
            return ReadMetainfoResult::ChannelClosed { seen };
        }

        tokio::select! {
            done = unordered.next(), if !unordered.is_empty() => {
                match done {
                    Some((_, Ok((info, info_bytes)))) => return ReadMetainfoResult::Found { info, info_bytes, seen, rx: addrs },
                    Some((addr, Err(e))) => {
                        in_flight.remove(&addr);
                        failed_at.insert(addr, Instant::now());
                        debug!("{:#}", e);
                    },
                    None => unreachable!()
                }
            }

            next_addr = addrs.next(), if !addrs_completed => {
                match next_addr {
                    Some(addr) => {
                        let new = seen.insert(addr);
                        let retry = failed_at.get(&addr).is_some_and(|at| at.elapsed() >= RETRY_LISTED_PEER_AFTER);
                        if (new || retry) && in_flight.insert(addr) {
                            failed_at.remove(&addr);
                            unordered.push(read_info_guarded(addr));
                        }
                        continue;
                    },
                    None => {
                        addrs_completed = true;
                    },
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use dht::{DhtBuilder, Id20};
    use librqbit_core::peer_id::generate_peer_id;

    use super::*;
    use std::{
        str::FromStr,
        sync::{Arc, Once},
    };

    static LOG_INIT: Once = Once::new();

    fn init_logging() {
        #[allow(unused_must_use)]
        LOG_INIT.call_once(|| {
            // pretty_env_logger::try_init();
        })
    }

    #[tokio::test]
    #[ignore]
    async fn read_metainfo_from_dht() {
        init_logging();

        let info_hash = Id20::from_str("cab507494d02ebb1178b38f2e9d7be299c86b862").unwrap();
        let dht = DhtBuilder::new().await.unwrap();

        let peer_rx = dht.get_peers(info_hash, None);
        let peer_id = generate_peer_id(b"-xx1234-");
        match read_metainfo_from_peer_receiver(
            peer_id,
            info_hash,
            Vec::new(),
            peer_rx,
            None,
            Arc::new(StreamConnector::new(Default::default()).await.unwrap()),
            crate::client_name_and_version().to_owned(),
        )
        .await
        {
            ReadMetainfoResult::Found { info, .. } => dbg!(info),
            ReadMetainfoResult::ChannelClosed { .. } => todo!("should not have happened"),
        };
    }
}

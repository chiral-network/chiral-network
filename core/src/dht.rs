use libp2p::PeerId;
use libp2p::Swarm;
use libp2p::SwarmBuilder;
use libp2p::kad::RecordKey;
use libp2p::kad::{
    Behaviour as KademliaBehaviour, Config as KademliaConfig, Event as KademliaEvent, QueryResult,
    Quorum, Record, store::MemoryStore,
};

use libp2p::noise::Config as NoiseConfig;
use libp2p::swarm::SwarmEvent;
use libp2p::tls::Config as TlsConfig;
use libp2p::yamux::Config as YamuxConfig;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use std::error::Error;
use std::time::Duration;

use futures_util::stream::StreamExt;
use log::info;

pub struct DHTNode {
    swarm: Swarm<KademliaBehaviour<MemoryStore>>,
    peer_id: PeerId,
    is_running: Arc<AtomicBool>,
}

impl DHTNode {
    pub fn create() -> Result<Self, Box<dyn Error>> {
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(local_key.public());

        let mut cfg = KademliaConfig::default();
        cfg.set_query_timeout(Duration::from_secs(60));

        let store = MemoryStore::new(peer_id);
        let kademlia = KademliaBehaviour::with_config(peer_id, store, cfg);

        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                Default::default(),
                (TlsConfig::new, NoiseConfig::new),
                || YamuxConfig::default(),
            )?
            .with_behaviour(|_key| kademlia)?
            .build();

        let is_running = Arc::new(AtomicBool::new(false));

        Ok(DHTNode {
            swarm,
            peer_id,
            is_running,
        })
    }

    async fn main_loop(&mut self) {
        // run until self.is_running is false

        while self.is_running.load(Ordering::SeqCst) {
            if let Some(event) = self.swarm.next().await {
                match event {
                    // behavior for Kademlia request

                    // behavior for Kademlia responses
                    SwarmEvent::Behaviour(KademliaEvent::OutboundQueryProgressed {
                        result,
                        ..
                    }) => match result {
                        QueryResult::GetRecord(Ok(ok)) => match ok {
                            libp2p::kad::GetRecordOk::FoundRecord(peer_record) => {
                                info!("Found record: {:?}", peer_record.record);
                            }
                            _ => {}
                        },
                        QueryResult::GetRecord(Err(err)) => {
                            info!("GetRecord error: {:?}", err);
                        }
                        QueryResult::PutRecord(Ok(ok)) => {
                            info!("PutRecord success: {:?}", ok);
                        }
                        QueryResult::PutRecord(Err(err)) => {
                            info!("PutRecord error: {:?}", err);
                        }
                        _ => {}
                    },

                    // just log connection events
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => info!("ConnectionEstablished: {peer_id} | {connection_id} | {endpoint:?}"),
                    SwarmEvent::ConnectionClosed {
                        peer_id,
                        connection_id,
                        endpoint,
                        cause,
                        ..
                    } => info!(
                        "ConnectionClosed: {peer_id} | {connection_id} | {endpoint:?} | {cause:?}"
                    ),
                    SwarmEvent::Dialing {
                        peer_id,
                        connection_id,
                    } => info!("Dialing: {peer_id:?} | {connection_id}"),
                    _ => {}
                }
            } else {
                break;
            }
        }
    }

    pub async fn get(&mut self, key: Vec<u8>) {
        let key = RecordKey::new(&key);
        self.swarm.behaviour_mut().get_record(key);
    }

    pub async fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let record = Record {
            key: key.into(),
            value,
            publisher: None,
            expires: None,
        };
        self.swarm
            .behaviour_mut()
            .put_record(record, Quorum::One)
            .unwrap();
    }

    pub async fn start(&mut self) {
        self.swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();

        // if is_running is already true, return
        if self.is_running.load(Ordering::SeqCst) {
            return;
        }

        // set is_running to true to be able to start the main loop
        self.is_running.store(true, Ordering::SeqCst);

        self.main_loop().await;
    }

    pub fn stop(&mut self) {
        // set is_running to false to stop the main loop
        self.is_running.store(false, Ordering::SeqCst);
    }
}

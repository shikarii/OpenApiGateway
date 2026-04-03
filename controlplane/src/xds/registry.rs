use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{mpsc, RwLock};
use tonic::Status;

use crate::proto::envoy::service::discovery::v3::DiscoveryResponse;

/// User-facing ACK/NACK status for one connected Envoy.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct EnvoyConnectionStatus {
    pub node_id: String,
    pub last_type_url: Option<String>,
    pub last_acked_version: Option<String>,
    pub last_nonce: Option<String>,
    pub last_nack: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ConnectionHandle {
    pub sender: mpsc::Sender<Result<DiscoveryResponse, Status>>,
    pub status: Arc<RwLock<EnvoyConnectionStatus>>,
}

/// Registry of active ADS streams keyed by Envoy node id.
#[derive(Debug, Default)]
pub(crate) struct XdsRegistry {
    connections: DashMap<String, ConnectionHandle>,
}

impl XdsRegistry {
    pub fn register(
        &self,
        node_id: String,
        sender: mpsc::Sender<Result<DiscoveryResponse, Status>>,
    ) -> Arc<RwLock<EnvoyConnectionStatus>> {
        let status = Arc::new(RwLock::new(EnvoyConnectionStatus {
            node_id: node_id.clone(),
            last_type_url: None,
            last_acked_version: None,
            last_nonce: None,
            last_nack: None,
        }));
        self.connections.insert(
            node_id,
            ConnectionHandle {
                sender,
                status: Arc::clone(&status),
            },
        );
        status
    }

    pub fn remove(&self, node_id: &str) {
        self.connections.remove(node_id);
    }

    pub async fn broadcast(&self, responses: &[DiscoveryResponse]) {
        let mut disconnected = Vec::new();
        for item in self.connections.iter() {
            for response in responses {
                if item
                    .value()
                    .sender
                    .send(Ok(response.clone()))
                    .await
                    .is_err()
                {
                    disconnected.push(item.key().clone());
                    break;
                }
            }
        }
        for node_id in disconnected {
            self.connections.remove(&node_id);
        }
    }

    pub async fn statuses(&self) -> Vec<EnvoyConnectionStatus> {
        let mut items = Vec::new();
        for entry in self.connections.iter() {
            items.push(entry.value().status.read().await.clone());
        }
        items.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        items
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }
}

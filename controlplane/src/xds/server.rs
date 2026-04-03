use std::pin::Pin;
use std::sync::Arc;

use futures_util::Stream;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::observability::MetricsRegistry;
use crate::proto::envoy::service::discovery::v3::aggregated_discovery_service_server::{
    AggregatedDiscoveryService, AggregatedDiscoveryServiceServer,
};
use crate::proto::envoy::service::discovery::v3::{DiscoveryRequest, DiscoveryResponse};
use shared::config_types::GatewayConfig;

use super::protocol::{is_nack, TYPE_URLS_IN_ORDER};
use super::registry::{EnvoyConnectionStatus, XdsRegistry};
use super::snapshot::XdsSnapshot;
use super::version::VersionCounter;

type ResponseStream =
    Pin<Box<dyn Stream<Item = Result<DiscoveryResponse, Status>> + Send + 'static>>;

/// Long-lived xDS control plane state.
#[derive(Debug)]
pub(crate) struct XdsControlPlane {
    snapshot: RwLock<XdsSnapshot>,
    versions: VersionCounter,
    registry: XdsRegistry,
    metrics: Arc<MetricsRegistry>,
}

impl XdsControlPlane {
    pub fn new(
        config: &GatewayConfig,
        metrics: Arc<MetricsRegistry>,
    ) -> Result<Arc<Self>, super::resources::ResourceError> {
        let snapshot = XdsSnapshot::build(config, 1)?;
        metrics.set_xds_snapshot_version(1);
        Ok(Arc::new(Self {
            snapshot: RwLock::new(snapshot),
            versions: VersionCounter::new(1),
            registry: XdsRegistry::default(),
            metrics,
        }))
    }

    pub fn ads_service(self: Arc<Self>) -> AggregatedDiscoveryServiceServer<AdsService> {
        AggregatedDiscoveryServiceServer::new(AdsService {
            control_plane: self,
        })
    }

    pub async fn rebuild_from_config(
        &self,
        config: &GatewayConfig,
    ) -> Result<(), super::resources::ResourceError> {
        let version = self.versions.increment();
        let started = std::time::Instant::now();
        let snapshot = XdsSnapshot::build(config, version)?;
        self.metrics.set_xds_snapshot_version(version as i64);
        *self.snapshot.write().await = snapshot.clone();
        self.registry.broadcast(&snapshot.ordered_responses()).await;
        self.metrics
            .record_xds_push(started.elapsed().as_secs_f64());
        Ok(())
    }

    pub async fn statuses(&self) -> Vec<EnvoyConnectionStatus> {
        self.registry.statuses().await
    }

    pub fn connected_envoys(&self) -> i64 {
        self.registry.len() as i64
    }
}

#[derive(Debug)]
pub(crate) struct AdsService {
    control_plane: Arc<XdsControlPlane>,
}

#[tonic::async_trait]
impl AggregatedDiscoveryService for AdsService {
    type StreamAggregatedResourcesStream = ResponseStream;

    async fn stream_aggregated_resources(
        &self,
        request: Request<tonic::Streaming<DiscoveryRequest>>,
    ) -> Result<Response<Self::StreamAggregatedResourcesStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = mpsc::channel(16);
        let control_plane = Arc::clone(&self.control_plane);

        tokio::spawn(async move {
            let mut node_id = String::new();
            let mut status_handle = None;

            while let Ok(Some(message)) = inbound.message().await {
                if node_id.is_empty() {
                    node_id = message
                        .node
                        .as_ref()
                        .map(|node| node.id.clone())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "unknown".to_owned());
                    status_handle =
                        Some(control_plane.registry.register(node_id.clone(), tx.clone()));
                    control_plane
                        .metrics
                        .set_xds_connected_envoys(control_plane.connected_envoys());
                    tracing::info!(node_id = %node_id, "xDS stream connected");
                }

                if let Some(handle) = status_handle.as_ref() {
                    let mut status = handle.write().await;
                    status.last_type_url =
                        (!message.type_url.is_empty()).then_some(message.type_url.clone());
                    status.last_nonce = (!message.response_nonce.is_empty())
                        .then_some(message.response_nonce.clone());
                    if is_nack(&message) {
                        status.last_nack = message
                            .error_detail
                            .as_ref()
                            .map(|detail| detail.message.clone());
                        control_plane.metrics.record_xds_nack();
                        tracing::warn!(
                            node_id = %node_id,
                            type_url = %message.type_url,
                            error = ?status.last_nack,
                            "xDS NACK received"
                        );
                    } else if !message.version_info.is_empty() {
                        status.last_acked_version = Some(message.version_info.clone());
                        status.last_nack = None;
                        control_plane.metrics.record_xds_ack();
                    }
                }

                let snapshot = control_plane.snapshot.read().await.clone();
                let mut responses = Vec::new();
                if message.type_url.is_empty() {
                    responses = snapshot.ordered_responses();
                } else if let Some(response) = snapshot.response_for(&message.type_url) {
                    responses.push(response);
                }

                if responses.is_empty() {
                    for type_url in TYPE_URLS_IN_ORDER {
                        if let Some(response) = snapshot.response_for(type_url) {
                            responses.push(response);
                        }
                    }
                }

                for response in responses {
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            }

            if !node_id.is_empty() {
                control_plane.registry.remove(&node_id);
                control_plane
                    .metrics
                    .set_xds_connected_envoys(control_plane.connected_envoys());
                tracing::info!(node_id = %node_id, "xDS stream disconnected");
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::StreamAggregatedResourcesStream
        ))
    }
}

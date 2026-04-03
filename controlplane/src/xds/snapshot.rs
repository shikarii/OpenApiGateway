use std::collections::HashMap;

use prost_types::Any;
use shared::config_types::GatewayConfig;

use crate::proto::envoy::service::discovery::v3::DiscoveryResponse;

use super::protocol::{
    CLUSTER_TYPE_URL, ENDPOINT_TYPE_URL, LISTENER_TYPE_URL, ROUTE_TYPE_URL, TYPE_URLS_IN_ORDER,
};
use super::resources;

/// Immutable set of xDS resources for one config version.
#[derive(Debug, Clone)]
pub(crate) struct XdsSnapshot {
    pub version: u64,
    pub nonce: String,
    resources: HashMap<&'static str, Vec<Any>>,
}

impl XdsSnapshot {
    pub fn build(config: &GatewayConfig, version: u64) -> Result<Self, resources::ResourceError> {
        let mut resources_by_type = HashMap::new();
        resources_by_type.insert(CLUSTER_TYPE_URL, resources::build_clusters(config)?);
        resources_by_type.insert(ENDPOINT_TYPE_URL, resources::build_endpoints(config)?);
        resources_by_type.insert(LISTENER_TYPE_URL, vec![resources::build_listener(config)?]);
        resources_by_type.insert(ROUTE_TYPE_URL, vec![resources::build_route_config(config)?]);

        Ok(Self {
            version,
            nonce: format!("snapshot-{version}"),
            resources: resources_by_type,
        })
    }

    pub fn ordered_responses(&self) -> Vec<DiscoveryResponse> {
        TYPE_URLS_IN_ORDER
            .iter()
            .filter_map(|type_url| self.response_for(type_url))
            .collect()
    }

    pub fn response_for(&self, type_url: &str) -> Option<DiscoveryResponse> {
        self.resources
            .get(type_url)
            .map(|resources| DiscoveryResponse {
                version_info: self.version.to_string(),
                resources: resources.clone(),
                type_url: type_url.to_owned(),
                nonce: self.nonce.clone(),
            })
    }
}

/// Ordered ADS resource types for state-of-the-world pushes.
pub(crate) const TYPE_URLS_IN_ORDER: &[&str] = &[
    CLUSTER_TYPE_URL,
    ENDPOINT_TYPE_URL,
    LISTENER_TYPE_URL,
    ROUTE_TYPE_URL,
];

pub(crate) const CLUSTER_TYPE_URL: &str = "type.googleapis.com/envoy.config.cluster.v3.Cluster";
pub(crate) const ENDPOINT_TYPE_URL: &str =
    "type.googleapis.com/envoy.config.endpoint.v3.ClusterLoadAssignment";
pub(crate) const LISTENER_TYPE_URL: &str = "type.googleapis.com/envoy.config.listener.v3.Listener";
pub(crate) const ROUTE_TYPE_URL: &str =
    "type.googleapis.com/envoy.config.route.v3.RouteConfiguration";

pub(crate) fn is_nack(
    request: &crate::proto::envoy::service::discovery::v3::DiscoveryRequest,
) -> bool {
    request
        .error_detail
        .as_ref()
        .map(|detail| !detail.message.is_empty())
        .unwrap_or(false)
}

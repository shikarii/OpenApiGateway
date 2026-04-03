use serde_yaml::Value;

use super::envoy_gen::{build_socket_address, val};

/// Build the ext_authz HTTP filter referencing the gateway-manager cluster.
pub(super) fn build_ext_authz_filter() -> Value {
    let mut server_uri = serde_yaml::Mapping::new();
    server_uri.insert(val("uri"), val("http://gateway_manager_extauthz"));
    server_uri.insert(val("cluster"), val("gateway_manager_extauthz"));
    server_uri.insert(val("timeout"), val("0.250s"));

    let mut any_pattern = serde_yaml::Mapping::new();
    any_pattern.insert(val("prefix"), val(""));

    let mut allowed_headers = serde_yaml::Mapping::new();
    allowed_headers.insert(
        val("patterns"),
        Value::Sequence(vec![Value::Mapping(any_pattern.clone())]),
    );
    let mut auth_request = serde_yaml::Mapping::new();
    auth_request.insert(val("allowed_headers"), Value::Mapping(allowed_headers));

    let mut allowed_upstream = serde_yaml::Mapping::new();
    allowed_upstream.insert(
        val("patterns"),
        Value::Sequence(vec![Value::Mapping(any_pattern)]),
    );
    let mut auth_response = serde_yaml::Mapping::new();
    auth_response.insert(
        val("allowed_upstream_headers"),
        Value::Mapping(allowed_upstream),
    );

    let mut http_service = serde_yaml::Mapping::new();
    http_service.insert(val("server_uri"), Value::Mapping(server_uri));
    // No path_prefix: Envoy sends the original request path to the check service.
    http_service.insert(val("authorization_request"), Value::Mapping(auth_request));
    http_service.insert(val("authorization_response"), Value::Mapping(auth_response));

    let mut typed_config = serde_yaml::Mapping::new();
    typed_config.insert(
        val("@type"),
        val("type.googleapis.com/envoy.extensions.filters.http.ext_authz.v3.ExtAuthz"),
    );
    typed_config.insert(val("transport_api_version"), val("V3"));
    typed_config.insert(val("http_service"), Value::Mapping(http_service));
    typed_config.insert(val("failure_mode_allow"), Value::Bool(false));

    let mut filter = serde_yaml::Mapping::new();
    filter.insert(val("name"), val("envoy.filters.http.ext_authz"));
    filter.insert(val("typed_config"), Value::Mapping(typed_config));

    Value::Mapping(filter)
}

/// Build a STATIC cluster for the ext_authz service (gateway-manager).
pub(super) fn build_ext_authz_cluster(host: &str, port: u16) -> Value {
    let mut ep_inner = serde_yaml::Mapping::new();
    ep_inner.insert(val("address"), build_socket_address(host, port));
    let mut lb_ep = serde_yaml::Mapping::new();
    lb_ep.insert(val("endpoint"), Value::Mapping(ep_inner));

    let mut endpoint_group = serde_yaml::Mapping::new();
    endpoint_group.insert(
        val("lb_endpoints"),
        Value::Sequence(vec![Value::Mapping(lb_ep)]),
    );

    let mut load_assignment = serde_yaml::Mapping::new();
    load_assignment.insert(val("cluster_name"), val("gateway_manager_extauthz"));
    load_assignment.insert(
        val("endpoints"),
        Value::Sequence(vec![Value::Mapping(endpoint_group)]),
    );

    let mut cluster = serde_yaml::Mapping::new();
    cluster.insert(val("name"), val("gateway_manager_extauthz"));
    cluster.insert(val("type"), val("STATIC"));
    cluster.insert(val("connect_timeout"), val("0.250s"));
    cluster.insert(val("load_assignment"), Value::Mapping(load_assignment));

    Value::Mapping(cluster)
}

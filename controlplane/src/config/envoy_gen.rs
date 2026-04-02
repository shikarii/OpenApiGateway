use serde_yaml::Value;
use shared::config_types::{GatewayConfig, RouteConfig, ServiceConfig};

use super::envoy_ext_authz::{build_ext_authz_cluster, build_ext_authz_filter};

/// Errors from Envoy config generation.
#[derive(Debug, thiserror::Error)]
pub(crate) enum EnvoyGenError {
    #[error("invalid endpoint format '{0}': expected host:port")]
    InvalidEndpoint(String),
    #[error("YAML serialization error: {0}")]
    Serialization(#[from] serde_yaml::Error),
}

/// Generate a complete Envoy v3 static bootstrap config from a gateway config.
///
/// The output is a YAML string ready to write to `/etc/envoy/envoy.yaml`.
pub(crate) fn generate_envoy_config(cfg: &GatewayConfig) -> Result<String, EnvoyGenError> {
    let listener = build_listener(cfg)?;
    let mut clusters = build_clusters(&cfg.services)?;

    // Add ext_authz cluster if configured.
    if let Some(ref addr) = cfg.gateway.extauthz_address {
        let (host, port) = parse_endpoint(addr)?;
        clusters.push(build_ext_authz_cluster(&host, port));
    }

    let root = serde_yaml::to_value(serde_yaml::Mapping::new())?;
    let mut root = match root {
        Value::Mapping(m) => m,
        _ => serde_yaml::Mapping::new(),
    };

    // static_resources
    let mut static_resources = serde_yaml::Mapping::new();
    static_resources.insert(val("listeners"), Value::Sequence(vec![listener]));
    static_resources.insert(val("clusters"), Value::Sequence(clusters));
    root.insert(val("static_resources"), Value::Mapping(static_resources));

    // Envoy admin interface on localhost:9901
    root.insert(val("admin"), build_admin());

    serde_yaml::to_string(&Value::Mapping(root)).map_err(EnvoyGenError::from)
}

/// Build the main listener with HCM filter and route config.
fn build_listener(cfg: &GatewayConfig) -> Result<Value, EnvoyGenError> {
    let (listen_host, listen_port) = parse_endpoint(&cfg.gateway.listen_address)?;
    let virtual_hosts = build_virtual_hosts(&cfg.routes);

    let mut route_config = serde_yaml::Mapping::new();
    route_config.insert(val("name"), val("local_route"));
    route_config.insert(val("virtual_hosts"), Value::Sequence(virtual_hosts));

    let mut hcm_typed_config = serde_yaml::Mapping::new();
    hcm_typed_config.insert(
        val("@type"),
        val("type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager"),
    );
    hcm_typed_config.insert(val("stat_prefix"), val("ingress_http"));
    hcm_typed_config.insert(val("generate_request_id"), Value::Bool(true));

    // Stream idle timeout from gateway config.
    let mut common_http = serde_yaml::Mapping::new();
    common_http.insert(
        val("idle_timeout"),
        val(&duration_string(cfg.gateway.idle_timeout_ms)),
    );
    hcm_typed_config.insert(
        val("common_http_protocol_options"),
        Value::Mapping(common_http),
    );

    hcm_typed_config.insert(val("route_config"), Value::Mapping(route_config));

    // HTTP filters: ext_authz (optional) → router (terminal).
    let mut http_filters = Vec::new();
    if cfg.gateway.extauthz_address.is_some() {
        http_filters.push(build_ext_authz_filter());
    }

    let mut router_typed = serde_yaml::Mapping::new();
    router_typed.insert(
        val("@type"),
        val("type.googleapis.com/envoy.extensions.filters.http.router.v3.Router"),
    );
    let mut router_filter = serde_yaml::Mapping::new();
    router_filter.insert(val("name"), val("envoy.filters.http.router"));
    router_filter.insert(val("typed_config"), Value::Mapping(router_typed));
    http_filters.push(Value::Mapping(router_filter));

    hcm_typed_config.insert(val("http_filters"), Value::Sequence(http_filters));

    let mut hcm_filter = serde_yaml::Mapping::new();
    hcm_filter.insert(
        val("name"),
        val("envoy.filters.network.http_connection_manager"),
    );
    hcm_filter.insert(val("typed_config"), Value::Mapping(hcm_typed_config));

    let mut filter_chain = serde_yaml::Mapping::new();
    filter_chain.insert(
        val("filters"),
        Value::Sequence(vec![Value::Mapping(hcm_filter)]),
    );

    let mut listener = serde_yaml::Mapping::new();
    listener.insert(val("name"), val("main_listener"));
    listener.insert(
        val("address"),
        build_socket_address(&listen_host, listen_port),
    );
    listener.insert(
        val("filter_chains"),
        Value::Sequence(vec![Value::Mapping(filter_chain)]),
    );

    Ok(Value::Mapping(listener))
}

/// Build virtual hosts from gateway routes, merging routes with identical domains.
///
/// Envoy requires each domain to appear in at most one virtual host.  Routes
/// that share the same hostname set are grouped into a single virtual host
/// with multiple route entries.
fn build_virtual_hosts(routes: &[RouteConfig]) -> Vec<Value> {
    // Group routes by their sorted domain set so identical hosts merge into one
    // virtual host.  Envoy rejects duplicate domains across virtual hosts.
    let mut groups: Vec<(Vec<String>, String, Vec<Value>)> = Vec::new();

    for route in routes {
        let mut key: Vec<String> = route.hostnames.clone();
        key.sort();

        if let Some(group) = groups.iter_mut().find(|(k, _, _)| *k == key) {
            group.2.push(build_route_entry(route));
        } else {
            groups.push((key, route.name.clone(), vec![build_route_entry(route)]));
        }
    }

    groups
        .into_iter()
        .map(|(_, name, route_entries)| {
            let matching = routes.iter().find(|r| r.name == name).expect("route");
            let domains: Vec<Value> = matching.hostnames.iter().map(|h| val(h)).collect();

            let mut vhost = serde_yaml::Mapping::new();
            vhost.insert(val("name"), val(&name));
            vhost.insert(val("domains"), Value::Sequence(domains));
            vhost.insert(val("routes"), Value::Sequence(route_entries));
            Value::Mapping(vhost)
        })
        .collect()
}

/// Build a single route entry (match + action) from a gateway route config.
fn build_route_entry(route: &RouteConfig) -> Value {
    let mut match_rule = serde_yaml::Mapping::new();
    match_rule.insert(val("prefix"), val(&route.path_prefix));

    let mut route_action = serde_yaml::Mapping::new();
    route_action.insert(val("cluster"), val(&route.upstream.service));
    route_action.insert(
        val("timeout"),
        val(&duration_string(route.upstream.request_timeout_ms)),
    );

    if route.upstream.retries > 0 {
        let mut retry_policy = serde_yaml::Mapping::new();
        retry_policy.insert(val("retry_on"), val("5xx,connect-failure"));
        retry_policy.insert(
            val("num_retries"),
            Value::Number(serde_yaml::Number::from(route.upstream.retries as u64)),
        );
        route_action.insert(val("retry_policy"), Value::Mapping(retry_policy));
    }

    let mut entry = serde_yaml::Mapping::new();
    entry.insert(val("match"), Value::Mapping(match_rule));
    entry.insert(val("route"), Value::Mapping(route_action));
    Value::Mapping(entry)
}

/// Build clusters from gateway services.
fn build_clusters(services: &[ServiceConfig]) -> Result<Vec<Value>, EnvoyGenError> {
    services.iter().map(build_cluster).collect()
}

/// Build a single Envoy cluster from a gateway service.
fn build_cluster(service: &ServiceConfig) -> Result<Value, EnvoyGenError> {
    let mut lb_endpoints = Vec::new();
    for ep in &service.endpoints {
        let (host, port) = parse_endpoint(ep)?;
        let mut ep_inner = serde_yaml::Mapping::new();
        ep_inner.insert(val("address"), build_socket_address(&host, port));
        let mut lb_ep = serde_yaml::Mapping::new();
        lb_ep.insert(val("endpoint"), Value::Mapping(ep_inner));
        lb_endpoints.push(Value::Mapping(lb_ep));
    }

    let mut endpoint_group = serde_yaml::Mapping::new();
    endpoint_group.insert(val("lb_endpoints"), Value::Sequence(lb_endpoints));

    let mut load_assignment = serde_yaml::Mapping::new();
    load_assignment.insert(val("cluster_name"), val(&service.name));
    load_assignment.insert(
        val("endpoints"),
        Value::Sequence(vec![Value::Mapping(endpoint_group)]),
    );

    let health_check = build_health_check(service);

    let mut cluster = serde_yaml::Mapping::new();
    cluster.insert(val("name"), val(&service.name));
    cluster.insert(val("type"), val("STRICT_DNS"));
    cluster.insert(val("lb_policy"), val("ROUND_ROBIN"));
    cluster.insert(
        val("connect_timeout"),
        val(&duration_string(service.health_check.timeout_ms)),
    );
    cluster.insert(val("load_assignment"), Value::Mapping(load_assignment));
    cluster.insert(val("health_checks"), Value::Sequence(vec![health_check]));

    Ok(Value::Mapping(cluster))
}

/// Build an HTTP health check for a cluster.
fn build_health_check(service: &ServiceConfig) -> Value {
    let hc = &service.health_check;

    let mut http_hc = serde_yaml::Mapping::new();
    http_hc.insert(val("path"), val(&hc.path));

    let mut check = serde_yaml::Mapping::new();
    check.insert(val("timeout"), val(&duration_string(hc.timeout_ms)));
    check.insert(val("interval"), val(&duration_string(hc.interval_ms)));
    check.insert(
        val("unhealthy_threshold"),
        Value::Number(serde_yaml::Number::from(3u64)),
    );
    check.insert(
        val("healthy_threshold"),
        Value::Number(serde_yaml::Number::from(1u64)),
    );
    check.insert(val("http_health_check"), Value::Mapping(http_hc));

    Value::Mapping(check)
}

/// Build the Envoy admin section (localhost:9901).
fn build_admin() -> Value {
    let mut admin = serde_yaml::Mapping::new();
    admin.insert(val("address"), build_socket_address("127.0.0.1", 9901));
    Value::Mapping(admin)
}

/// Build an Envoy socket_address block.
pub(super) fn build_socket_address(host: &str, port: u16) -> Value {
    let mut sa = serde_yaml::Mapping::new();
    sa.insert(val("address"), val(host));
    sa.insert(
        val("port_value"),
        Value::Number(serde_yaml::Number::from(port as u64)),
    );

    let mut address = serde_yaml::Mapping::new();
    address.insert(val("socket_address"), Value::Mapping(sa));
    Value::Mapping(address)
}

/// Parse a "host:port" endpoint string.
fn parse_endpoint(endpoint: &str) -> Result<(String, u16), EnvoyGenError> {
    let colon_pos = endpoint
        .rfind(':')
        .ok_or_else(|| EnvoyGenError::InvalidEndpoint(endpoint.to_owned()))?;

    let host = &endpoint[..colon_pos];
    let port_str = &endpoint[colon_pos + 1..];

    if host.is_empty() {
        return Err(EnvoyGenError::InvalidEndpoint(endpoint.to_owned()));
    }

    let port: u16 = port_str
        .parse()
        .map_err(|_| EnvoyGenError::InvalidEndpoint(endpoint.to_owned()))?;

    Ok((host.to_owned(), port))
}

/// Convert milliseconds to an Envoy duration string.
///
/// Envoy expects durations like "5s", "0.500s", "0.050s".
fn duration_string(ms: u64) -> String {
    let secs = ms / 1000;
    let frac = ms % 1000;
    if frac == 0 {
        format!("{secs}s")
    } else {
        format!("{secs}.{frac:03}s")
    }
}

/// Shorthand to create a `serde_yaml::Value::String`.
pub(super) fn val(s: &str) -> Value {
    Value::String(s.to_owned())
}

#[cfg(test)]
#[path = "envoy_gen_tests.rs"]
mod tests;

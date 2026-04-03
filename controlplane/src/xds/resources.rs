use std::collections::BTreeMap;

use prost::Message;
use prost_types::{Any, Duration};
use shared::config_types::{GatewayConfig, RouteConfig, ServiceConfig};

use super::protocol::{CLUSTER_TYPE_URL, ENDPOINT_TYPE_URL, LISTENER_TYPE_URL, ROUTE_TYPE_URL};
use crate::proto::envoy::config::cluster::v3::Cluster;
use crate::proto::envoy::config::core::v3::{
    Address, HealthCheck, HttpProtocolOptions, SocketAddress,
};
use crate::proto::envoy::config::endpoint::v3::{
    ClusterLoadAssignment, Endpoint, LbEndpoint, LocalityLbEndpoints,
};
use crate::proto::envoy::config::listener::v3::{Filter, FilterChain, Listener};
use crate::proto::envoy::config::route::v3::{
    RetryPolicy, Route, RouteAction, RouteConfiguration, RouteMatch, VirtualHost,
};
use crate::proto::envoy::extensions::filters::http::ext_authz::v3::ExtAuthz;
use crate::proto::envoy::extensions::filters::http::ext_proc::v3::ExtProc;
use crate::proto::envoy::extensions::filters::http::router::v3::Router;
use crate::proto::envoy::extensions::filters::network::http_connection_manager::v3::{
    HttpConnectionManager, HttpFilter, Rds,
};

/// xDS resource build failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ResourceError {
    #[error("invalid endpoint format '{0}'")]
    InvalidEndpoint(String),
    #[error("failed to encode xDS resource: {0}")]
    Encode(String),
}

pub(crate) fn build_listener(config: &GatewayConfig) -> Result<Any, ResourceError> {
    let hcm = HttpConnectionManager {
        stat_prefix: "ingress_http".to_owned(),
        generate_request_id: true,
        common_http_protocol_options: Some(HttpProtocolOptions {
            idle_timeout: Some(duration(config.gateway.idle_timeout_ms)),
        }),
        rds: Some(Rds {
            route_config_name: "gateway-routes".to_owned(),
            config_source: "ads".to_owned(),
        }),
        http_filters: build_http_filters(config)?,
    };
    let (host, port) = parse_endpoint(&config.gateway.listen_address)?;
    let listener = Listener {
        name: "main_listener".to_owned(),
        address: Some(socket_address(&host, port)),
        filter_chains: vec![FilterChain {
            filters: vec![Filter {
                name: "envoy.filters.network.http_connection_manager".to_owned(),
                typed_config: Some(pack_any(
                    "type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager",
                    &hcm,
                )?),
            }],
        }],
    };
    pack_any(LISTENER_TYPE_URL, &listener)
}

pub(crate) fn build_route_config(config: &GatewayConfig) -> Result<Any, ResourceError> {
    let route_config = RouteConfiguration {
        name: "gateway-routes".to_owned(),
        virtual_hosts: build_virtual_hosts(&config.routes),
    };
    pack_any(ROUTE_TYPE_URL, &route_config)
}

pub(crate) fn build_clusters(config: &GatewayConfig) -> Result<Vec<Any>, ResourceError> {
    config
        .services
        .iter()
        .map(|service| {
            let assignment = build_cluster_assignment(service)?;
            let cluster = Cluster {
                name: service.name.clone(),
                r#type: "STRICT_DNS".to_owned(),
                lb_policy: "ROUND_ROBIN".to_owned(),
                connect_timeout: Some(duration(service.health_check.timeout_ms)),
                load_assignment: Some(assignment),
                health_checks: vec![HealthCheck {
                    timeout: Some(duration(service.health_check.timeout_ms)),
                    interval: Some(duration(service.health_check.interval_ms)),
                    path: service.health_check.path.clone(),
                    unhealthy_threshold: 3,
                    healthy_threshold: 1,
                }],
            };
            pack_any(CLUSTER_TYPE_URL, &cluster)
        })
        .collect()
}

pub(crate) fn build_endpoints(config: &GatewayConfig) -> Result<Vec<Any>, ResourceError> {
    config
        .services
        .iter()
        .map(|service| {
            let assignment = build_cluster_assignment(service)?;
            pack_any(ENDPOINT_TYPE_URL, &assignment)
        })
        .collect()
}

fn build_http_filters(config: &GatewayConfig) -> Result<Vec<HttpFilter>, ResourceError> {
    let mut filters = Vec::new();
    if let Some(ext_authz) = config.gateway.extauthz_address.as_ref() {
        filters.push(HttpFilter {
            name: "envoy.filters.http.ext_authz".to_owned(),
            typed_config: Some(pack_any(
                "type.googleapis.com/envoy.extensions.filters.http.ext_authz.v3.ExtAuthz",
                &ExtAuthz {
                    grpc_service: ext_authz.clone(),
                },
            )?),
        });
    }
    if config.ext_proc.enabled {
        filters.push(HttpFilter {
            name: "envoy.filters.http.ext_proc".to_owned(),
            typed_config: Some(pack_any(
                "type.googleapis.com/envoy.extensions.filters.http.ext_proc.v3.ExtProc",
                &ExtProc {
                    grpc_service: config.ext_proc.listen_address.clone(),
                },
            )?),
        });
    }
    filters.push(HttpFilter {
        name: "envoy.filters.http.router".to_owned(),
        typed_config: Some(pack_any(
            "type.googleapis.com/envoy.extensions.filters.http.router.v3.Router",
            &Router {},
        )?),
    });
    Ok(filters)
}

fn build_virtual_hosts(routes: &[RouteConfig]) -> Vec<VirtualHost> {
    let mut groups: BTreeMap<Vec<String>, Vec<Route>> = BTreeMap::new();
    let mut names: BTreeMap<Vec<String>, String> = BTreeMap::new();

    for route in routes {
        let mut key = route.hostnames.clone();
        key.sort();
        groups.entry(key.clone()).or_default().push(Route {
            r#match: Some(RouteMatch {
                prefix: route.path_prefix.clone(),
            }),
            route: Some(RouteAction {
                cluster: route.upstream.service.clone(),
                timeout: Some(duration(route.upstream.request_timeout_ms)),
                retry_policy: (route.upstream.retries > 0).then_some(RetryPolicy {
                    retry_on: "5xx,connect-failure".to_owned(),
                    num_retries: route.upstream.retries,
                }),
            }),
        });
        names.entry(key).or_insert_with(|| route.name.clone());
    }

    groups
        .into_iter()
        .map(|(domains, routes)| VirtualHost {
            name: names
                .get(&domains)
                .cloned()
                .unwrap_or_else(|| "vhost".to_owned()),
            domains,
            routes,
        })
        .collect()
}

fn build_cluster_assignment(
    service: &ServiceConfig,
) -> Result<ClusterLoadAssignment, ResourceError> {
    let endpoints = service
        .endpoints
        .iter()
        .map(|endpoint| {
            let (host, port) = parse_endpoint(endpoint)?;
            Ok(LbEndpoint {
                endpoint: Some(Endpoint {
                    address: Some(socket_address(&host, port)),
                }),
            })
        })
        .collect::<Result<Vec<_>, ResourceError>>()?;

    Ok(ClusterLoadAssignment {
        cluster_name: service.name.clone(),
        endpoints: vec![LocalityLbEndpoints {
            lb_endpoints: endpoints,
        }],
    })
}

fn socket_address(host: &str, port: u16) -> Address {
    Address {
        socket_address: Some(SocketAddress {
            address: host.to_owned(),
            port_value: u32::from(port),
        }),
    }
}

fn duration(ms: u64) -> Duration {
    Duration {
        seconds: (ms / 1000) as i64,
        nanos: ((ms % 1000) * 1_000_000) as i32,
    }
}

fn parse_endpoint(endpoint: &str) -> Result<(String, u16), ResourceError> {
    let Some(idx) = endpoint.rfind(':') else {
        return Err(ResourceError::InvalidEndpoint(endpoint.to_owned()));
    };
    let host = &endpoint[..idx];
    let port = endpoint[idx + 1..]
        .parse::<u16>()
        .map_err(|_| ResourceError::InvalidEndpoint(endpoint.to_owned()))?;
    if host.is_empty() {
        return Err(ResourceError::InvalidEndpoint(endpoint.to_owned()));
    }
    Ok((host.to_owned(), port))
}

fn pack_any<T: Message>(type_url: &str, message: &T) -> Result<Any, ResourceError> {
    let mut value = Vec::new();
    message
        .encode(&mut value)
        .map_err(|e| ResourceError::Encode(e.to_string()))?;
    Ok(Any {
        type_url: type_url.to_owned(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use crate::config::load_config_from_str;

    use super::*;

    #[test]
    fn snapshot_builders_encode_expected_resources() {
        let cfg = load_config_from_str(include_str!(
            "../../../examples/configs/gateway-single-node.yaml"
        ))
        .unwrap();
        let listener = build_listener(&cfg).unwrap();
        let route = build_route_config(&cfg).unwrap();
        let clusters = build_clusters(&cfg).unwrap();
        let endpoints = build_endpoints(&cfg).unwrap();

        assert_eq!(listener.type_url, LISTENER_TYPE_URL);
        assert_eq!(route.type_url, ROUTE_TYPE_URL);
        assert_eq!(clusters.len(), 1);
        assert_eq!(endpoints.len(), 1);
    }
}

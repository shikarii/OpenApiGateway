use serde_yaml::Value;

use super::*;

fn sample_config() -> GatewayConfig {
    let yaml = include_str!("../../../examples/configs/gateway-single-node.yaml");
    serde_yaml::from_str(yaml).unwrap()
}

fn generate_and_parse(cfg: &GatewayConfig) -> Value {
    let yaml_str = generate_envoy_config(cfg).unwrap();
    serde_yaml::from_str(&yaml_str).unwrap()
}

#[test]
fn generate_from_sample_config() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    // Top-level keys exist.
    assert!(envoy["static_resources"].is_mapping());
    assert!(envoy["admin"].is_mapping());

    // Listeners and clusters present.
    let listeners = &envoy["static_resources"]["listeners"];
    assert!(listeners.is_sequence());
    assert_eq!(listeners.as_sequence().unwrap().len(), 1);

    let clusters = &envoy["static_resources"]["clusters"];
    assert!(clusters.is_sequence());
    assert_eq!(clusters.as_sequence().unwrap().len(), cfg.services.len());
}

#[test]
fn listener_address_from_config() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let addr = &envoy["static_resources"]["listeners"][0]["address"]["socket_address"];
    assert_eq!(addr["address"].as_str().unwrap(), "0.0.0.0");
    assert_eq!(addr["port_value"].as_u64().unwrap(), 8080);
}

#[test]
fn virtual_hosts_from_routes() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let hcm = &envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]
        ["typed_config"];
    let vhosts = hcm["route_config"]["virtual_hosts"].as_sequence().unwrap();

    // Both routes share the same hostnames, so they merge into one virtual host.
    assert_eq!(vhosts.len(), 1);
    let vh0 = &vhosts[0];
    assert_eq!(vh0["name"].as_str().unwrap(), "public-echo");
    let domains: Vec<&str> = vh0["domains"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(domains.contains(&"localhost"));
    assert!(domains.contains(&"127.0.0.1"));

    // Both route entries present in the merged virtual host.
    let routes = vh0["routes"].as_sequence().unwrap();
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0]["match"]["prefix"].as_str().unwrap(), "/public");
    assert_eq!(routes[0]["route"]["cluster"].as_str().unwrap(), "backend");
    assert_eq!(routes[1]["match"]["prefix"].as_str().unwrap(), "/private");
    assert_eq!(routes[1]["route"]["cluster"].as_str().unwrap(), "backend");
}

#[test]
fn clusters_from_services() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let clusters = envoy["static_resources"]["clusters"].as_sequence().unwrap();
    let cluster = &clusters[0];
    assert_eq!(cluster["name"].as_str().unwrap(), "backend");
    assert_eq!(cluster["type"].as_str().unwrap(), "STRICT_DNS");
    assert_eq!(cluster["lb_policy"].as_str().unwrap(), "ROUND_ROBIN");

    let ep = &cluster["load_assignment"]["endpoints"][0]["lb_endpoints"][0];
    let sa = &ep["endpoint"]["address"]["socket_address"];
    assert_eq!(sa["address"].as_str().unwrap(), "echo-backend");
    assert_eq!(sa["port_value"].as_u64().unwrap(), 8081);
}

#[test]
fn health_checks_configured() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let hc = &envoy["static_resources"]["clusters"][0]["health_checks"][0];
    assert_eq!(
        hc["http_health_check"]["path"].as_str().unwrap(),
        "/healthz"
    );
    assert_eq!(hc["interval"].as_str().unwrap(), "2s");
    assert_eq!(hc["timeout"].as_str().unwrap(), "0.500s");
}

#[test]
fn timeouts_set() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    // Route timeout
    let hcm = &envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]
        ["typed_config"];
    let route = &hcm["route_config"]["virtual_hosts"][0]["routes"][0];
    assert_eq!(route["route"]["timeout"].as_str().unwrap(), "5s");

    // Idle timeout via common_http_protocol_options
    let idle = &hcm["common_http_protocol_options"]["idle_timeout"];
    assert_eq!(idle.as_str().unwrap(), "60s");
}

#[test]
fn request_id_generation_enabled() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let hcm = &envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]
        ["typed_config"];
    assert!(hcm["generate_request_id"].as_bool().unwrap());
}

#[test]
fn retry_policy_when_retries_nonzero() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    // Sample config has retries: 1 on both routes.
    let hcm = &envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]
        ["typed_config"];
    let route = &hcm["route_config"]["virtual_hosts"][0]["routes"][0];
    let retry = &route["route"]["retry_policy"];
    assert_eq!(retry["num_retries"].as_u64().unwrap(), 1);
    assert_eq!(retry["retry_on"].as_str().unwrap(), "5xx,connect-failure");
}

#[test]
fn no_retry_policy_when_retries_zero() {
    let mut cfg = sample_config();
    cfg.routes[0].upstream.retries = 0;
    let envoy = generate_and_parse(&cfg);

    let hcm = &envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]
        ["typed_config"];
    let route = &hcm["route_config"]["virtual_hosts"][0]["routes"][0];
    assert!(route["route"]["retry_policy"].is_null());
}

#[test]
fn parse_endpoint_valid() {
    let (host, port) = parse_endpoint("backend:8080").unwrap();
    assert_eq!(host, "backend");
    assert_eq!(port, 8080);

    let (host, port) = parse_endpoint("192.168.1.1:443").unwrap();
    assert_eq!(host, "192.168.1.1");
    assert_eq!(port, 443);
}

#[test]
fn parse_endpoint_invalid() {
    assert!(parse_endpoint("no-port").is_err());
    assert!(parse_endpoint(":8080").is_err());
    assert!(parse_endpoint("host:abc").is_err());
    assert!(parse_endpoint("host:99999").is_err());
}

#[test]
fn duration_string_formatting() {
    assert_eq!(duration_string(5000), "5s");
    assert_eq!(duration_string(500), "0.500s");
    assert_eq!(duration_string(50), "0.050s");
    assert_eq!(duration_string(0), "0s");
    assert_eq!(duration_string(1), "0.001s");
    assert_eq!(duration_string(15000), "15s");
    assert_eq!(duration_string(2500), "2.500s");
}

#[test]
fn admin_address_is_localhost() {
    let cfg = sample_config();
    let envoy = generate_and_parse(&cfg);

    let admin_addr = &envoy["admin"]["address"]["socket_address"];
    assert_eq!(admin_addr["address"].as_str().unwrap(), "127.0.0.1");
    assert_eq!(admin_addr["port_value"].as_u64().unwrap(), 9901);
}

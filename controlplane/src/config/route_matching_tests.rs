use serde_yaml::Value;
use shared::config_types::*;

use super::envoy_gen::generate_envoy_config;

/// Build a minimal valid config with custom routes and services.
fn config_with_routes(routes: Vec<RouteConfig>, services: Vec<ServiceConfig>) -> GatewayConfig {
    GatewayConfig {
        version: 1,
        gateway: GatewayServer {
            listen_address: "0.0.0.0:8080".into(),
            admin_address: "0.0.0.0:9090".into(),
            request_timeout_ms: 15000,
            idle_timeout_ms: 60000,
            max_request_body_bytes: 10485760,
            trust_forwarded_headers: false,
        },
        auth: AuthConfig { providers: vec![] },
        rate_limits: RateLimitsConfig {
            redis_address: "localhost:6379".into(),
            redis_db: 0,
            redis_key_prefix: "rl".into(),
            default_timeout_ms: 100,
            fail_open: true,
            survivability_mode: SurvivabilityMode {
                enabled: false,
                fallback_capacity: 10,
                fallback_refill_rate_per_sec: 1.0,
            },
        },
        routes,
        services,
        observability: ObservabilityConfig {
            access_log_json: true,
            prometheus_enabled: true,
            tracing: TracingConfig {
                enabled: false,
                otlp_endpoint: String::new(),
                sample_rate: 0.0,
            },
        },
    }
}

fn make_route(name: &str, hostnames: Vec<&str>, path: &str, service: &str) -> RouteConfig {
    RouteConfig {
        name: name.into(),
        hostnames: hostnames.into_iter().map(String::from).collect(),
        path_prefix: path.into(),
        methods: vec!["GET".into()],
        auth_required: false,
        auth_provider: None,
        required_scopes: None,
        rate_limit: RouteRateLimit {
            bucket_capacity: 100,
            refill_rate_per_sec: 10.0,
            key_by: "ip".into(),
        },
        upstream: UpstreamConfig {
            service: service.into(),
            request_timeout_ms: 5000,
            retries: 0,
        },
    }
}

fn make_service(name: &str) -> ServiceConfig {
    ServiceConfig {
        name: name.into(),
        endpoints: vec!["backend:8080".into()],
        health_check: HealthCheckConfig {
            path: "/healthz".into(),
            interval_ms: 5000,
            timeout_ms: 500,
        },
    }
}

fn get_virtual_hosts(envoy: &Value) -> &Vec<Value> {
    envoy["static_resources"]["listeners"][0]["filter_chains"][0]["filters"][0]["typed_config"]
        ["route_config"]["virtual_hosts"]
        .as_sequence()
        .expect("virtual_hosts should be a sequence")
}

// --- Virtual host grouping tests ---

#[test]
fn same_hostname_routes_merge_into_one_vhost() {
    let routes = vec![
        make_route("route-a", vec!["api.example.com"], "/v1", "svc-a"),
        make_route("route-b", vec!["api.example.com"], "/v2", "svc-b"),
    ];
    let services = vec![make_service("svc-a"), make_service("svc-b")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    assert_eq!(vhosts.len(), 1, "same-hostname routes should merge");

    let route_entries = vhosts[0]["routes"].as_sequence().unwrap();
    assert_eq!(route_entries.len(), 2, "both routes in the merged vhost");
}

#[test]
fn different_hostnames_produce_separate_vhosts() {
    let routes = vec![
        make_route("route-a", vec!["api.example.com"], "/v1", "svc-a"),
        make_route("route-b", vec!["admin.example.com"], "/v1", "svc-a"),
    ];
    let services = vec![make_service("svc-a")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    assert_eq!(vhosts.len(), 2, "different hostnames get separate vhosts");
}

#[test]
fn wildcard_hostname_produces_single_vhost() {
    let routes = vec![
        make_route("public", vec!["*"], "/public", "svc-a"),
        make_route("private", vec!["*"], "/private", "svc-a"),
    ];
    let services = vec![make_service("svc-a")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    assert_eq!(vhosts.len(), 1);

    let domains: Vec<&str> = vhosts[0]["domains"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(domains, vec!["*"]);
}

#[test]
fn multi_hostname_routes_grouped_by_sorted_set() {
    // Same hostnames in different order should still merge.
    let routes = vec![
        make_route("route-a", vec!["b.com", "a.com"], "/v1", "svc-a"),
        make_route("route-b", vec!["a.com", "b.com"], "/v2", "svc-a"),
    ];
    let services = vec![make_service("svc-a")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    assert_eq!(
        vhosts.len(),
        1,
        "same hostname set in different order merges"
    );

    let route_entries = vhosts[0]["routes"].as_sequence().unwrap();
    assert_eq!(route_entries.len(), 2);
}

// --- Path prefix matching tests ---

#[test]
fn path_prefix_set_correctly_on_route_entries() {
    let routes = vec![
        make_route("short", vec!["*"], "/api", "svc-a"),
        make_route("long", vec!["*"], "/api/v2/users", "svc-a"),
    ];
    let services = vec![make_service("svc-a")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    let route_entries = vhosts[0]["routes"].as_sequence().unwrap();

    let prefixes: Vec<&str> = route_entries
        .iter()
        .map(|r| r["match"]["prefix"].as_str().unwrap())
        .collect();
    assert!(prefixes.contains(&"/api"));
    assert!(prefixes.contains(&"/api/v2/users"));
}

#[test]
fn route_maps_to_correct_cluster() {
    let routes = vec![
        make_route("users", vec!["*"], "/users", "user-svc"),
        make_route("orders", vec!["*"], "/orders", "order-svc"),
    ];
    let services = vec![make_service("user-svc"), make_service("order-svc")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    let route_entries = vhosts[0]["routes"].as_sequence().unwrap();

    let users_route = route_entries
        .iter()
        .find(|r| r["match"]["prefix"].as_str().unwrap() == "/users")
        .expect("/users route");
    assert_eq!(
        users_route["route"]["cluster"].as_str().unwrap(),
        "user-svc"
    );

    let orders_route = route_entries
        .iter()
        .find(|r| r["match"]["prefix"].as_str().unwrap() == "/orders")
        .expect("/orders route");
    assert_eq!(
        orders_route["route"]["cluster"].as_str().unwrap(),
        "order-svc"
    );
}

// --- Cluster generation tests ---

#[test]
fn each_service_produces_one_cluster() {
    let routes = vec![make_route("r1", vec!["*"], "/", "svc-a")];
    let services = vec![make_service("svc-a"), make_service("svc-b")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let clusters = envoy["static_resources"]["clusters"].as_sequence().unwrap();
    assert_eq!(clusters.len(), 2);

    let names: Vec<&str> = clusters
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"svc-a"));
    assert!(names.contains(&"svc-b"));
}

#[test]
fn multiple_endpoints_per_cluster() {
    let routes = vec![make_route("r1", vec!["*"], "/", "multi-ep")];
    let mut svc = make_service("multi-ep");
    svc.endpoints = vec![
        "host-a:8080".into(),
        "host-b:8080".into(),
        "host-c:8080".into(),
    ];
    let cfg = config_with_routes(routes, vec![svc]);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let cluster = &envoy["static_resources"]["clusters"][0];
    let lb_eps = cluster["load_assignment"]["endpoints"][0]["lb_endpoints"]
        .as_sequence()
        .unwrap();
    assert_eq!(lb_eps.len(), 3);
}

// --- Three-vhost scenario (mixed hostnames) ---

#[test]
fn mixed_hostnames_produce_correct_vhost_count() {
    let routes = vec![
        make_route("api-v1", vec!["api.example.com"], "/v1", "svc-a"),
        make_route("api-v2", vec!["api.example.com"], "/v2", "svc-a"),
        make_route("admin", vec!["admin.example.com"], "/admin", "svc-a"),
        make_route("public", vec!["*"], "/", "svc-a"),
    ];
    let services = vec![make_service("svc-a")];
    let cfg = config_with_routes(routes, services);
    let envoy: Value = serde_yaml::from_str(&generate_envoy_config(&cfg).unwrap()).unwrap();

    let vhosts = get_virtual_hosts(&envoy);
    assert_eq!(vhosts.len(), 3, "api.*, admin.*, and * are three vhosts");

    // api.example.com vhost has 2 routes.
    let api_vhost = vhosts
        .iter()
        .find(|v| {
            v["domains"]
                .as_sequence()
                .unwrap()
                .iter()
                .any(|d| d.as_str().unwrap() == "api.example.com")
        })
        .expect("api vhost");
    assert_eq!(api_vhost["routes"].as_sequence().unwrap().len(), 2);
}

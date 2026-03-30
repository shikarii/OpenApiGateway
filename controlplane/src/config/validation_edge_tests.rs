use shared::config_types::*;

use super::validate;

fn valid_config() -> GatewayConfig {
    GatewayConfig {
        version: 1,
        gateway: GatewayServer {
            listen_address: "0.0.0.0:8080".into(),
            admin_address: "0.0.0.0:9090".into(),
            request_timeout_ms: 15000,
            idle_timeout_ms: 60000,
            max_request_body_bytes: 10_485_760,
            trust_forwarded_headers: false,
        },
        auth: AuthConfig {
            providers: vec![AuthProvider {
                name: "main".into(),
                issuer: "https://auth.example.local/".into(),
                audience: "api-gateway".into(),
                jwks_uri: "http://localhost:7001/.well-known/jwks.json".into(),
                cache_ttl_seconds: 300,
                clock_skew_seconds: 30,
            }],
        },
        rate_limits: RateLimitsConfig {
            redis_address: "redis:6379".into(),
            redis_db: 0,
            redis_key_prefix: "rl".into(),
            default_timeout_ms: 50,
            fail_open: false,
            survivability_mode: SurvivabilityMode {
                enabled: true,
                fallback_capacity: 20,
                fallback_refill_rate_per_sec: 5.0,
            },
        },
        routes: vec![RouteConfig {
            name: "public-api".into(),
            hostnames: vec!["api.example.com".into()],
            path_prefix: "/public".into(),
            methods: vec!["GET".into(), "POST".into()],
            auth_required: false,
            auth_provider: None,
            required_scopes: None,
            rate_limit: RouteRateLimit {
                bucket_capacity: 100,
                refill_rate_per_sec: 10.0,
                key_by: "ip".into(),
            },
            upstream: UpstreamConfig {
                service: "backend".into(),
                request_timeout_ms: 5000,
                retries: 1,
            },
        }],
        services: vec![ServiceConfig {
            name: "backend".into(),
            endpoints: vec!["backend-01:8080".into()],
            health_check: HealthCheckConfig {
                path: "/healthz".into(),
                interval_ms: 2000,
                timeout_ms: 500,
            },
        }],
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

#[test]
fn valid_full_config_all_optional_fields() {
    let mut cfg = valid_config();
    // Add a second auth provider.
    cfg.auth.providers.push(AuthProvider {
        name: "secondary".into(),
        issuer: "https://secondary.local/".into(),
        audience: "api-gateway".into(),
        jwks_uri: "http://localhost:7002/.well-known/jwks.json".into(),
        cache_ttl_seconds: 600,
        clock_skew_seconds: 10,
    });
    // Add a second service.
    cfg.services.push(ServiceConfig {
        name: "internal".into(),
        endpoints: vec!["internal-01:9090".into(), "internal-02:9090".into()],
        health_check: HealthCheckConfig {
            path: "/health".into(),
            interval_ms: 5000,
            timeout_ms: 1000,
        },
    });
    // Add auth-required route with scopes.
    cfg.routes.push(RouteConfig {
        name: "protected-api".into(),
        hostnames: vec!["api.example.com".into()],
        path_prefix: "/protected".into(),
        methods: vec!["GET".into(), "POST".into(), "PUT".into(), "DELETE".into()],
        auth_required: true,
        auth_provider: Some("main".into()),
        required_scopes: Some(vec!["api.read".into(), "api.write".into()]),
        rate_limit: RouteRateLimit {
            bucket_capacity: 50,
            refill_rate_per_sec: 5.0,
            key_by: "sub".into(),
        },
        upstream: UpstreamConfig {
            service: "internal".into(),
            request_timeout_ms: 3000,
            retries: 2,
        },
    });
    assert!(validate(&cfg).is_ok());
}

#[test]
fn load_example_yaml_file() {
    let yaml = include_str!("../../../examples/configs/gateway-single-node.yaml");
    let result = crate::config::load_config_from_str(yaml);
    assert!(result.is_ok(), "example config should be valid: {result:?}");
}

#[test]
fn empty_route_name_rejected() {
    let mut cfg = valid_config();
    cfg.routes[0].name = String::new();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("name") && e.to_string().contains("must not be empty")));
}

#[test]
fn empty_hostnames_rejected() {
    let mut cfg = valid_config();
    cfg.routes[0].hostnames = vec![];
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("hostnames")));
}

#[test]
fn empty_methods_rejected() {
    let mut cfg = valid_config();
    cfg.routes[0].methods = vec![];
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("methods")));
}

#[test]
fn empty_service_name_rejected() {
    let mut cfg = valid_config();
    cfg.services[0].name = String::new();
    // Also fix route reference so we only test the name check.
    cfg.routes[0].upstream.service = String::new();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("name") && e.to_string().contains("must not be empty")));
}

#[test]
fn empty_endpoints_rejected() {
    let mut cfg = valid_config();
    cfg.services[0].endpoints = vec![];
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("endpoints")));
}

#[test]
fn empty_provider_name_rejected() {
    let mut cfg = valid_config();
    cfg.auth.providers[0].name = String::new();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("name") && e.to_string().contains("must not be empty")));
}

#[test]
fn invalid_service_endpoint_format() {
    let mut cfg = valid_config();
    cfg.services[0].endpoints = vec!["no-port-here".into()];
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("host:port")));
}

#[test]
fn invalid_listen_address_format() {
    let mut cfg = valid_config();
    cfg.gateway.listen_address = "bad-address".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("listen_address")));
}

#[test]
fn invalid_admin_address_format() {
    let mut cfg = valid_config();
    cfg.gateway.admin_address = "bad-address".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("admin_address")));
}

#[test]
fn negative_refill_rate_rejected() {
    let mut cfg = valid_config();
    cfg.routes[0].rate_limit.refill_rate_per_sec = -1.0;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("refill_rate_per_sec")));
}

#[test]
fn sample_rate_zero_passes() {
    let mut cfg = valid_config();
    cfg.observability.tracing.sample_rate = 0.0;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn sample_rate_one_passes() {
    let mut cfg = valid_config();
    cfg.observability.tracing.sample_rate = 1.0;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn sample_rate_negative_rejected() {
    let mut cfg = valid_config();
    cfg.observability.tracing.sample_rate = -0.1;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("sample_rate")));
}

#[test]
fn all_valid_http_methods_accepted() {
    let mut cfg = valid_config();
    cfg.routes[0].methods = vec![
        "GET".into(),
        "POST".into(),
        "PUT".into(),
        "DELETE".into(),
        "PATCH".into(),
        "HEAD".into(),
    ];
    assert!(validate(&cfg).is_ok());
}

#[test]
fn valid_auth_route_with_scopes() {
    let mut cfg = valid_config();
    cfg.routes[0].auth_required = true;
    cfg.routes[0].auth_provider = Some("main".into());
    cfg.routes[0].required_scopes = Some(vec!["api.read".into()]);
    assert!(validate(&cfg).is_ok());
}

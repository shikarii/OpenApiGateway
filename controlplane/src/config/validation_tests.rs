use shared::config_error::ConfigError;
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
            extauthz_address: None,
            max_concurrent_requests: None,
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
fn valid_config_passes() {
    assert!(validate(&valid_config()).is_ok());
}

#[test]
fn wrong_version() {
    let mut cfg = valid_config();
    cfg.version = 2;
    let err = validate(&cfg).unwrap_err();
    assert_eq!(err.len(), 1);
    assert!(err.errors()[0].to_string().contains("version must be 1"));
}

#[test]
fn duplicate_route_names() {
    let mut cfg = valid_config();
    let dup = cfg.routes[0].clone();
    cfg.routes.push(dup);
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| { matches!(e, ConfigError::DuplicateName { kind: "route", .. }) }));
}

#[test]
fn duplicate_service_names() {
    let mut cfg = valid_config();
    let dup = cfg.services[0].clone();
    cfg.services.push(dup);
    let err = validate(&cfg).unwrap_err();
    assert!(err.errors().iter().any(|e| {
        matches!(
            e,
            ConfigError::DuplicateName {
                kind: "service",
                ..
            }
        )
    }));
}

#[test]
fn duplicate_auth_provider_names() {
    let mut cfg = valid_config();
    let dup = cfg.auth.providers[0].clone();
    cfg.auth.providers.push(dup);
    let err = validate(&cfg).unwrap_err();
    assert!(err.errors().iter().any(|e| {
        matches!(
            e,
            ConfigError::DuplicateName {
                kind: "auth provider",
                ..
            }
        )
    }));
}

#[test]
fn unknown_upstream_service() {
    let mut cfg = valid_config();
    cfg.routes[0].upstream.service = "nonexistent".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err.errors().iter().any(|e| {
        matches!(
            e,
            ConfigError::UnknownReference {
                target_kind: "service",
                ..
            }
        )
    }));
}

#[test]
fn unknown_auth_provider_ref() {
    let mut cfg = valid_config();
    cfg.routes[0].auth_required = true;
    cfg.routes[0].auth_provider = Some("nonexistent".into());
    let err = validate(&cfg).unwrap_err();
    assert!(err.errors().iter().any(|e| {
        matches!(
            e,
            ConfigError::UnknownReference {
                target_kind: "auth provider",
                ..
            }
        )
    }));
}

#[test]
fn invalid_path_prefix() {
    let mut cfg = valid_config();
    cfg.routes[0].path_prefix = "no-slash".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("path_prefix")));
}

#[test]
fn invalid_http_method() {
    let mut cfg = valid_config();
    cfg.routes[0].methods = vec!["BOGUS".into()];
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("invalid HTTP method")));
}

#[test]
fn bucket_capacity_zero() {
    let mut cfg = valid_config();
    cfg.routes[0].rate_limit.bucket_capacity = 0;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("bucket_capacity")));
}

#[test]
fn refill_rate_zero() {
    let mut cfg = valid_config();
    cfg.routes[0].rate_limit.refill_rate_per_sec = 0.0;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("refill_rate_per_sec")));
}

#[test]
fn invalid_key_by() {
    let mut cfg = valid_config();
    cfg.routes[0].rate_limit.key_by = "cookie".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("key_by")));
}

#[test]
fn invalid_jwks_uri() {
    let mut cfg = valid_config();
    cfg.auth.providers[0].jwks_uri = "not-a-url".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("jwks_uri")));
}

#[test]
fn cache_ttl_zero() {
    let mut cfg = valid_config();
    cfg.auth.providers[0].cache_ttl_seconds = 0;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("cache_ttl_seconds")));
}

#[test]
fn invalid_redis_address() {
    let mut cfg = valid_config();
    cfg.rate_limits.redis_address = "no-port".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("redis_address")));
}

#[test]
fn sample_rate_out_of_range() {
    let mut cfg = valid_config();
    cfg.observability.tracing.sample_rate = 1.5;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("sample_rate")));
}

#[test]
fn auth_required_without_provider() {
    let mut cfg = valid_config();
    cfg.routes[0].auth_required = true;
    cfg.routes[0].auth_provider = None;
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| { matches!(e, ConfigError::MissingConditional { .. }) }));
}

#[test]
fn empty_required_scopes() {
    let mut cfg = valid_config();
    cfg.routes[0].required_scopes = Some(vec![]);
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("required_scopes")));
}

#[test]
fn tracing_enabled_without_endpoint() {
    let mut cfg = valid_config();
    cfg.observability.tracing.enabled = true;
    cfg.observability.tracing.otlp_endpoint = String::new();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("otlp_endpoint")));
}

#[test]
fn multiple_errors_accumulated() {
    let mut cfg = valid_config();
    cfg.version = 99;
    cfg.routes[0].path_prefix = "bad".into();
    cfg.routes[0].rate_limit.bucket_capacity = 0;
    let err = validate(&cfg).unwrap_err();
    assert!(err.len() >= 3);
}

#[test]
fn invalid_health_check_path() {
    let mut cfg = valid_config();
    cfg.services[0].health_check.path = "no-slash".into();
    let err = validate(&cfg).unwrap_err();
    assert!(err
        .errors()
        .iter()
        .any(|e| e.to_string().contains("health_check.path")));
}

use std::path::Path;

use shared::config_error::{ConfigError, ConfigErrors};
use shared::config_types::GatewayConfig;

use super::validation;

/// Load and validate a gateway config from a YAML file.
pub fn load_config(path: &Path) -> Result<GatewayConfig, ConfigErrors> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        ConfigErrors::new(vec![ConfigError::YamlParse(format!(
            "failed to read {}: {e}",
            path.display()
        ))])
    })?;

    load_config_from_str(&contents)
}

/// Parse and validate a gateway config from a YAML string.
pub fn load_config_from_str(yaml: &str) -> Result<GatewayConfig, ConfigErrors> {
    let cfg: GatewayConfig = serde_yaml::from_str(yaml)
        .map_err(|e| ConfigErrors::new(vec![ConfigError::YamlParse(e.to_string())]))?;

    validation::validate(&cfg)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_YAML: &str = r#"
version: 1
gateway:
  listen_address: "0.0.0.0:8080"
  admin_address: "0.0.0.0:9090"
  request_timeout_ms: 15000
  idle_timeout_ms: 60000
  max_request_body_bytes: 10485760
  trust_forwarded_headers: false
auth:
  providers:
    - name: "main"
      issuer: "https://auth.example.local/"
      audience: "api-gateway"
      jwks_uri: "http://localhost:7001/.well-known/jwks.json"
      cache_ttl_seconds: 300
      clock_skew_seconds: 30
rate_limits:
  redis_address: "redis:6379"
  redis_db: 0
  redis_key_prefix: "rl"
  default_timeout_ms: 50
  fail_open: false
  survivability_mode:
    enabled: true
    fallback_capacity: 20
    fallback_refill_rate_per_sec: 5
routes:
  - name: "public-api"
    hostnames: ["api.example.com"]
    path_prefix: "/public"
    methods: ["GET", "POST"]
    auth_required: false
    rate_limit:
      bucket_capacity: 100
      refill_rate_per_sec: 10
      key_by: "ip"
    upstream:
      service: "backend"
      request_timeout_ms: 5000
      retries: 1
services:
  - name: "backend"
    endpoints:
      - "backend-01:8080"
    health_check:
      path: "/healthz"
      interval_ms: 2000
      timeout_ms: 500
observability:
  access_log_json: true
  prometheus_enabled: true
  tracing:
    enabled: false
    otlp_endpoint: ""
    sample_rate: 0.0
"#;

    #[test]
    fn load_valid_yaml_string() {
        let cfg = load_config_from_str(VALID_YAML).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.routes.len(), 1);
    }

    #[test]
    fn reject_malformed_yaml() {
        let err = load_config_from_str("not: [valid: yaml: {{").unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err.errors()[0].to_string().contains("YAML parse error"));
    }

    #[test]
    fn reject_unknown_field_via_loader() {
        let yaml = VALID_YAML.replace(
            "trust_forwarded_headers: false",
            "trust_forwarded_headers: false\n  bogus: true",
        );
        let err = load_config_from_str(&yaml).unwrap_err();
        assert_eq!(err.len(), 1);
    }

    #[test]
    fn load_missing_file() {
        let err = load_config(Path::new("/nonexistent/gateway.yaml")).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err.errors()[0].to_string().contains("failed to read"));
    }
}

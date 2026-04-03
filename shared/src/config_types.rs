use serde::Deserialize;

/// Root configuration for the API gateway.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    pub version: u32,
    pub gateway: GatewayServer,
    pub auth: AuthConfig,
    pub rate_limits: RateLimitsConfig,
    pub routes: Vec<RouteConfig>,
    pub services: Vec<ServiceConfig>,
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub xds: XdsConfig,
    #[serde(default)]
    pub ext_proc: ExtProcConfig,
}

/// Gateway server listen addresses and timeouts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayServer {
    pub listen_address: String,
    pub admin_address: String,
    pub request_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub max_request_body_bytes: u64,
    pub trust_forwarded_headers: bool,
    /// Address for the ext_authz HTTP service (e.g. `"0.0.0.0:10003"`).
    /// When set, the generated Envoy config includes an ext_authz filter.
    #[serde(default)]
    pub extauthz_address: Option<String>,
    /// Maximum concurrent requests before the gateway returns 503.
    /// `None` means no limit.
    #[serde(default)]
    pub max_concurrent_requests: Option<u64>,
}

/// Top-level auth configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub providers: Vec<AuthProvider>,
}

/// A single JWT/JWKS auth provider.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthProvider {
    pub name: String,
    pub issuer: String,
    pub audience: String,
    pub jwks_uri: String,
    pub cache_ttl_seconds: u64,
    pub clock_skew_seconds: u64,
}

/// Global rate-limiting settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitsConfig {
    pub redis_address: String,
    pub redis_db: u32,
    pub redis_key_prefix: String,
    pub default_timeout_ms: u64,
    pub fail_open: bool,
    pub survivability_mode: SurvivabilityMode,
}

/// In-memory fallback when Redis is unavailable.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurvivabilityMode {
    pub enabled: bool,
    pub fallback_capacity: u64,
    pub fallback_refill_rate_per_sec: f64,
}

/// A single route definition.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    pub name: String,
    pub hostnames: Vec<String>,
    pub path_prefix: String,
    pub methods: Vec<String>,
    pub auth_required: bool,
    #[serde(default)]
    pub auth_provider: Option<String>,
    #[serde(default)]
    pub required_scopes: Option<Vec<String>>,
    pub rate_limit: RouteRateLimit,
    pub upstream: UpstreamConfig,
    #[serde(default)]
    pub plugins: Vec<PluginInstance>,
}

/// Per-route rate limit bucket parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRateLimit {
    pub bucket_capacity: u64,
    pub refill_rate_per_sec: f64,
    pub key_by: String,
}

/// Upstream service reference and request settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamConfig {
    pub service: String,
    pub request_timeout_ms: u64,
    pub retries: u32,
}

/// A backend service with endpoints and health check.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    pub name: String,
    pub endpoints: Vec<String>,
    pub health_check: HealthCheckConfig,
}

/// Health check settings for an upstream service.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthCheckConfig {
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
}

/// Observability and telemetry settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityConfig {
    pub access_log_json: bool,
    pub prometheus_enabled: bool,
    pub tracing: TracingConfig,
}

/// Distributed tracing configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracingConfig {
    pub enabled: bool,
    pub otlp_endpoint: String,
    pub sample_rate: f64,
}

/// Plugin engine settings in gateway config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub limits: PluginLimits,
    #[serde(default)]
    pub global: Vec<PluginInstance>,
}

/// Resource limits for the plugin engine.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginLimits {
    #[serde(default = "default_plugin_max_memory_bytes")]
    pub max_memory_bytes: usize,
    #[serde(default = "default_plugin_max_instructions")]
    pub max_instructions: u32,
    #[serde(default = "default_plugin_chain_timeout_ms")]
    pub chain_timeout_ms: u64,
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: default_plugin_max_memory_bytes(),
            max_instructions: default_plugin_max_instructions(),
            chain_timeout_ms: default_plugin_chain_timeout_ms(),
        }
    }
}

/// A single plugin instance bound globally or to a route.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginInstance {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_fail_mode")]
    pub fail_mode: String,
    #[serde(default)]
    pub config: serde_yaml::Value,
}

/// xDS server settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XdsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_xds_listen_address")]
    pub listen_address: String,
}

impl Default for XdsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_address: default_xds_listen_address(),
        }
    }
}

/// ext_proc server settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtProcConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ext_proc_listen_address")]
    pub listen_address: String,
}

impl Default for ExtProcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_address: default_ext_proc_listen_address(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_fail_mode() -> String {
    "closed".to_owned()
}

fn default_plugin_max_memory_bytes() -> usize {
    8 * 1024 * 1024
}

fn default_plugin_max_instructions() -> u32 {
    1_000_000
}

fn default_plugin_chain_timeout_ms() -> u64 {
    50
}

fn default_xds_listen_address() -> String {
    "0.0.0.0:18000".to_owned()
}

fn default_ext_proc_listen_address() -> String {
    "0.0.0.0:19000".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> &'static str {
        r#"
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
"#
    }

    #[test]
    fn deserialize_valid_config() {
        let cfg: GatewayConfig = serde_yaml::from_str(sample_yaml()).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.gateway.listen_address, "0.0.0.0:8080");
        assert_eq!(cfg.routes.len(), 1);
        assert_eq!(cfg.services.len(), 1);
        assert_eq!(cfg.auth.providers.len(), 1);
        assert!(!cfg.plugins.enabled);
        assert!(!cfg.xds.enabled);
        assert!(!cfg.ext_proc.enabled);
    }

    #[test]
    fn reject_unknown_fields() {
        let yaml = r#"
version: 1
gateway:
  listen_address: "0.0.0.0:8080"
  admin_address: "0.0.0.0:9090"
  request_timeout_ms: 15000
  idle_timeout_ms: 60000
  max_request_body_bytes: 10485760
  trust_forwarded_headers: false
  bogus_field: true
auth:
  providers: []
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
routes: []
services: []
observability:
  access_log_json: true
  prometheus_enabled: true
  tracing:
    enabled: false
    otlp_endpoint: ""
    sample_rate: 0.0
"#;
        let result = serde_yaml::from_str::<GatewayConfig>(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn plugin_defaults_are_backwards_compatible() {
        let cfg: GatewayConfig = serde_yaml::from_str(sample_yaml()).unwrap();
        assert_eq!(cfg.plugins.directory, "");
        assert_eq!(cfg.plugins.limits.max_memory_bytes, 8 * 1024 * 1024);
        assert_eq!(cfg.plugins.limits.max_instructions, 1_000_000);
        assert_eq!(cfg.plugins.limits.chain_timeout_ms, 50);
    }
}

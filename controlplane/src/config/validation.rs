use std::collections::HashSet;

use shared::config_error::{ConfigError, ConfigErrors};
use shared::config_types::GatewayConfig;
use url::Url;

const VALID_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];
const VALID_KEY_BY: &[&str] = &["ip", "sub"];

/// Validate a parsed [`GatewayConfig`], collecting all errors.
///
/// Returns `Ok(())` when the config is valid, or `Err(ConfigErrors)` with
/// every violation found.
pub(crate) fn validate(cfg: &GatewayConfig) -> Result<(), ConfigErrors> {
    let mut errs: Vec<ConfigError> = Vec::new();

    check_version(cfg, &mut errs);
    check_uniqueness(cfg, &mut errs);
    check_referential_integrity(cfg, &mut errs);
    check_value_constraints(cfg, &mut errs);
    check_conditional_requirements(cfg, &mut errs);

    if errs.is_empty() {
        Ok(())
    } else {
        Err(ConfigErrors::new(errs))
    }
}

fn check_version(cfg: &GatewayConfig, errs: &mut Vec<ConfigError>) {
    if cfg.version != 1 {
        errs.push(ConfigError::InvalidVersion {
            expected: 1,
            actual: cfg.version,
        });
    }
}

fn check_uniqueness(cfg: &GatewayConfig, errs: &mut Vec<ConfigError>) {
    let mut route_names = HashSet::new();
    for r in &cfg.routes {
        if !route_names.insert(&r.name) {
            errs.push(ConfigError::DuplicateName {
                kind: "route",
                name: r.name.clone(),
            });
        }
    }

    let mut service_names = HashSet::new();
    for s in &cfg.services {
        if !service_names.insert(&s.name) {
            errs.push(ConfigError::DuplicateName {
                kind: "service",
                name: s.name.clone(),
            });
        }
    }

    let mut provider_names = HashSet::new();
    for p in &cfg.auth.providers {
        if !provider_names.insert(&p.name) {
            errs.push(ConfigError::DuplicateName {
                kind: "auth provider",
                name: p.name.clone(),
            });
        }
    }
}

fn check_referential_integrity(cfg: &GatewayConfig, errs: &mut Vec<ConfigError>) {
    let service_names: HashSet<&str> = cfg.services.iter().map(|s| s.name.as_str()).collect();
    let provider_names: HashSet<&str> =
        cfg.auth.providers.iter().map(|p| p.name.as_str()).collect();

    for r in &cfg.routes {
        if !service_names.contains(r.upstream.service.as_str()) {
            errs.push(ConfigError::UnknownReference {
                field: format!("routes[{}].upstream.service", r.name),
                target_kind: "service",
                target: r.upstream.service.clone(),
            });
        }

        if r.auth_required {
            if let Some(ref provider) = r.auth_provider {
                if !provider_names.contains(provider.as_str()) {
                    errs.push(ConfigError::UnknownReference {
                        field: format!("routes[{}].auth_provider", r.name),
                        target_kind: "auth provider",
                        target: provider.clone(),
                    });
                }
            }
        }
    }
}

fn check_value_constraints(cfg: &GatewayConfig, errs: &mut Vec<ConfigError>) {
    // Gateway address format
    validate_host_port(&cfg.gateway.listen_address, "gateway.listen_address", errs);
    validate_host_port(&cfg.gateway.admin_address, "gateway.admin_address", errs);

    // Auth providers
    for p in &cfg.auth.providers {
        if Url::parse(&p.jwks_uri).is_err() {
            errs.push(ConfigError::InvalidValue {
                field: format!("auth.providers[{}].jwks_uri", p.name),
                reason: "must be a valid URL".into(),
            });
        }
        if p.cache_ttl_seconds == 0 {
            errs.push(ConfigError::InvalidValue {
                field: format!("auth.providers[{}].cache_ttl_seconds", p.name),
                reason: "must be > 0".into(),
            });
        }
    }

    // Rate limits
    validate_host_port(
        &cfg.rate_limits.redis_address,
        "rate_limits.redis_address",
        errs,
    );

    // Routes
    for r in &cfg.routes {
        let prefix = format!("routes[{}]", r.name);

        if !r.path_prefix.starts_with('/') {
            errs.push(ConfigError::InvalidValue {
                field: format!("{prefix}.path_prefix"),
                reason: "must start with '/'".into(),
            });
        }

        for m in &r.methods {
            if !VALID_METHODS.contains(&m.as_str()) {
                errs.push(ConfigError::InvalidValue {
                    field: format!("{prefix}.methods"),
                    reason: format!("invalid HTTP method: {m}"),
                });
            }
        }

        if r.rate_limit.bucket_capacity < 1 {
            errs.push(ConfigError::InvalidValue {
                field: format!("{prefix}.rate_limit.bucket_capacity"),
                reason: "must be >= 1".into(),
            });
        }

        if r.rate_limit.refill_rate_per_sec <= 0.0 {
            errs.push(ConfigError::InvalidValue {
                field: format!("{prefix}.rate_limit.refill_rate_per_sec"),
                reason: "must be > 0".into(),
            });
        }

        if !VALID_KEY_BY.contains(&r.rate_limit.key_by.as_str()) {
            errs.push(ConfigError::InvalidValue {
                field: format!("{prefix}.rate_limit.key_by"),
                reason: format!("must be one of: {}", VALID_KEY_BY.join(", ")),
            });
        }
    }

    // Services
    for s in &cfg.services {
        let prefix = format!("services[{}]", s.name);

        for ep in &s.endpoints {
            validate_host_port(ep, &format!("{prefix}.endpoints"), errs);
        }

        if !s.health_check.path.starts_with('/') {
            errs.push(ConfigError::InvalidValue {
                field: format!("{prefix}.health_check.path"),
                reason: "must start with '/'".into(),
            });
        }
    }

    // Observability -- tracing sample_rate
    if cfg.observability.tracing.sample_rate < 0.0 || cfg.observability.tracing.sample_rate > 1.0 {
        errs.push(ConfigError::InvalidValue {
            field: "observability.tracing.sample_rate".into(),
            reason: "must be between 0.0 and 1.0".into(),
        });
    }
}

fn check_conditional_requirements(cfg: &GatewayConfig, errs: &mut Vec<ConfigError>) {
    for r in &cfg.routes {
        let prefix = format!("routes[{}]", r.name);

        if r.auth_required && r.auth_provider.is_none() {
            errs.push(ConfigError::MissingConditional {
                field: format!("{prefix}.auth_provider"),
                reason: "required when auth_required is true".into(),
            });
        }

        if let Some(ref scopes) = r.required_scopes {
            if scopes.is_empty() {
                errs.push(ConfigError::InvalidValue {
                    field: format!("{prefix}.required_scopes"),
                    reason: "must be non-empty when present".into(),
                });
            }
        }
    }

    if cfg.observability.tracing.enabled && cfg.observability.tracing.otlp_endpoint.is_empty() {
        errs.push(ConfigError::MissingConditional {
            field: "observability.tracing.otlp_endpoint".into(),
            reason: "required when tracing is enabled".into(),
        });
    }
}

fn validate_host_port(value: &str, field: &str, errs: &mut Vec<ConfigError>) {
    let parts: Vec<&str> = value.rsplitn(2, ':').collect();
    if parts.len() != 2 || parts[0].parse::<u16>().is_err() || parts[1].is_empty() {
        errs.push(ConfigError::InvalidValue {
            field: field.into(),
            reason: format!("must be host:port format, got: {value}"),
        });
    }
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;

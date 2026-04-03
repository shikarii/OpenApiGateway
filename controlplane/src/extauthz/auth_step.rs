use axum::http::{HeaderMap, StatusCode};

use crate::admin::state::SharedState;

use super::helpers::{extract_bearer, BearerResult};

pub(super) struct AuthDeny {
    pub status: StatusCode,
    pub error_code: String,
}

pub(super) async fn apply_auth(
    state: &SharedState,
    cfg: &shared::config_types::GatewayConfig,
    route: &shared::config_types::RouteConfig,
    headers: &HeaderMap,
    response_headers: &mut HeaderMap,
    auth_subject: &mut Option<String>,
) -> Result<(), AuthDeny> {
    if !route.auth_required {
        return Ok(());
    }

    tracing::info!(route = %route.name, "auth_check");
    let token = match extract_bearer(headers) {
        Some(BearerResult::Valid(token)) => token,
        Some(BearerResult::Malformed) => {
            state
                .metrics
                .record_auth_failure(&route.name, "invalid_token_format");
            return Err(AuthDeny {
                status: StatusCode::UNAUTHORIZED,
                error_code: "invalid_token_format".to_owned(),
            });
        }
        None => {
            state
                .metrics
                .record_auth_failure(&route.name, "missing_token");
            return Err(AuthDeny {
                status: StatusCode::UNAUTHORIZED,
                error_code: "missing_token".to_owned(),
            });
        }
    };

    let provider_name = route.auth_provider.as_deref().unwrap_or("main");
    let provider = cfg
        .auth
        .providers
        .iter()
        .find(|provider| provider.name == provider_name)
        .ok_or_else(|| {
            state
                .metrics
                .record_auth_failure(&route.name, "unknown_provider");
            AuthDeny {
                status: StatusCode::SERVICE_UNAVAILABLE,
                error_code: "auth_provider_unavailable".to_owned(),
            }
        })?;

    let cache = state.jwks_registry.get(provider_name).ok_or_else(|| {
        state
            .metrics
            .record_auth_failure(&route.name, "unknown_provider");
        AuthDeny {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error_code: "auth_provider_unavailable".to_owned(),
        }
    })?;

    let required_scopes = route.required_scopes.as_deref().unwrap_or(&[]);
    match crate::auth::validate_with_refresh(token, provider, cache, required_scopes).await {
        Ok(identity) => {
            tracing::Span::current().record("auth_subject", identity.sub.as_str());
            *auth_subject = Some(identity.sub.clone());
            if let Ok(value) = identity.sub.parse() {
                response_headers.insert("x-auth-sub", value);
            }
            if let Ok(value) = identity.iss.parse() {
                response_headers.insert("x-auth-iss", value);
            }
            let scopes = identity.scopes.join(",");
            if let Ok(value) = scopes.parse() {
                response_headers.insert("x-auth-scopes", value);
            }
            Ok(())
        }
        Err(error) => {
            state
                .metrics
                .record_auth_failure(&route.name, error.error_code());
            Err(AuthDeny {
                status: error.http_status(),
                error_code: error.error_code().to_owned(),
            })
        }
    }
}

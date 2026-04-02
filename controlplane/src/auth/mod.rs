// Auth subsystem types and functions used by tests now and by the proxy
// middleware when the data-plane integration lands (Issue #14).
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod jwks_cache;
pub(crate) mod registry;
#[allow(dead_code)]
mod types;
#[allow(dead_code)]
mod validator;

#[allow(dead_code)]
pub(crate) use error::AuthError;
pub(crate) use registry::JwksCacheRegistry;
#[allow(dead_code)]
pub(crate) use types::ValidatedIdentity;

pub(crate) use jwks_cache::JwksCache;
#[allow(dead_code)]
use shared::config_types::AuthProvider;

#[cfg(test)]
#[path = "integration_tests.rs"]
mod integration_tests;

#[allow(dead_code)]
/// Validate a token, triggering a JWKS refresh if the `kid` is unknown.
///
/// On unknown `kid`, triggers one background refresh and retries. Returns
/// `Err(AuthError::UnknownKeyId)` if the `kid` is still missing after refresh.
pub(crate) async fn validate_with_refresh(
    raw_token: &str,
    provider: &AuthProvider,
    cache: &JwksCache,
    required_scopes: &[String],
) -> Result<ValidatedIdentity, AuthError> {
    if cache.is_stale().await {
        return Err(AuthError::AuthProviderUnavailable);
    }

    let keys = cache.get_keys().await;
    match validator::validate_token(raw_token, provider, &keys, required_scopes) {
        Err(AuthError::UnknownKeyId) => {
            cache.trigger_refresh();
            // Wait for the refresh with a bounded timeout.
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(5), cache.wait_for_refresh())
                    .await;
            let keys = cache.get_keys().await;
            validator::validate_token(raw_token, provider, &keys, required_scopes)
        }
        other => other,
    }
}

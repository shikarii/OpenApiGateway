use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

/// Auth-related errors with direct mapping to HTTP status and JSON body.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthError {
    #[error("missing_token")]
    MissingToken,
    #[error("invalid_token_format")]
    InvalidTokenFormat,
    #[error("invalid_token")]
    InvalidToken,
    #[error("unsupported_algorithm")]
    UnsupportedAlgorithm,
    #[error("unknown_key_id")]
    UnknownKeyId,
    #[error("invalid_signature")]
    InvalidSignature,
    #[error("invalid_issuer")]
    InvalidIssuer,
    #[error("invalid_audience")]
    InvalidAudience,
    #[error("token_expired")]
    TokenExpired,
    #[error("token_not_yet_valid")]
    TokenNotYetValid,
    #[error("missing_subject")]
    MissingSubject,
    #[error("insufficient_scopes")]
    InsufficientScopes,
    #[error("auth_provider_unavailable")]
    AuthProviderUnavailable,
    /// Internal: JWKS fetch failed. Surfaces as `auth_provider_unavailable` to clients.
    #[error("jwks fetch error: {0}")]
    JwksFetch(String),
}

impl AuthError {
    /// HTTP status code for this error.
    pub(crate) fn http_status(&self) -> StatusCode {
        match self {
            Self::InsufficientScopes => StatusCode::FORBIDDEN,
            Self::AuthProviderUnavailable | Self::JwksFetch(_) => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::UNAUTHORIZED,
        }
    }

    /// Machine-readable error code for the JSON body.
    pub(crate) fn error_code(&self) -> &'static str {
        match self {
            Self::MissingToken => "missing_token",
            Self::InvalidTokenFormat => "invalid_token_format",
            Self::InvalidToken => "invalid_token",
            Self::UnsupportedAlgorithm => "unsupported_algorithm",
            Self::UnknownKeyId => "unknown_key_id",
            Self::InvalidSignature => "invalid_signature",
            Self::InvalidIssuer => "invalid_issuer",
            Self::InvalidAudience => "invalid_audience",
            Self::TokenExpired => "token_expired",
            Self::TokenNotYetValid => "token_not_yet_valid",
            Self::MissingSubject => "missing_subject",
            Self::InsufficientScopes => "insufficient_scopes",
            Self::AuthProviderUnavailable | Self::JwksFetch(_) => "auth_provider_unavailable",
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let status = self.http_status();
        let body = serde_json::json!({ "error": self.error_code() });
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_have_correct_status() {
        let cases: Vec<(AuthError, StatusCode)> = vec![
            (AuthError::MissingToken, StatusCode::UNAUTHORIZED),
            (AuthError::InvalidTokenFormat, StatusCode::UNAUTHORIZED),
            (AuthError::InvalidToken, StatusCode::UNAUTHORIZED),
            (AuthError::UnsupportedAlgorithm, StatusCode::UNAUTHORIZED),
            (AuthError::UnknownKeyId, StatusCode::UNAUTHORIZED),
            (AuthError::InvalidSignature, StatusCode::UNAUTHORIZED),
            (AuthError::InvalidIssuer, StatusCode::UNAUTHORIZED),
            (AuthError::InvalidAudience, StatusCode::UNAUTHORIZED),
            (AuthError::TokenExpired, StatusCode::UNAUTHORIZED),
            (AuthError::TokenNotYetValid, StatusCode::UNAUTHORIZED),
            (AuthError::MissingSubject, StatusCode::UNAUTHORIZED),
            (AuthError::InsufficientScopes, StatusCode::FORBIDDEN),
            (
                AuthError::AuthProviderUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                AuthError::JwksFetch("test".into()),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(err.http_status(), expected, "wrong status for {err}");
        }
    }

    #[test]
    fn error_codes_are_snake_case() {
        let variants: Vec<AuthError> = vec![
            AuthError::MissingToken,
            AuthError::InvalidTokenFormat,
            AuthError::InvalidToken,
            AuthError::UnsupportedAlgorithm,
            AuthError::UnknownKeyId,
            AuthError::InvalidSignature,
            AuthError::InvalidIssuer,
            AuthError::InvalidAudience,
            AuthError::TokenExpired,
            AuthError::TokenNotYetValid,
            AuthError::MissingSubject,
            AuthError::InsufficientScopes,
            AuthError::AuthProviderUnavailable,
        ];

        for v in variants {
            let code = v.error_code();
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "error code not snake_case: {code}"
            );
        }
    }

    #[test]
    fn jwks_fetch_surfaces_as_unavailable() {
        let err = AuthError::JwksFetch("connection refused".into());
        assert_eq!(err.error_code(), "auth_provider_unavailable");
    }
}

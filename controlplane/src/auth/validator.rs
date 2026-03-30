use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use shared::config_types::AuthProvider;

use super::error::AuthError;
use super::types::{Claims, JwksKey, ValidatedIdentity};

/// Validate a raw JWT string against a provider config and a key set.
///
/// This function is pure: it performs no I/O. The caller is responsible for
/// resolving the correct key set from the JWKS cache.
pub(crate) fn validate_token(
    raw_token: &str,
    provider: &AuthProvider,
    keys: &[JwksKey],
    required_scopes: &[String],
) -> Result<ValidatedIdentity, AuthError> {
    // 1. Decode header to check alg and extract kid.
    let header = decode_header(raw_token).map_err(|_| AuthError::InvalidToken)?;

    if header.alg != Algorithm::RS256 {
        return Err(AuthError::UnsupportedAlgorithm);
    }

    let kid = header.kid.as_deref().ok_or(AuthError::UnknownKeyId)?;

    // 2. Find the matching key by kid.
    let key = keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or(AuthError::UnknownKeyId)?;

    // 3. Decode and verify signature + standard claims.
    let claims = decode_and_verify(raw_token, &key.decoding_key, provider)?;

    // 4. Scope check.
    let token_scopes = extract_scopes(&claims);
    check_scopes(&token_scopes, required_scopes)?;

    // 5. Extract sub (mandatory).
    let sub = claims.sub.ok_or(AuthError::MissingSubject)?;

    let aud = match claims.aud {
        super::types::Audience::Single(s) => vec![s],
        super::types::Audience::Multiple(v) => v,
    };

    Ok(ValidatedIdentity {
        sub,
        iss: claims.iss.unwrap_or_default(),
        aud,
        scopes: token_scopes,
        exp_unix: claims.exp.unwrap_or(0),
    })
}

/// Decode token, verify signature, and validate iss/aud/exp/nbf with clock skew.
fn decode_and_verify(
    raw_token: &str,
    decoding_key: &DecodingKey,
    provider: &AuthProvider,
) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[&provider.issuer]);
    validation.set_audience(&[&provider.audience]);
    // jsonwebtoken validates exp/nbf automatically; configure clock skew.
    validation.leeway = provider.clock_skew_seconds;
    validation.validate_exp = true;
    validation.validate_nbf = true;
    // We validate sub ourselves (it's optional in JWT spec but required by our spec).
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);

    let token_data = decode::<Claims>(raw_token, decoding_key, &validation).map_err(|e| {
        use jsonwebtoken::errors::ErrorKind;
        match e.kind() {
            ErrorKind::InvalidSignature => AuthError::InvalidSignature,
            ErrorKind::InvalidIssuer => AuthError::InvalidIssuer,
            ErrorKind::InvalidAudience => AuthError::InvalidAudience,
            ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            ErrorKind::ImmatureSignature => AuthError::TokenNotYetValid,
            _ => AuthError::InvalidToken,
        }
    })?;

    Ok(token_data.claims)
}

/// Extract scopes from `scope` (space-delimited) or `scp` (array) claim.
fn extract_scopes(claims: &Claims) -> Vec<String> {
    if let Some(ref scope_str) = claims.scope {
        return scope_str.split_whitespace().map(String::from).collect();
    }
    if let Some(ref scp_arr) = claims.scp {
        return scp_arr.clone();
    }
    Vec::new()
}

/// Verify all required scopes are present in the token scopes.
fn check_scopes(token_scopes: &[String], required: &[String]) -> Result<(), AuthError> {
    for req in required {
        if !token_scopes.iter().any(|s| s == req) {
            return Err(AuthError::InsufficientScopes);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "validator_tests.rs"]
mod tests;

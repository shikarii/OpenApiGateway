use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
use shared::config_types::AuthProvider;
use std::time::{Duration, Instant};

use super::error::AuthError;
use super::jwks_cache::JwksCache;
use super::types::JwksKey;
use super::validate_with_refresh;

// ── helpers (duplicated from validator_tests to keep files independent) ──

fn rsa_key_pair() -> (EncodingKey, DecodingKey) {
    let priv_pem = include_bytes!("../../../test_fixtures/rsa_private.pem");
    let pub_pem = include_bytes!("../../../test_fixtures/rsa_public.pem");
    (
        EncodingKey::from_rsa_pem(priv_pem).expect("test private key"),
        DecodingKey::from_rsa_pem(pub_pem).expect("test public key"),
    )
}

fn test_provider() -> AuthProvider {
    AuthProvider {
        name: "test".into(),
        issuer: "https://auth.example.com/".into(),
        audience: "api-gateway".into(),
        jwks_uri: "http://localhost/.well-known/jwks.json".into(),
        cache_ttl_seconds: 300,
        clock_skew_seconds: 30,
    }
}

fn make_jwks_keys(decoding_key: DecodingKey, kid: &str) -> Vec<JwksKey> {
    vec![JwksKey {
        kid: kid.to_string(),
        decoding_key,
    }]
}

#[derive(Debug, serde::Serialize)]
struct TestClaims {
    sub: Option<String>,
    iss: Option<String>,
    aud: serde_json::Value,
    exp: i64,
    nbf: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scp: Option<Vec<String>>,
}

fn default_claims() -> TestClaims {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs() as i64;
    TestClaims {
        sub: Some("user-123".into()),
        iss: Some("https://auth.example.com/".into()),
        aud: serde_json::json!("api-gateway"),
        exp: now + 3600,
        nbf: now - 60,
        scope: Some("api.read api.write".into()),
        scp: None,
    }
}

fn encode_token(claims: &TestClaims, enc_key: &EncodingKey, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    encode(&header, claims, enc_key).expect("encode failed")
}

// ── integration tests ──

#[tokio::test]
async fn stale_cache_returns_provider_unavailable() {
    let provider = test_provider();
    let old = Instant::now() - Duration::from_secs(7200);
    // TTL=1, STALE_FACTOR=10 → threshold 10s; elapsed ~7200s → stale.
    let stale_provider = AuthProvider {
        cache_ttl_seconds: 1,
        ..provider.clone()
    };
    let cache = JwksCache::for_test(stale_provider, vec![], old);
    let err = validate_with_refresh("dummy.token.here", &provider, &cache, &[])
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthError::AuthProviderUnavailable),
        "expected AuthProviderUnavailable, got {err:?}"
    );
}

#[tokio::test]
async fn valid_token_through_full_pipeline() {
    let (enc, dec, kid) = {
        let pair = rsa_key_pair();
        (pair.0, pair.1, "test-key-1".to_string())
    };
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let cache = JwksCache::for_test(provider.clone(), keys, Instant::now());

    let claims = default_claims();
    let token = encode_token(&claims, &enc, &kid);

    let identity = validate_with_refresh(&token, &provider, &cache, &[])
        .await
        .expect("validation should succeed");
    assert_eq!(identity.sub, "user-123");
    assert_eq!(identity.iss, "https://auth.example.com/");
}

#[tokio::test]
async fn unknown_kid_returns_error_after_timeout() {
    // Pause time so the 5-second timeout in validate_with_refresh completes instantly.
    tokio::time::pause();

    let (enc, dec, _) = {
        let pair = rsa_key_pair();
        (pair.0, pair.1, "test-key-1".to_string())
    };
    let provider = test_provider();
    // Load cache with key "correct-kid" but sign token with "wrong-kid".
    let keys = make_jwks_keys(dec, "correct-kid");
    let cache = JwksCache::for_test(provider.clone(), keys, Instant::now());

    let claims = default_claims();
    let token = encode_token(&claims, &enc, "wrong-kid");

    let err = validate_with_refresh(&token, &provider, &cache, &[])
        .await
        .unwrap_err();
    assert!(
        matches!(err, AuthError::UnknownKeyId),
        "expected UnknownKeyId, got {err:?}"
    );
}

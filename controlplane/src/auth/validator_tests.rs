use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
use shared::config_types::AuthProvider;

use super::*;

/// Build a test RSA key pair. Returns (encoding_key, decoding_key, kid).
fn test_keys() -> (EncodingKey, DecodingKey, String) {
    // Use a pre-generated 2048-bit RSA key for deterministic tests.
    let rsa = rsa_key_pair();
    let kid = "test-key-1".to_string();
    (rsa.0, rsa.1, kid)
}

fn rsa_key_pair() -> (EncodingKey, DecodingKey) {
    use jsonwebtoken::{DecodingKey, EncodingKey};
    // Generate a fresh RSA key pair via the rsa crate (transitive dep of jsonwebtoken).
    // For tests we use a small key; this is NOT production-safe.
    let doc = include_bytes!("../../../test_fixtures/rsa_private.pem");
    let pub_doc = include_bytes!("../../../test_fixtures/rsa_public.pem");
    (
        EncodingKey::from_rsa_pem(doc).expect("test private key"),
        DecodingKey::from_rsa_pem(pub_doc).expect("test public key"),
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
        .unwrap()
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

#[test]
fn valid_token_passes() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let claims = default_claims();
    let token = encode_token(&claims, &enc, &kid);

    let identity = validate_token(&token, &provider, &keys, &[]).unwrap();
    assert_eq!(identity.sub, "user-123");
    assert_eq!(identity.iss, "https://auth.example.com/");
    assert_eq!(identity.scopes, vec!["api.read", "api.write"]);
}

#[test]
fn reject_non_rs256() {
    let (_enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let claims = default_claims();

    // Encode with HS256 header (will fail signature but header check comes first).
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(kid.clone());
    // HS256 needs a symmetric key; use a dummy.
    let token =
        encode(&header, &claims, &EncodingKey::from_secret(b"secret")).expect("encode failed");

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "unsupported_algorithm");
}

#[test]
fn reject_unknown_kid() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let claims = default_claims();
    let token = encode_token(&claims, &enc, "unknown-kid");

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "unknown_key_id");
}

#[test]
fn reject_wrong_issuer() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    claims.iss = Some("https://wrong.example.com/".into());
    let token = encode_token(&claims, &enc, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "invalid_issuer");
}

#[test]
fn reject_wrong_audience() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    claims.aud = serde_json::json!("wrong-audience");
    let token = encode_token(&claims, &enc, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "invalid_audience");
}

#[test]
fn reject_expired_token() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    // Expired well beyond clock skew.
    claims.exp = 1000;
    claims.nbf = 500;
    let token = encode_token(&claims, &enc, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "token_expired");
}

#[test]
fn reject_not_yet_valid() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    // nbf far in the future, beyond clock skew.
    claims.nbf = now + 3600;
    let token = encode_token(&claims, &enc, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "token_not_yet_valid");
}

#[test]
fn reject_missing_subject() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    claims.sub = None;
    let token = encode_token(&claims, &enc, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "missing_subject");
}

#[test]
fn reject_insufficient_scopes() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let claims = default_claims(); // has api.read, api.write
    let token = encode_token(&claims, &enc, &kid);

    let required = vec!["admin.delete".to_string()];
    let err = validate_token(&token, &provider, &keys, &required).unwrap_err();
    assert_eq!(err.error_code(), "insufficient_scopes");
}

#[test]
fn scope_superset_passes() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let claims = default_claims(); // has api.read, api.write
    let token = encode_token(&claims, &enc, &kid);

    let required = vec!["api.read".to_string()];
    let identity = validate_token(&token, &provider, &keys, &required).unwrap();
    assert_eq!(identity.sub, "user-123");
}

#[test]
fn scp_array_claim_parsed() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    claims.scope = None;
    claims.scp = Some(vec!["api.read".into(), "api.write".into()]);
    let token = encode_token(&claims, &enc, &kid);

    let identity = validate_token(&token, &provider, &keys, &[]).unwrap();
    assert_eq!(identity.scopes, vec!["api.read", "api.write"]);
}

#[test]
fn malformed_token_rejected() {
    let provider = test_provider();
    let err = validate_token("not.a.jwt", &provider, &[], &[]).unwrap_err();
    assert_eq!(err.error_code(), "invalid_token");
}

#[test]
fn signature_mismatch_rejected() {
    let (enc, _, kid) = test_keys();
    let provider = test_provider();
    let claims = default_claims();
    let token = encode_token(&claims, &enc, &kid);

    // Use a different key for verification.
    let (_, other_dec, _) = other_test_keys();
    let keys = make_jwks_keys(other_dec, &kid);

    let err = validate_token(&token, &provider, &keys, &[]).unwrap_err();
    assert_eq!(err.error_code(), "invalid_signature");
}

fn other_test_keys() -> (EncodingKey, DecodingKey, String) {
    let doc = include_bytes!("../../../test_fixtures/rsa_private_2.pem");
    let pub_doc = include_bytes!("../../../test_fixtures/rsa_public_2.pem");
    (
        EncodingKey::from_rsa_pem(doc).expect("test private key 2"),
        DecodingKey::from_rsa_pem(pub_doc).expect("test public key 2"),
        "test-key-2".to_string(),
    )
}

#[test]
fn audience_array_accepted() {
    let (enc, dec, kid) = test_keys();
    let provider = test_provider();
    let keys = make_jwks_keys(dec, &kid);
    let mut claims = default_claims();
    claims.aud = serde_json::json!(["api-gateway", "admin"]);
    let token = encode_token(&claims, &enc, &kid);

    let identity = validate_token(&token, &provider, &keys, &[]).unwrap();
    assert_eq!(identity.aud, vec!["api-gateway", "admin"]);
}

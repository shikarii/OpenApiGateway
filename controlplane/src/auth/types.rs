/// Canonical identity extracted from a validated JWT.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedIdentity {
    pub sub: String,
    pub iss: String,
    pub aud: Vec<String>,
    pub scopes: Vec<String>,
    pub exp_unix: i64,
}

/// A single JWKS key with its decoded signing material.
pub(crate) struct JwksKey {
    pub kid: String,
    pub decoding_key: jsonwebtoken::DecodingKey,
}

/// Raw JWKS JSON response from an identity provider.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct JwksDocument {
    pub keys: Vec<JwksRawKey>,
}

/// A single key entry in a JWKS JSON response.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct JwksRawKey {
    pub kid: String,
    pub kty: String,
    #[serde(default)]
    pub alg: Option<String>,
    pub n: String,
    pub e: String,
}

/// JWT claims used during validation.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Claims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    #[serde(default)]
    pub aud: Audience,
    pub exp: Option<i64>,
    pub nbf: Option<i64>,
    /// Space-delimited scope string (standard).
    pub scope: Option<String>,
    /// Array-form scope claim (alternative).
    pub scp: Option<Vec<String>>,
}

/// Audience claim that may be a single string or an array.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

impl Default for Audience {
    fn default() -> Self {
        Self::Multiple(Vec::new())
    }
}

impl Audience {
    /// Check whether the audience set contains a specific value.
    pub(crate) fn contains(&self, target: &str) -> bool {
        match self {
            Self::Single(s) => s == target,
            Self::Multiple(v) => v.iter().any(|a| a == target),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_single_contains() {
        let aud = Audience::Single("api-gateway".into());
        assert!(aud.contains("api-gateway"));
        assert!(!aud.contains("other"));
    }

    #[test]
    fn audience_multiple_contains() {
        let aud = Audience::Multiple(vec!["api-gateway".into(), "admin".into()]);
        assert!(aud.contains("api-gateway"));
        assert!(aud.contains("admin"));
        assert!(!aud.contains("other"));
    }

    #[test]
    fn audience_default_is_empty() {
        let aud = Audience::default();
        assert!(!aud.contains("anything"));
    }
}

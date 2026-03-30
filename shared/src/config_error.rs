use std::fmt;

use thiserror::Error;

/// A single configuration validation error.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("version must be {expected}, got {actual}")]
    InvalidVersion { expected: u32, actual: u32 },

    #[error("duplicate {kind} name: {name}")]
    DuplicateName { kind: &'static str, name: String },

    #[error("{field} references unknown {target_kind}: {target}")]
    UnknownReference {
        field: String,
        target_kind: &'static str,
        target: String,
    },

    #[error("{field}: {reason}")]
    InvalidValue { field: String, reason: String },

    #[error("{field}: {reason}")]
    MissingConditional { field: String, reason: String },

    #[error("YAML parse error: {0}")]
    YamlParse(String),
}

/// Accumulates multiple [`ConfigError`]s so validation is not fail-fast.
#[derive(Debug)]
pub struct ConfigErrors {
    errors: Vec<ConfigError>,
}

impl ConfigErrors {
    pub fn new(errors: Vec<ConfigError>) -> Self {
        Self { errors }
    }

    pub fn errors(&self) -> &[ConfigError] {
        &self.errors
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }
}

impl fmt::Display for ConfigErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} config error(s):", self.errors.len())?;
        for (i, e) in self.errors.iter().enumerate() {
            writeln!(f, "  {}: {e}", i + 1)?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigErrors {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_errors_display_lists_all() {
        let errs = ConfigErrors::new(vec![
            ConfigError::InvalidVersion {
                expected: 1,
                actual: 2,
            },
            ConfigError::DuplicateName {
                kind: "route",
                name: "foo".into(),
            },
        ]);
        let text = errs.to_string();
        assert!(text.contains("2 config error(s)"));
        assert!(text.contains("version must be 1, got 2"));
        assert!(text.contains("duplicate route name: foo"));
    }

    #[test]
    fn config_errors_len_and_empty() {
        let empty = ConfigErrors::new(vec![]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let one = ConfigErrors::new(vec![ConfigError::YamlParse("bad".into())]);
        assert!(!one.is_empty());
        assert_eq!(one.len(), 1);
    }
}

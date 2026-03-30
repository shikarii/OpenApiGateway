/// Rate-limiting errors.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RateLimitError {
    #[error("redis connection error: {0}")]
    RedisConnect(String),
    #[error("redis command error: {0}")]
    RedisCommand(String),
    #[error("redis timeout")]
    Timeout,
    #[error("rate limiter unavailable")]
    Unavailable,
    #[error("lua response parse error: {0}")]
    LuaResponseParse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        let e = RateLimitError::RedisConnect("refused".into());
        assert!(e.to_string().contains("refused"));

        let e = RateLimitError::RedisCommand("OOM".into());
        assert!(e.to_string().contains("OOM"));

        assert!(RateLimitError::Timeout.to_string().contains("timeout"));
        assert!(RateLimitError::Unavailable
            .to_string()
            .contains("unavailable"));

        let e = RateLimitError::LuaResponseParse("bad json".into());
        assert!(e.to_string().contains("bad json"));
    }
}

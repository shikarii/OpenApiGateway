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

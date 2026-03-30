// Rate limiting subsystem: token bucket with Redis + Lua primary
// and in-memory fallback for survivability mode.
#[allow(dead_code)]
mod bucket;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
pub(crate) mod limiter;
#[allow(dead_code)]
mod lua;

#[allow(dead_code, unused_imports)]
pub(crate) use error::RateLimitError;
pub(crate) use limiter::RateLimiter;

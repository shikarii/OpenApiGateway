/// Lua script for atomic token bucket execution in Redis.
///
/// Inputs: KEYS[1]=bucket_key, ARGV[1..5]=now_ms, capacity, refill_rate, requested, ttl.
/// Output: JSON string with {allowed, remaining_tokens, retry_after_ms}.
pub(crate) const LUA_SCRIPT: &str = r#"
local key = KEYS[1]
local now_ms       = tonumber(ARGV[1])
local capacity     = tonumber(ARGV[2])
local refill_rate  = tonumber(ARGV[3])
local requested    = tonumber(ARGV[4])
local ttl_seconds  = tonumber(ARGV[5])

local tokens        = tonumber(redis.call('HGET', key, 'tokens'))
local last_refill   = tonumber(redis.call('HGET', key, 'last_refill_ms'))

if tokens == nil then tokens = capacity end
if last_refill == nil then last_refill = now_ms end

local elapsed_s     = (now_ms - last_refill) / 1000.0
local refilled      = refill_rate * elapsed_s
local new_tokens    = math.min(tokens + refilled, capacity)

if new_tokens >= requested then
    new_tokens = new_tokens - requested
    redis.call('HSET', key, 'tokens', tostring(new_tokens), 'last_refill_ms', tostring(now_ms))
    redis.call('EXPIRE', key, ttl_seconds)
    return '{"allowed":1,"remaining_tokens":' .. math.floor(new_tokens) .. ',"retry_after_ms":0}'
else
    local deficit = requested - new_tokens
    local retry_ms = math.ceil((deficit / refill_rate) * 1000)
    return '{"allowed":0,"remaining_tokens":0,"retry_after_ms":' .. retry_ms .. '}'
end
"#;

/// Deserialised response from the Lua script.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct LuaResponse {
    pub allowed: u8,
    pub remaining_tokens: u64,
    pub retry_after_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_allowed_response() {
        let json = r#"{"allowed":1,"remaining_tokens":9,"retry_after_ms":0}"#;
        let resp: LuaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.allowed, 1);
        assert_eq!(resp.remaining_tokens, 9);
        assert_eq!(resp.retry_after_ms, 0);
    }

    #[test]
    fn parse_denied_response() {
        let json = r#"{"allowed":0,"remaining_tokens":0,"retry_after_ms":180}"#;
        let resp: LuaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.allowed, 0);
        assert_eq!(resp.remaining_tokens, 0);
        assert_eq!(resp.retry_after_ms, 180);
    }

    #[test]
    fn parse_response_zero_remaining_allowed() {
        let json = r#"{"allowed":1,"remaining_tokens":0,"retry_after_ms":0}"#;
        let resp: LuaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.allowed, 1);
        assert_eq!(resp.remaining_tokens, 0);
    }

    #[test]
    fn parse_response_large_retry() {
        let json = r#"{"allowed":0,"remaining_tokens":0,"retry_after_ms":999999}"#;
        let resp: LuaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.retry_after_ms, 999_999);
    }

    #[test]
    fn parse_invalid_json_fails() {
        let bad = r#"{"allowed":}"#;
        assert!(serde_json::from_str::<LuaResponse>(bad).is_err());
    }
}

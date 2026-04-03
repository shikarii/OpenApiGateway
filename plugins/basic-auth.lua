local cjson = require("cjson")

local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 550,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      realm = { type = "string" },
      credentials = {
        type = "array",
        minItems = 1,
        items = {
          type = "object",
          additionalProperties = false,
          properties = {
            username = { type = "string" },
            password = { type = "string" },
            consumer = { type = "string" },
          },
          required = { "username", "password", "consumer" },
        },
      },
    },
    required = { "credentials" },
  },
}

local BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

local function decode_base64(input)
  local sanitized = input:gsub("[^" .. BASE64_ALPHABET .. "=]", "")
  local bit_pattern = sanitized:gsub(".", function(char)
    if char == "=" then
      return ""
    end
    local index = BASE64_ALPHABET:find(char, 1, true)
    if not index then
      return ""
    end
    local value = index - 1
    local bits = ""
    for _ = 1, 6 do
      bits = tostring(value % 2) .. bits
      value = math.floor(value / 2)
    end
    return bits
  end)

  return bit_pattern:gsub("%d%d%d?%d?%d?%d?%d?%d?", function(chunk)
    if #chunk ~= 8 then
      return ""
    end
    local value = 0
    for i = 1, 8 do
      value = (value * 2) + tonumber(chunk:sub(i, i))
    end
    return string.char(value)
  end)
end

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  local realm = config.realm or "OpenApiGateway"
  local authorization = gateway.request.get_header("authorization")
  if not authorization then
    gateway.response.set_header("www-authenticate", 'Basic realm="' .. realm .. '"')
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(401, cjson.encode({ error = "missing_basic_auth" }))
  end

  local encoded = authorization:match("^Basic%s+(.+)$")
  if not encoded then
    gateway.response.set_header("www-authenticate", 'Basic realm="' .. realm .. '"')
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(401, cjson.encode({ error = "invalid_basic_auth" }))
  end

  local decoded = decode_base64(encoded)
  local username, password = decoded:match("^([^:]+):(.*)$")
  if not username then
    gateway.response.set_header("www-authenticate", 'Basic realm="' .. realm .. '"')
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(401, cjson.encode({ error = "invalid_basic_auth" }))
  end

  for _, entry in ipairs(config.credentials) do
    if entry.username == username and entry.password == password then
      gateway.service.request.set_header("x-consumer-id", entry.consumer)
      gateway.service.request.set_header("x-consumer-username", username)
      return
    end
  end

  gateway.response.set_header("www-authenticate", 'Basic realm="' .. realm .. '"')
  gateway.response.set_header("content-type", "application/json")
  gateway.response.exit(401, cjson.encode({ error = "invalid_basic_auth" }))
end

return plugin

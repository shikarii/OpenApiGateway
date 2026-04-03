local cjson = require("cjson")

local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 450,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      allow = {
        type = "array",
        items = { type = "string" },
      },
      deny = {
        type = "array",
        items = { type = "string" },
      },
      status_code = { type = "integer", minimum = 400, maximum = 599 },
      message = { type = "string" },
    },
  },
}

local function ipv4_to_int(ip)
  local a, b, c, d = ip:match("^(%d+)%.(%d+)%.(%d+)%.(%d+)$")
  if not a then
    return nil
  end

  a = tonumber(a)
  b = tonumber(b)
  c = tonumber(c)
  d = tonumber(d)
  for _, value in ipairs({ a, b, c, d }) do
    if value < 0 or value > 255 then
      return nil
    end
  end

  return ((a * 256 + b) * 256 + c) * 256 + d
end

local function match_entry(ip, entry)
  local base, prefix = entry:match("^([^/]+)/(%d+)$")
  if not prefix then
    return ip == entry
  end

  local ip_value = ipv4_to_int(ip)
  local base_value = ipv4_to_int(base)
  prefix = tonumber(prefix)
  if not ip_value or not base_value or prefix < 0 or prefix > 32 then
    return false
  end

  local host_bits = 32 - prefix
  local block_size = 2 ^ host_bits
  return math.floor(ip_value / block_size) == math.floor(base_value / block_size)
end

local function list_matches(ip, entries)
  for _, entry in ipairs(entries or {}) do
    if match_entry(ip, entry) then
      return true
    end
  end
  return false
end

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  local client_ip = gateway.request.get_client_ip()
  local denied = list_matches(client_ip, config.deny)
  local allow_list = config.allow or {}
  local allowed = (#allow_list == 0) or list_matches(client_ip, allow_list)

  if denied or not allowed then
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(
      config.status_code or 403,
      cjson.encode({ error = config.message or "ip_not_allowed", client_ip = client_ip })
    )
  end
end

return plugin

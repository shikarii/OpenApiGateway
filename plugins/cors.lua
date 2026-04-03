local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 100,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      origins = {
        type = "array",
        minItems = 1,
        items = { type = "string" },
      },
      methods = {
        type = "array",
        minItems = 1,
        items = { type = "string" },
      },
      headers = {
        type = "array",
        items = { type = "string" },
      },
      max_age = { type = "integer", minimum = 0 },
      credentials = { type = "boolean" },
    },
    required = { "origins", "methods" },
  },
}

local function join(values)
  return table.concat(values, ", ")
end

local function origin_allowed(origin, origins)
  for _, allowed in ipairs(origins) do
    if allowed == "*" or allowed == origin then
      return true
    end
  end
  return false
end

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  local origin = gateway.request.get_header("origin")
  if not origin or not origin_allowed(origin, config.origins) then
    return
  end

  local allow_origin = origin
  if #config.origins == 1 and config.origins[1] == "*" then
    allow_origin = "*"
  end

  gateway.response.set_header("access-control-allow-origin", allow_origin)
  gateway.response.set_header("vary", "Origin")
  if config.credentials then
    gateway.response.set_header("access-control-allow-credentials", "true")
  end

  if gateway.request.get_method() ~= "OPTIONS" then
    return
  end

  gateway.response.set_header("access-control-allow-methods", join(config.methods))
  if config.headers and #config.headers > 0 then
    gateway.response.set_header("access-control-allow-headers", join(config.headers))
  end
  if config.max_age then
    gateway.response.set_header("access-control-max-age", tostring(config.max_age))
  end
  gateway.response.exit(204, "")
end

return plugin

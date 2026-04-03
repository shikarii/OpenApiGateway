local cjson = require("cjson")

local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 350,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      max_bytes = { type = "integer", minimum = 1 },
      status_code = { type = "integer", minimum = 400, maximum = 599 },
      message = { type = "string" },
    },
    required = { "max_bytes" },
  },
}

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  local content_length = gateway.request.get_header("content-length")
  if not content_length then
    return
  end

  local size = tonumber(content_length)
  if size and size > config.max_bytes then
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(
      config.status_code or 413,
      cjson.encode({ error = config.message or "payload_too_large", max_bytes = config.max_bytes })
    )
  end
end

return plugin

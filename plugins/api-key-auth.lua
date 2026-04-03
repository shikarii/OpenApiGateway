local cjson = require("cjson")

local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 500,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      header_name = { type = "string" },
      query_name = { type = "string" },
      hide_credentials = { type = "boolean" },
      keys = {
        type = "array",
        minItems = 1,
        items = {
          type = "object",
          additionalProperties = false,
          properties = {
            key = { type = "string" },
            consumer = { type = "string" },
          },
          required = { "key", "consumer" },
        },
      },
    },
    required = { "keys" },
  },
}

local function config_value(config, key, fallback)
  if config[key] == nil then
    return fallback
  end
  return config[key]
end

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  local header_name = config_value(config, "header_name", "x-api-key")
  local query_name = config_value(config, "query_name", "apikey")
  local hide_credentials = config_value(config, "hide_credentials", false)

  local key = gateway.request.get_header(header_name)
  local source = "header"
  if not key then
    key = gateway.request.get_query(query_name)
    source = "query"
  end

  if not key then
    gateway.response.set_header("content-type", "application/json")
    gateway.response.exit(401, cjson.encode({ error = "missing_api_key" }))
  end

  for _, entry in ipairs(config.keys) do
    if entry.key == key then
      gateway.service.request.set_header("x-consumer-id", entry.consumer)
      if hide_credentials and source == "query" then
        gateway.service.request.remove_query_param(query_name)
      end
      return
    end
  end

  gateway.response.set_header("content-type", "application/json")
  gateway.response.exit(401, cjson.encode({ error = "invalid_api_key" }))
end

return plugin

local plugin = {
  VERSION = "0.1.0",
  PRIORITY = 200,
  SCHEMA = {
    type = "object",
    additionalProperties = false,
    properties = {
      add_headers = {
        type = "object",
        additionalProperties = { type = "string" },
      },
      rename_headers = {
        type = "object",
        additionalProperties = { type = "string" },
      },
      set_query_params = {
        type = "object",
        additionalProperties = { type = "string" },
      },
      remove_query_params = {
        type = "array",
        items = { type = "string" },
      },
      rewrite_path = { type = "string" },
    },
  },
}

local function apply_map(map_value, callback)
  if not map_value then
    return
  end
  for key, value in pairs(map_value) do
    callback(key, value)
  end
end

function plugin:access(_ctx)
  local config = gateway.plugin.get_config()
  if config.rewrite_path then
    gateway.service.request.set_path(config.rewrite_path)
  end
  apply_map(config.add_headers, function(name, value)
    gateway.service.request.set_header(name, value)
  end)
  apply_map(config.rename_headers, function(from_name, to_name)
    local value = gateway.request.get_header(from_name)
    if value then
      gateway.service.request.set_header(to_name, value)
    end
  end)
  apply_map(config.set_query_params, function(name, value)
    gateway.service.request.set_query_param(name, value)
  end)
  if config.remove_query_params then
    for _, name in ipairs(config.remove_query_params) do
      gateway.service.request.remove_query_param(name)
    end
  end
end

return plugin

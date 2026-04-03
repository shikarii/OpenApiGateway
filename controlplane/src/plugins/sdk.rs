use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, LuaSerdeExt, Result as LuaResult, Table, Value};

use super::types::{PluginBinding, PluginExit, PluginRequest};

const EXIT_SENTINEL: &str = "__gateway_plugin_exit__";
const RESERVED_HEADER_PREFIXES: &[&str] = &["x-auth-", "x-ratelimit-", "x-envoy-"];

pub(crate) struct RuntimeState {
    pub upstream_headers: HashMap<String, String>,
    pub response_headers: HashMap<String, String>,
    pub short_circuit: Option<PluginExit>,
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            upstream_headers: HashMap::new(),
            response_headers: HashMap::new(),
            short_circuit: None,
        }
    }
}

pub(crate) fn is_exit_error(error: &mlua::Error) -> bool {
    matches!(error, mlua::Error::RuntimeError(message) if message == EXIT_SENTINEL)
}

pub(crate) fn install_gateway(
    lua: &Lua,
    request: &PluginRequest<'_>,
    binding: &PluginBinding,
    shared_ctx: Table,
    runtime: Rc<RefCell<RuntimeState>>,
    response_status: Option<u16>,
) -> LuaResult<Table> {
    let gateway = lua.create_table()?;
    gateway.set("request", build_request_api(lua, request)?)?;
    gateway.set(
        "response",
        build_response_api(lua, runtime.clone(), binding.name.clone())?,
    )?;
    gateway.set("service", build_service_api(lua, runtime.clone())?)?;
    gateway.set("log", build_log_api(lua)?)?;
    gateway.set("ctx", build_ctx_api(lua, shared_ctx, response_status)?)?;
    gateway.set("route", build_route_api(lua, request)?)?;
    gateway.set("plugin", build_plugin_api(lua, binding)?)?;
    lua.globals().set("gateway", gateway.clone())?;
    Ok(gateway)
}

fn build_request_api(lua: &Lua, request: &PluginRequest<'_>) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let headers = request.headers.clone();
    table.set(
        "get_header",
        lua.create_function(move |_, name: String| {
            Ok(headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value.clone()))
        })?,
    )?;
    let method = request.method.to_owned();
    table.set(
        "get_method",
        lua.create_function(move |_, ()| Ok(method.clone()))?,
    )?;
    let path = request.path.to_owned();
    table.set(
        "get_path",
        lua.create_function(move |_, ()| Ok(path.clone()))?,
    )?;
    let host = request.host.to_owned();
    table.set(
        "get_host",
        lua.create_function(move |_, ()| Ok(host.clone()))?,
    )?;
    Ok(table)
}

fn build_response_api(
    lua: &Lua,
    runtime: Rc<RefCell<RuntimeState>>,
    plugin_name: String,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let runtime_for_headers = runtime.clone();
    table.set(
        "set_header",
        lua.create_function(move |_, (name, value): (String, String)| {
            runtime_for_headers
                .borrow_mut()
                .response_headers
                .insert(name.to_ascii_lowercase(), value);
            Ok(())
        })?,
    )?;
    table.set(
        "exit",
        lua.create_function(move |_, (status, body): (u16, Option<String>)| {
            runtime.borrow_mut().short_circuit = Some(PluginExit {
                status,
                body,
                plugin_name: plugin_name.clone(),
            });
            Err::<(), _>(mlua::Error::RuntimeError(EXIT_SENTINEL.to_owned()))
        })?,
    )?;
    Ok(table)
}

fn build_service_api(lua: &Lua, runtime: Rc<RefCell<RuntimeState>>) -> LuaResult<Table> {
    let service = lua.create_table()?;
    let request = lua.create_table()?;
    request.set(
        "set_header",
        lua.create_function(move |_, (name, value): (String, String)| {
            let lower = name.to_ascii_lowercase();
            if RESERVED_HEADER_PREFIXES
                .iter()
                .any(|prefix| lower.starts_with(prefix))
            {
                return Err(mlua::Error::RuntimeError(format!(
                    "header '{name}' is reserved and cannot be set by plugins"
                )));
            }
            runtime.borrow_mut().upstream_headers.insert(lower, value);
            Ok(())
        })?,
    )?;
    service.set("request", request)?;
    Ok(service)
}

fn build_log_api(lua: &Lua) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for level in ["debug", "info", "warn", "error"] {
        let level_name = level.to_owned();
        table.set(
            level,
            lua.create_function(move |_, message: String| {
                match level_name.as_str() {
                    "debug" => tracing::debug!(target: "plugins", "{message}"),
                    "info" => tracing::info!(target: "plugins", "{message}"),
                    "warn" => tracing::warn!(target: "plugins", "{message}"),
                    _ => tracing::error!(target: "plugins", "{message}"),
                }
                Ok(())
            })?,
        )?;
    }
    Ok(table)
}

fn build_ctx_api(lua: &Lua, shared_ctx: Table, response_status: Option<u16>) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("shared", shared_ctx)?;
    if let Some(status) = response_status {
        table.set("response_status", status)?;
    }
    Ok(table)
}

fn build_route_api(lua: &Lua, request: &PluginRequest<'_>) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let route_name = request.route_name.to_owned();
    table.set(
        "get_name",
        lua.create_function(move |_, ()| Ok(route_name.clone()))?,
    )?;
    Ok(table)
}

fn build_plugin_api(lua: &Lua, binding: &PluginBinding) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let name = binding.name.clone();
    table.set(
        "get_name",
        lua.create_function(move |_, ()| Ok(name.clone()))?,
    )?;
    let version = binding.version.clone();
    table.set(
        "get_version",
        lua.create_function(move |_, ()| Ok(version.clone()))?,
    )?;
    table.set("get_config", lua.to_value(&binding.config)?)?;
    Ok(table)
}

pub(crate) fn clear_gateway(lua: &Lua) -> LuaResult<()> {
    lua.globals().set("gateway", Value::Nil)
}

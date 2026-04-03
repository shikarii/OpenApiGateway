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
    path: String,
    query_params: Vec<(String, String)>,
    path_dirty: bool,
}

impl RuntimeState {
    pub fn new(request: &PluginRequest<'_>) -> Self {
        Self {
            upstream_headers: HashMap::new(),
            response_headers: HashMap::new(),
            short_circuit: None,
            path: request.path.to_owned(),
            query_params: request.query_params.clone(),
            path_dirty: false,
        }
    }

    fn set_path(&mut self, path: String) {
        if let Some((next_path, raw_query)) = path.split_once('?') {
            self.path = normalize_path(next_path);
            self.query_params = parse_query_params(raw_query);
        } else {
            self.path = normalize_path(&path);
        }
        self.path_dirty = true;
        self.sync_path_header();
    }

    fn set_query_param(&mut self, name: String, value: String) {
        if let Some((_, current_value)) = self
            .query_params
            .iter_mut()
            .find(|(current_name, _)| current_name == &name)
        {
            *current_value = value;
        } else {
            self.query_params.push((name, value));
        }
        self.path_dirty = true;
        self.sync_path_header();
    }

    fn remove_query_param(&mut self, name: &str) {
        self.query_params
            .retain(|(current_name, _)| current_name != name);
        self.path_dirty = true;
        self.sync_path_header();
    }

    fn sync_path_header(&mut self) {
        if !self.path_dirty {
            self.upstream_headers.remove(":path");
            return;
        }

        let mut rewritten = self.path.clone();
        if !self.query_params.is_empty() {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (name, value) in &self.query_params {
                serializer.append_pair(name, value);
            }
            rewritten.push('?');
            rewritten.push_str(&serializer.finish());
        }
        self.upstream_headers.insert(":path".to_owned(), rewritten);
    }
}

pub(crate) fn is_exit_error(error: &mlua::Error) -> bool {
    match error {
        mlua::Error::RuntimeError(message) => message.contains(EXIT_SENTINEL),
        mlua::Error::CallbackError { cause, .. } => is_exit_error(cause),
        _ => false,
    }
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
    let query_params = request.query_params.clone();
    table.set(
        "get_query",
        lua.create_function(move |_, name: String| {
            Ok(query_params
                .iter()
                .find(|(key, _)| key == &name)
                .map(|(_, value)| value.clone()))
        })?,
    )?;
    let host = request.host.to_owned();
    table.set(
        "get_host",
        lua.create_function(move |_, ()| Ok(host.clone()))?,
    )?;
    let client_ip = request.client_ip.to_owned();
    table.set(
        "get_client_ip",
        lua.create_function(move |_, ()| Ok(client_ip.clone()))?,
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
    let runtime_for_headers = runtime.clone();
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
            runtime_for_headers
                .borrow_mut()
                .upstream_headers
                .insert(lower, value);
            Ok(())
        })?,
    )?;
    let runtime_for_path = runtime.clone();
    request.set(
        "set_path",
        lua.create_function(move |_, path: String| {
            runtime_for_path.borrow_mut().set_path(path);
            Ok(())
        })?,
    )?;
    let runtime_for_set_query = runtime.clone();
    request.set(
        "set_query_param",
        lua.create_function(move |_, (name, value): (String, String)| {
            runtime_for_set_query
                .borrow_mut()
                .set_query_param(name, value);
            Ok(())
        })?,
    )?;
    request.set(
        "remove_query_param",
        lua.create_function(move |_, name: String| {
            runtime.borrow_mut().remove_query_param(&name);
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
    let config = lua.to_value(&binding.config)?;
    table.set(
        "get_config",
        lua.create_function(move |_, ()| Ok(config.clone()))?,
    )?;
    Ok(table)
}

pub(crate) fn clear_gateway(lua: &Lua) -> LuaResult<()> {
    lua.globals().set("gateway", Value::Nil)
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_owned()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

fn parse_query_params(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

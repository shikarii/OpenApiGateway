use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mlua::{LuaSerdeExt, Table};
use serde_json::Value as JsonValue;
use shared::config_types::{GatewayConfig, PluginInstance};

use super::sandbox::create_sandboxed_vm;
use super::types::{PluginBinding, PluginChain, PluginError, PluginMeta, PluginRuntime};

pub(crate) fn build_runtime(
    config: &GatewayConfig,
    generation: u64,
) -> Result<PluginRuntime, PluginError> {
    if !config.plugins.enabled {
        return Ok(PluginRuntime {
            generation,
            limits: config.plugins.limits.clone(),
            chains: HashMap::new(),
        });
    }

    let directory = Path::new(&config.plugins.directory);
    if !directory.exists() {
        return Err(PluginError::MissingDirectory(
            directory.display().to_string(),
        ));
    }

    let registry = discover_plugins(directory, &config.plugins.limits)?;
    let mut chains = HashMap::new();

    for route in &config.routes {
        let mut bindings = Vec::new();
        push_bindings(
            &mut bindings,
            route.name.as_str(),
            &config.plugins.global,
            &registry,
        )?;
        push_bindings(
            &mut bindings,
            route.name.as_str(),
            &route.plugins,
            &registry,
        )?;
        bindings.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.name.cmp(&right.name))
        });
        chains.insert(route.name.clone(), PluginChain { bindings });
    }

    Ok(PluginRuntime {
        generation,
        limits: config.plugins.limits.clone(),
        chains,
    })
}

fn discover_plugins(
    directory: &Path,
    limits: &shared::config_types::PluginLimits,
) -> Result<HashMap<String, PluginMeta>, PluginError> {
    let mut plugins = HashMap::new();

    for entry in std::fs::read_dir(directory).map_err(|e| PluginError::Load {
        name: directory.display().to_string(),
        reason: e.to_string(),
    })? {
        let entry = entry.map_err(|e| PluginError::Load {
            name: directory.display().to_string(),
            reason: e.to_string(),
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("lua") {
            continue;
        }

        let source = std::fs::read_to_string(&path).map_err(|e| PluginError::Load {
            name: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| PluginError::Load {
                name: path.display().to_string(),
                reason: "invalid UTF-8 file name".to_owned(),
            })?
            .to_owned();

        let meta = extract_meta(&name, &path, source, limits)?;
        plugins.insert(name, meta);
    }

    Ok(plugins)
}

fn extract_meta(
    name: &str,
    _path: &PathBuf,
    source: String,
    limits: &shared::config_types::PluginLimits,
) -> Result<PluginMeta, PluginError> {
    let lua = create_sandboxed_vm(limits).map_err(|e| PluginError::Load {
        name: name.to_owned(),
        reason: e.to_string(),
    })?;
    let plugin: Table = lua
        .load(&source)
        .set_name(name)
        .eval()
        .map_err(|e| PluginError::Load {
            name: name.to_owned(),
            reason: e.to_string(),
        })?;
    let version: String = plugin.get("VERSION").map_err(|e| PluginError::Load {
        name: name.to_owned(),
        reason: format!("missing VERSION: {e}"),
    })?;
    let priority: i64 = plugin.get("PRIORITY").map_err(|e| PluginError::Load {
        name: name.to_owned(),
        reason: format!("missing PRIORITY: {e}"),
    })?;
    let schema = plugin
        .get::<mlua::Value>("SCHEMA")
        .ok()
        .and_then(|value| lua.from_value(value).ok());

    Ok(PluginMeta {
        name: name.to_owned(),
        priority,
        version,
        source: Arc::new(source),
        schema,
    })
}

fn push_bindings(
    bindings: &mut Vec<PluginBinding>,
    route_name: &str,
    configured: &[PluginInstance],
    registry: &HashMap<String, PluginMeta>,
) -> Result<(), PluginError> {
    for instance in configured.iter().filter(|plugin| plugin.enabled) {
        let meta = registry
            .get(&instance.name)
            .ok_or_else(|| PluginError::MissingPlugin(instance.name.clone()))?;
        validate_schema(meta, instance)?;
        bindings.push(PluginBinding {
            id: format!("{route_name}:{}", instance.name),
            name: meta.name.clone(),
            priority: meta.priority,
            version: meta.version.clone(),
            fail_open: instance.fail_mode == "open",
            source: Arc::clone(&meta.source),
            config: serde_json::to_value(&instance.config).unwrap_or(JsonValue::Null),
        });
    }
    Ok(())
}

fn validate_schema(meta: &PluginMeta, instance: &PluginInstance) -> Result<(), PluginError> {
    let Some(schema) = meta.schema.as_ref() else {
        return Ok(());
    };
    let compiled =
        jsonschema::JSONSchema::compile(schema).map_err(|e| PluginError::SchemaValidation {
            name: meta.name.clone(),
            reason: e.to_string(),
        })?;
    let instance_value = serde_json::to_value(&instance.config).unwrap_or(JsonValue::Null);
    if let Err(errors) = compiled.validate(&instance_value) {
        let reason = errors
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(PluginError::SchemaValidation {
            name: meta.name.clone(),
            reason,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

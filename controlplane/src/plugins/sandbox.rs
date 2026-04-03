use mlua::{Error as LuaError, HookTriggers, Lua, LuaSerdeExt, Result as LuaResult};
use shared::config_types::PluginLimits;

const REGISTRY_CJSON: &str = "__gateway_cjson";
const REGISTRY_CJSON_SAFE: &str = "__gateway_cjson_safe";

pub(crate) fn create_sandboxed_vm(limits: &PluginLimits) -> Result<Lua, LuaError> {
    let lua = unsafe { Lua::unsafe_new() };
    lua.set_memory_limit(limits.max_memory_bytes)?;
    sandbox_globals(&lua)?;
    preload_cjson(&lua)?;
    install_instruction_hook(&lua, limits.max_instructions)?;
    Ok(lua)
}

pub(crate) fn reset_limits(lua: &Lua, limits: &PluginLimits) -> Result<(), LuaError> {
    lua.set_memory_limit(limits.max_memory_bytes)?;
    install_instruction_hook(lua, limits.max_instructions)
}

fn sandbox_globals(lua: &Lua) -> LuaResult<()> {
    lua.load("if jit then jit.off() end").exec()?;

    let globals = lua.globals();
    for name in [
        "print",
        "load",
        "loadfile",
        "loadstring",
        "dofile",
        "collectgarbage",
        "newproxy",
        "_G",
    ] {
        globals.set(name, mlua::Value::Nil)?;
    }

    for name in ["io", "debug", "package", "ffi", "jit"] {
        globals.set(name, mlua::Value::Nil)?;
    }

    let os_table: mlua::Table = globals.get("os")?;
    let safe_os = lua.create_table()?;
    for name in ["clock", "time", "date", "difftime"] {
        safe_os.set(name, os_table.get::<mlua::Function>(name)?)?;
    }
    globals.set("os", safe_os)?;

    let string_table: mlua::Table = globals.get("string")?;
    string_table.set("dump", mlua::Value::Nil)?;

    let require = lua.create_function(|lua, name: String| match name.as_str() {
        "cjson" => lua.named_registry_value::<mlua::Table>(REGISTRY_CJSON),
        "cjson.safe" => lua.named_registry_value::<mlua::Table>(REGISTRY_CJSON_SAFE),
        _ => Err(LuaError::RuntimeError(format!(
            "module '{name}' is not available in sandbox"
        ))),
    })?;
    globals.set("require", require)?;
    Ok(())
}

fn preload_cjson(lua: &Lua) -> LuaResult<()> {
    let cjson = lua.create_table()?;
    let encode = lua.create_function(|_, value: mlua::Value| {
        serde_json::to_string(&value).map_err(|e| LuaError::RuntimeError(e.to_string()))
    })?;
    let decode = lua.create_function(|lua, input: String| {
        let value: serde_json::Value =
            serde_json::from_str(&input).map_err(|e| LuaError::RuntimeError(e.to_string()))?;
        lua.to_value(&value)
    })?;
    cjson.set("encode", encode)?;
    cjson.set("decode", decode)?;

    let safe = lua.create_table()?;
    let safe_decode = lua.create_function(|lua, input: String| {
        match serde_json::from_str::<serde_json::Value>(&input) {
            Ok(value) => Ok((Some(lua.to_value(&value)?), Option::<String>::None)),
            Err(e) => Ok((Option::<mlua::Value>::None, Some(e.to_string()))),
        }
    })?;
    safe.set("encode", cjson.get::<mlua::Function>("encode")?)?;
    safe.set("decode", safe_decode)?;

    lua.set_named_registry_value(REGISTRY_CJSON, cjson)?;
    lua.set_named_registry_value(REGISTRY_CJSON_SAFE, safe)?;
    Ok(())
}

fn install_instruction_hook(lua: &Lua, max_instructions: u32) -> LuaResult<()> {
    #[allow(clippy::needless_update)]
    let triggers = HookTriggers {
        every_nth_instruction: Some(max_instructions),
        ..HookTriggers::default()
    };
    lua.set_hook(triggers, move |_lua, _debug| {
        Err(LuaError::RuntimeError(format!(
            "instruction limit exceeded ({max_instructions})"
        )))
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_limits() -> PluginLimits {
        PluginLimits {
            max_memory_bytes: 8 * 1024 * 1024,
            max_instructions: 1_000_000,
            chain_timeout_ms: 50,
        }
    }

    #[test]
    fn dangerous_globals_are_removed() {
        let lua = create_sandboxed_vm(&test_limits()).unwrap();
        for name in ["io", "debug", "package", "ffi", "jit", "_G", "loadstring"] {
            let value: mlua::Value = lua.globals().get(name).unwrap();
            assert!(matches!(value, mlua::Value::Nil), "{name} should be nil");
        }
    }

    #[test]
    fn sandboxed_require_only_allows_cjson() {
        let lua = create_sandboxed_vm(&test_limits()).unwrap();
        let cjson: mlua::Table = lua.load(r#"return require("cjson")"#).eval().unwrap();
        let encode: mlua::Function = cjson.get("encode").unwrap();
        let value = lua.to_value(&serde_json::json!({"ok": true})).unwrap();
        let encoded: String = encode.call(value).unwrap();
        assert!(encoded.contains("ok"));

        let err = lua.load(r#"return require("io")"#).eval::<mlua::Value>();
        assert!(err.is_err());
    }

    #[test]
    fn safe_os_and_string_dump_rules_apply() {
        let lua = create_sandboxed_vm(&test_limits()).unwrap();
        let ok: bool = lua
            .load(
                r#"
                return os.execute == nil
                    and os.clock() >= 0
                    and string.dump == nil
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn memory_limit_triggers_error() {
        let lua = create_sandboxed_vm(&test_limits()).unwrap();
        let err = lua
            .load(r#"return string.rep("x", 2^25)"#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("memory"));
    }

    #[test]
    fn instruction_limit_triggers_error() {
        let lua = create_sandboxed_vm(&test_limits()).unwrap();
        let err = lua.load("while true do end").exec().unwrap_err();
        assert!(err.to_string().contains("instruction limit exceeded"));
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

use mlua::{Lua, RegistryKey};
use shared::config_types::PluginLimits;

use super::sandbox::{create_sandboxed_vm, reset_limits};
use super::types::PluginError;

pub(crate) struct ThreadLocalVm {
    lua: Lua,
    generation: u64,
    instances: HashMap<String, RegistryKey>,
}

impl ThreadLocalVm {
    fn new() -> Result<Self, PluginError> {
        let limits = PluginLimits::default();
        let lua = create_sandboxed_vm(&limits).map_err(|e| PluginError::Load {
            name: "sandbox".to_owned(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            lua,
            generation: 0,
            instances: HashMap::new(),
        })
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    pub fn prepare(&mut self, generation: u64, limits: &PluginLimits) -> Result<(), PluginError> {
        if self.generation != generation {
            self.generation = generation;
            self.instances.clear();
        }
        reset_limits(&self.lua, limits).map_err(|e| PluginError::Load {
            name: "sandbox".to_owned(),
            reason: e.to_string(),
        })
    }

    pub fn registry_key(&self, binding_id: &str) -> Option<&RegistryKey> {
        self.instances.get(binding_id)
    }

    pub fn insert_registry_key(&mut self, binding_id: &str, key: RegistryKey) {
        self.instances.insert(binding_id.to_owned(), key);
    }
}

thread_local! {
    static LUA_VM: RefCell<Result<ThreadLocalVm, PluginError>> = RefCell::new(ThreadLocalVm::new());
}

pub(crate) fn with_thread_local_vm<R>(
    generation: u64,
    limits: &PluginLimits,
    f: impl FnOnce(&mut ThreadLocalVm) -> Result<R, PluginError>,
) -> Result<R, PluginError> {
    LUA_VM.with(|cell| {
        let mut slot = cell.borrow_mut();
        let vm = slot.as_mut().map_err(|err| PluginError::Load {
            name: "sandbox".to_owned(),
            reason: err.to_string(),
        })?;
        vm.prepare(generation, limits)?;
        f(vm)
    })
}

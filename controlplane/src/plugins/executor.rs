use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use mlua::{Function, LuaSerdeExt, Table};

use super::pool::{with_thread_local_vm, ThreadLocalVm};
use super::sdk::{clear_gateway, install_gateway, is_exit_error, RuntimeState};
use super::types::{
    PluginAccessResult, PluginBinding, PluginEngine, PluginError, PluginErrorType,
    PluginInvocationOutcome, PluginRequest,
};

impl PluginEngine {
    pub async fn execute_access(
        &self,
        request: &PluginRequest<'_>,
    ) -> Result<PluginAccessResult, PluginError> {
        let runtime = self.runtime().await;
        let Some(chain) = runtime.chains.get(request.route_name) else {
            return Ok(PluginAccessResult::Continue {
                upstream_headers: Default::default(),
                response_headers: Default::default(),
                chain_duration_us: 0,
            });
        };
        let limits = runtime.limits.clone();
        let generation = runtime.generation;
        let bindings = chain.bindings.clone();
        let started = Instant::now();

        with_thread_local_vm(generation, &limits, |vm| {
            let shared_ctx = vm.lua().create_table().map_err(|e| PluginError::Runtime {
                name: "ctx".to_owned(),
                reason: e.to_string(),
            })?;
            let runtime_state = Rc::new(RefCell::new(RuntimeState::new()));

            for binding in &bindings {
                if started.elapsed().as_millis() as u64 > limits.chain_timeout_ms {
                    return Ok(PluginAccessResult::Error {
                        plugin_name: binding.name.clone(),
                        message: format!("plugin chain exceeded {} ms", limits.chain_timeout_ms),
                        error_type: PluginErrorType::Timeout,
                        chain_duration_us: started.elapsed().as_micros() as u64,
                    });
                }

                let outcome = match run_phase(
                    vm,
                    binding,
                    request,
                    shared_ctx.clone(),
                    runtime_state.clone(),
                    "access",
                    None,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        let error_type = classify_error(&error);
                        return Ok(PluginAccessResult::Error {
                            plugin_name: binding.name.clone(),
                            message: error.to_string(),
                            error_type,
                            chain_duration_us: started.elapsed().as_micros() as u64,
                        });
                    }
                };

                if let Some(exit) = outcome.short_circuit {
                    let headers = runtime_state.borrow().response_headers.clone();
                    return Ok(PluginAccessResult::ShortCircuit {
                        plugin_name: exit.plugin_name,
                        status: exit.status,
                        body: exit.body,
                        headers,
                        chain_duration_us: started.elapsed().as_micros() as u64,
                    });
                }
            }

            let (upstream_headers, response_headers) = {
                let state = runtime_state.borrow();
                (
                    state.upstream_headers.clone(),
                    state.response_headers.clone(),
                )
            };
            Ok(PluginAccessResult::Continue {
                upstream_headers,
                response_headers,
                chain_duration_us: started.elapsed().as_micros() as u64,
            })
        })
    }

    pub async fn execute_log(
        &self,
        request: &PluginRequest<'_>,
        response_status: u16,
    ) -> Result<(), PluginError> {
        let runtime = self.runtime().await;
        let Some(chain) = runtime.chains.get(request.route_name) else {
            return Ok(());
        };
        let limits = runtime.limits.clone();
        let generation = runtime.generation;
        let mut bindings = chain.bindings.clone();
        bindings.reverse();

        with_thread_local_vm(generation, &limits, |vm| {
            let shared_ctx = vm.lua().create_table().map_err(|e| PluginError::Runtime {
                name: "ctx".to_owned(),
                reason: e.to_string(),
            })?;
            let runtime_state = Rc::new(RefCell::new(RuntimeState::new()));

            for binding in &bindings {
                let _ = run_phase(
                    vm,
                    binding,
                    request,
                    shared_ctx.clone(),
                    runtime_state.clone(),
                    "log",
                    Some(response_status),
                );
            }
            Ok(())
        })
    }
}

fn classify_error(error: &PluginError) -> PluginErrorType {
    match error {
        PluginError::Runtime { reason, .. } if reason.to_ascii_lowercase().contains("memory") => {
            PluginErrorType::Memory
        }
        _ => PluginErrorType::Runtime,
    }
}

fn run_phase(
    vm: &mut ThreadLocalVm,
    binding: &PluginBinding,
    request: &PluginRequest<'_>,
    shared_ctx: Table,
    runtime_state: Rc<RefCell<RuntimeState>>,
    phase: &str,
    response_status: Option<u16>,
) -> Result<PluginInvocationOutcome, PluginError> {
    let instance = plugin_instance(vm, binding)?;
    let lua = vm.lua();
    install_gateway(
        lua,
        request,
        binding,
        shared_ctx.clone(),
        runtime_state.clone(),
        response_status,
    )
    .map_err(|e| PluginError::Runtime {
        name: binding.name.clone(),
        reason: e.to_string(),
    })?;
    let ctx = lua.create_table().map_err(|e| PluginError::Runtime {
        name: binding.name.clone(),
        reason: e.to_string(),
    })?;
    ctx.set("shared", shared_ctx)
        .map_err(|e| PluginError::Runtime {
            name: binding.name.clone(),
            reason: e.to_string(),
        })?;
    if let Some(status) = response_status {
        ctx.set("response_status", status)
            .map_err(|e| PluginError::Runtime {
                name: binding.name.clone(),
                reason: e.to_string(),
            })?;
    }

    let phase_fn = match instance.get::<Function>(phase) {
        Ok(function) => function,
        Err(_) => {
            clear_gateway(lua).ok();
            return Ok(PluginInvocationOutcome {
                short_circuit: None,
            });
        }
    };

    let call_result = phase_fn.call::<()>((instance.clone(), ctx));
    clear_gateway(lua).ok();

    match call_result {
        Ok(()) => Ok(PluginInvocationOutcome {
            short_circuit: runtime_state.borrow().short_circuit.clone(),
        }),
        Err(error) if is_exit_error(&error) => Ok(PluginInvocationOutcome {
            short_circuit: runtime_state.borrow().short_circuit.clone(),
        }),
        Err(error) if binding.fail_open => {
            tracing::warn!(
                plugin = %binding.name,
                phase,
                error = %error,
                "plugin failed open"
            );
            Ok(PluginInvocationOutcome {
                short_circuit: None,
            })
        }
        Err(error) => Err(PluginError::Runtime {
            name: binding.name.clone(),
            reason: error.to_string(),
        }),
    }
}

fn plugin_instance(vm: &mut ThreadLocalVm, binding: &PluginBinding) -> Result<Table, PluginError> {
    let lua = vm.lua();
    if let Some(key) = vm.registry_key(&binding.id) {
        return lua.registry_value(key).map_err(|e| PluginError::Load {
            name: binding.name.clone(),
            reason: e.to_string(),
        });
    }

    let plugin: Table = lua
        .load(binding.source.as_str())
        .set_name(&binding.name)
        .eval()
        .map_err(|e| PluginError::Load {
            name: binding.name.clone(),
            reason: e.to_string(),
        })?;

    let instance = match plugin.get::<Function>("new") {
        Ok(constructor) => constructor
            .call::<Table>((
                plugin.clone(),
                lua.to_value(&binding.config)
                    .map_err(|e| PluginError::Load {
                        name: binding.name.clone(),
                        reason: e.to_string(),
                    })?,
            ))
            .map_err(|e| PluginError::Load {
                name: binding.name.clone(),
                reason: e.to_string(),
            })?,
        Err(_) => plugin,
    };

    let key = lua
        .create_registry_value(instance.clone())
        .map_err(|e| PluginError::Load {
            name: binding.name.clone(),
            reason: e.to_string(),
        })?;
    vm.insert_registry_key(&binding.id, key);
    Ok(instance)
}

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value as JsonValue;
use shared::config_types::PluginLimits;

/// Shared engine state swapped atomically on reload.
#[derive(Debug, Clone)]
pub(crate) struct PluginRuntime {
    pub generation: u64,
    pub limits: PluginLimits,
    pub chains: HashMap<String, PluginChain>,
}

/// Ordered plugin chain for one route.
#[derive(Debug, Clone, Default)]
pub(crate) struct PluginChain {
    pub bindings: Vec<PluginBinding>,
}

/// One configured plugin bound to a route.
#[derive(Debug, Clone)]
pub(crate) struct PluginBinding {
    pub id: String,
    pub name: String,
    pub priority: i64,
    pub version: String,
    pub fail_open: bool,
    pub source: Arc<String>,
    pub config: JsonValue,
}

/// Metadata discovered from a Lua plugin file.
#[derive(Debug, Clone)]
pub(crate) struct PluginMeta {
    pub name: String,
    pub priority: i64,
    pub version: String,
    pub source: Arc<String>,
    pub schema: Option<JsonValue>,
}

/// Request view exposed to the plugin engine.
#[derive(Debug, Clone)]
pub(crate) struct PluginRequest<'a> {
    pub route_name: &'a str,
    pub host: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub headers: Vec<(String, String)>,
}

/// Result of the access phase.
#[derive(Debug, Clone)]
pub(crate) enum PluginAccessResult {
    Continue {
        upstream_headers: HashMap<String, String>,
        response_headers: HashMap<String, String>,
        chain_duration_us: u64,
    },
    ShortCircuit {
        plugin_name: String,
        status: u16,
        body: Option<String>,
        headers: HashMap<String, String>,
        chain_duration_us: u64,
    },
    Error {
        plugin_name: String,
        message: String,
        error_type: PluginErrorType,
        chain_duration_us: u64,
    },
}

/// Result of a single access or log invocation.
#[derive(Debug, Clone)]
pub(crate) struct PluginInvocationOutcome {
    pub short_circuit: Option<PluginExit>,
}

/// Short-circuit response requested by a plugin.
#[derive(Debug, Clone)]
pub(crate) struct PluginExit {
    pub status: u16,
    pub body: Option<String>,
    pub plugin_name: String,
}

/// Classified plugin runtime failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginErrorType {
    Runtime,
    Memory,
    Timeout,
}

impl PluginErrorType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Memory => "memory",
            Self::Timeout => "timeout",
        }
    }
}

/// Errors during engine load or execution.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginError {
    #[error("plugin directory '{0}' does not exist")]
    MissingDirectory(String),
    #[error("plugin '{0}' is configured but no Lua file was found")]
    MissingPlugin(String),
    #[error("plugin '{name}' failed schema validation: {reason}")]
    SchemaValidation { name: String, reason: String },
    #[error("plugin '{name}' failed to load: {reason}")]
    Load { name: String, reason: String },
    #[error("plugin '{name}' runtime error: {reason}")]
    Runtime { name: String, reason: String },
}

/// Mutable engine holder shared across handlers.
pub(crate) struct PluginEngine {
    runtime: Arc<tokio::sync::RwLock<PluginRuntime>>,
    next_generation: AtomicU64,
}

impl std::fmt::Debug for PluginEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginEngine").finish_non_exhaustive()
    }
}

impl PluginEngine {
    pub fn new(runtime: PluginRuntime) -> Self {
        let generation = runtime.generation;
        Self {
            runtime: Arc::new(tokio::sync::RwLock::new(runtime)),
            next_generation: AtomicU64::new(generation + 1),
        }
    }

    pub fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn runtime(&self) -> PluginRuntime {
        self.runtime.read().await.clone()
    }

    pub async fn replace_runtime(&self, runtime: PluginRuntime) {
        *self.runtime.write().await = runtime;
    }
}

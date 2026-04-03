mod executor;
mod pool;
mod registry;
mod sandbox;
mod sdk;
mod types;

use shared::config_types::GatewayConfig;

pub(crate) use types::{
    PluginAccessResult, PluginEngine, PluginError, PluginErrorType, PluginRequest,
};

impl PluginEngine {
    pub fn from_config(config: &GatewayConfig) -> Result<Self, PluginError> {
        let runtime = registry::build_runtime(config, 1)?;
        Ok(Self::new(runtime))
    }

    pub async fn reload_from_config(&self, config: &GatewayConfig) -> Result<(), PluginError> {
        let generation = self.next_generation();
        let runtime = registry::build_runtime(config, generation)?;
        self.replace_runtime(runtime).await;
        Ok(())
    }
}

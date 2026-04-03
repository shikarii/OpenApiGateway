use std::path::PathBuf;

use serde_yaml::Value as YamlValue;
use shared::config_types::PluginInstance;

use super::*;

fn plugin_dir() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins")
        .to_string_lossy()
        .into_owned()
}

fn sample_config() -> shared::config_types::GatewayConfig {
    crate::config::load_config_from_str(include_str!(
        "../../../examples/configs/gateway-single-node.yaml"
    ))
    .unwrap()
}

fn plugin_instance(name: &str, config: YamlValue) -> PluginInstance {
    PluginInstance {
        name: name.to_owned(),
        enabled: true,
        fail_mode: "closed".to_owned(),
        config,
    }
}

#[test]
fn build_runtime_loads_bundled_plugins_in_priority_order() {
    let mut config = sample_config();
    config.plugins.enabled = true;
    config.plugins.directory = plugin_dir();
    config.routes[0].plugins = vec![
        plugin_instance(
            "request-transformer",
            serde_yaml::from_str(
                r#"
add_headers:
  x-added: "true"
"#,
            )
            .unwrap(),
        ),
        plugin_instance(
            "cors",
            serde_yaml::from_str(
                r#"
origins: ["*"]
methods: ["GET", "POST"]
"#,
            )
            .unwrap(),
        ),
    ];

    let runtime = build_runtime(&config, 7).unwrap();
    let bindings = &runtime.chains["public-echo"].bindings;

    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].name, "cors");
    assert_eq!(bindings[1].name, "request-transformer");
    assert_eq!(runtime.generation, 7);
}

#[test]
fn build_runtime_rejects_invalid_bundled_plugin_config() {
    let mut config = sample_config();
    config.plugins.enabled = true;
    config.plugins.directory = plugin_dir();
    config.routes[0].plugins = vec![plugin_instance(
        "request-size-limiting",
        serde_yaml::from_str("{}").unwrap(),
    )];

    let error = build_runtime(&config, 1).unwrap_err();
    match error {
        PluginError::SchemaValidation { name, reason } => {
            assert_eq!(name, "request-size-limiting");
            assert!(reason.contains("max_bytes"));
        }
        other => panic!("expected schema validation error, got {other:?}"),
    }
}

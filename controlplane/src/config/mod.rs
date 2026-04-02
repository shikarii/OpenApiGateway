mod loader;
mod validation;

#[allow(dead_code)]
mod envoy_ext_authz;
#[allow(dead_code)]
mod envoy_gen;

#[allow(dead_code)]
pub(crate) use envoy_gen::generate_envoy_config;
pub(crate) use loader::load_config_from_str;

#[cfg(test)]
#[path = "route_matching_tests.rs"]
mod route_matching_tests;

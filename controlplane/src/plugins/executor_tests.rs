use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use shared::config_types::PluginLimits;

use super::*;
use crate::plugins::types::{PluginBinding, PluginChain, PluginRuntime};

fn test_engine(name: &str, source: &str, config: serde_json::Value) -> PluginEngine {
    let runtime = PluginRuntime {
        generation: 1,
        limits: PluginLimits::default(),
        chains: HashMap::from([(
            "route".to_owned(),
            PluginChain {
                bindings: vec![PluginBinding {
                    id: format!("route:{name}"),
                    name: name.to_owned(),
                    priority: 100,
                    version: "0.1.0".to_owned(),
                    fail_open: false,
                    source: Arc::new(source.to_owned()),
                    config,
                }],
            },
        )]),
    };
    PluginEngine::new(runtime)
}

fn request(
    method: &'static str,
    path: &'static str,
    client_ip: &'static str,
    headers: &[(&str, &str)],
    query_params: &[(&str, &str)],
) -> crate::plugins::PluginRequest<'static> {
    crate::plugins::PluginRequest {
        route_name: "route",
        host: "example.com",
        method,
        path,
        client_ip,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        query_params: query_params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

#[tokio::test]
async fn api_key_auth_allows_query_credentials_and_strips_them_from_upstream_path() {
    let engine = test_engine(
        "api-key-auth",
        include_str!("../../../plugins/api-key-auth.lua"),
        json!({
            "query_name": "apikey",
            "hide_credentials": true,
            "keys": [
                { "key": "secret", "consumer": "consumer-123" }
            ]
        }),
    );

    let result = engine
        .execute_access(&request(
            "GET",
            "/public/echo",
            "10.0.0.4",
            &[("host", "example.com")],
            &[("apikey", "secret"), ("page", "2")],
        ))
        .await
        .unwrap();

    match result {
        PluginAccessResult::Continue {
            upstream_headers, ..
        } => {
            assert_eq!(
                upstream_headers.get("x-consumer-id"),
                Some(&"consumer-123".to_owned())
            );
            assert_eq!(
                upstream_headers.get(":path"),
                Some(&"/public/echo?page=2".to_owned())
            );
        }
        other => panic!("expected continue result, got {other:?}"),
    }
}

#[tokio::test]
async fn basic_auth_accepts_valid_credentials() {
    let engine = test_engine(
        "basic-auth",
        include_str!("../../../plugins/basic-auth.lua"),
        json!({
            "credentials": [
                { "username": "user", "password": "pass", "consumer": "consumer-42" }
            ]
        }),
    );

    let result = engine
        .execute_access(&request(
            "GET",
            "/private",
            "10.0.0.5",
            &[("authorization", "Basic dXNlcjpwYXNz")],
            &[],
        ))
        .await
        .unwrap();

    match result {
        PluginAccessResult::Continue {
            upstream_headers, ..
        } => {
            assert_eq!(
                upstream_headers.get("x-consumer-id"),
                Some(&"consumer-42".to_owned())
            );
            assert_eq!(
                upstream_headers.get("x-consumer-username"),
                Some(&"user".to_owned())
            );
        }
        other => panic!("expected continue result, got {other:?}"),
    }
}

#[tokio::test]
async fn cors_short_circuits_preflight_requests() {
    let engine = test_engine(
        "cors",
        include_str!("../../../plugins/cors.lua"),
        json!({
            "origins": ["https://app.example.com"],
            "methods": ["GET", "POST"],
            "headers": ["authorization", "content-type"],
            "max_age": 300,
            "credentials": true
        }),
    );

    let result = engine
        .execute_access(&request(
            "OPTIONS",
            "/public/echo",
            "10.0.0.6",
            &[
                ("origin", "https://app.example.com"),
                ("access-control-request-method", "POST"),
            ],
            &[],
        ))
        .await
        .unwrap();

    match result {
        PluginAccessResult::ShortCircuit {
            status, headers, ..
        } => {
            assert_eq!(status, 204);
            assert_eq!(
                headers.get("access-control-allow-origin"),
                Some(&"https://app.example.com".to_owned())
            );
            assert_eq!(
                headers.get("access-control-allow-methods"),
                Some(&"GET, POST".to_owned())
            );
            assert_eq!(
                headers.get("access-control-max-age"),
                Some(&"300".to_owned())
            );
        }
        other => panic!("expected short circuit result, got {other:?}"),
    }
}

#[tokio::test]
async fn ip_restriction_blocks_denied_cidr_ranges() {
    let engine = test_engine(
        "ip-restriction",
        include_str!("../../../plugins/ip-restriction.lua"),
        json!({
            "deny": ["10.0.0.0/24"],
            "message": "blocked"
        }),
    );

    let result = engine
        .execute_access(&request("GET", "/private", "10.0.0.9", &[], &[]))
        .await
        .unwrap();

    match result {
        PluginAccessResult::ShortCircuit {
            status,
            body,
            headers,
            ..
        } => {
            assert_eq!(status, 403);
            assert_eq!(
                headers.get("content-type"),
                Some(&"application/json".to_owned())
            );
            assert!(body.unwrap_or_default().contains("blocked"));
        }
        other => panic!("expected short circuit result, got {other:?}"),
    }
}

#[tokio::test]
async fn request_size_limiting_rejects_large_payloads() {
    let engine = test_engine(
        "request-size-limiting",
        include_str!("../../../plugins/request-size-limiting.lua"),
        json!({
            "max_bytes": 8
        }),
    );

    let result = engine
        .execute_access(&request(
            "POST",
            "/upload",
            "10.0.0.10",
            &[("content-length", "32")],
            &[],
        ))
        .await
        .unwrap();

    match result {
        PluginAccessResult::ShortCircuit { status, body, .. } => {
            assert_eq!(status, 413);
            assert!(body.unwrap_or_default().contains("payload_too_large"));
        }
        other => panic!("expected short circuit result, got {other:?}"),
    }
}

#[tokio::test]
async fn request_transformer_rewrites_path_headers_and_query_params() {
    let engine = test_engine(
        "request-transformer",
        include_str!("../../../plugins/request-transformer.lua"),
        json!({
            "add_headers": {
                "x-added": "true"
            },
            "rename_headers": {
                "x-request-id": "x-original-request-id"
            },
            "set_query_params": {
                "version": "v2"
            },
            "remove_query_params": ["debug"],
            "rewrite_path": "/rewritten/resource"
        }),
    );

    let result = engine
        .execute_access(&request(
            "GET",
            "/public/resource",
            "10.0.0.11",
            &[("x-request-id", "req-123")],
            &[("debug", "1"), ("page", "3")],
        ))
        .await
        .unwrap();

    match result {
        PluginAccessResult::Continue {
            upstream_headers, ..
        } => {
            assert_eq!(upstream_headers.get("x-added"), Some(&"true".to_owned()));
            assert_eq!(
                upstream_headers.get("x-original-request-id"),
                Some(&"req-123".to_owned())
            );
            assert_eq!(
                upstream_headers.get(":path"),
                Some(&"/rewritten/resource?page=3&version=v2".to_owned())
            );
        }
        other => panic!("expected continue result, got {other:?}"),
    }
}

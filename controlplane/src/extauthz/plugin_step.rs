use std::collections::HashMap;

use crate::plugins::{PluginAccessResult, PluginErrorType, PluginRequest};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::helpers::{emit_log, insert_string_headers};
use crate::admin::state::SharedState;

pub(super) struct PluginContinue<'a> {
    pub request: PluginRequest<'a>,
    pub upstream_headers: HashMap<String, String>,
}

pub(super) struct PluginExecutionMeta<'a> {
    pub headers: &'a HeaderMap,
    pub host: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub request_id: &'a str,
    pub remote_addr: &'a str,
    pub start: &'a std::time::Instant,
}

pub(super) async fn execute_access_plugins<'a>(
    state: &SharedState,
    route: &'a shared::config_types::RouteConfig,
    meta: PluginExecutionMeta<'a>,
    response_headers: &mut HeaderMap,
) -> Result<PluginContinue<'a>, Response> {
    let request = PluginRequest {
        route_name: &route.name,
        host: meta.host,
        method: meta.method,
        path: meta.path,
        headers: meta
            .headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect(),
    };

    let Some(plugin_engine) = state.plugin_engine.as_ref() else {
        return Ok(PluginContinue {
            request,
            upstream_headers: HashMap::new(),
        });
    };

    match plugin_engine.execute_access(&request).await {
        Ok(PluginAccessResult::Continue {
            upstream_headers,
            response_headers: plugin_response_headers,
            chain_duration_us,
        }) => {
            state
                .metrics
                .record_plugin_chain_duration(&route.name, chain_duration_us);
            state.metrics.record_plugin_duration(
                "__chain__",
                &route.name,
                "access",
                chain_duration_us,
            );
            insert_string_headers(response_headers, &plugin_response_headers);
            Ok(PluginContinue {
                request,
                upstream_headers,
            })
        }
        Ok(PluginAccessResult::ShortCircuit {
            plugin_name,
            status,
            body,
            headers: plugin_headers,
            chain_duration_us,
        }) => {
            state
                .metrics
                .record_plugin_chain_duration(&route.name, chain_duration_us);
            state.metrics.record_plugin_duration(
                "__chain__",
                &route.name,
                "access",
                chain_duration_us,
            );
            state
                .metrics
                .record_plugin_short_circuit(&plugin_name, &route.name, status);
            let mut headers = HeaderMap::new();
            insert_string_headers(&mut headers, &plugin_headers);
            if let Ok(value) = meta.request_id.parse() {
                headers.insert("x-request-id", value);
            }
            emit_log(
                meta.request_id,
                meta.remote_addr,
                meta.host,
                meta.method,
                meta.path,
                &route.name,
                status,
                meta.start,
                None,
                "plugin-short-circuit",
                &route.upstream.service,
            );
            state.metrics.record_request(
                &route.name,
                meta.method,
                status,
                meta.start.elapsed().as_millis() as f64,
            );
            let response = match body {
                Some(body) => (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    headers,
                    body,
                )
                    .into_response(),
                None => (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    headers,
                )
                    .into_response(),
            };
            run_plugin_log(state, &request, status).await;
            Err(response)
        }
        Ok(PluginAccessResult::Error {
            plugin_name,
            message,
            error_type,
            chain_duration_us,
        }) => {
            state
                .metrics
                .record_plugin_chain_duration(&route.name, chain_duration_us);
            state.metrics.record_plugin_duration(
                "__chain__",
                &route.name,
                "access",
                chain_duration_us,
            );
            state
                .metrics
                .record_plugin_error(&plugin_name, &route.name, error_type.as_str());
            let mut headers = HeaderMap::new();
            if let Ok(value) = plugin_name.parse() {
                headers.insert("x-plugin-error", value);
            }
            if let Ok(value) = meta.request_id.parse() {
                headers.insert("x-request-id", value);
            }
            let status = StatusCode::INTERNAL_SERVER_ERROR;
            emit_log(
                meta.request_id,
                meta.remote_addr,
                meta.host,
                meta.method,
                meta.path,
                &route.name,
                status.as_u16(),
                meta.start,
                None,
                "plugin-error",
                &route.upstream.service,
            );
            state.metrics.record_request(
                &route.name,
                meta.method,
                status.as_u16(),
                meta.start.elapsed().as_millis() as f64,
            );
            let response = (
                status,
                headers,
                axum::Json(serde_json::json!({
                    "error": "plugin_error",
                    "plugin": plugin_name,
                    "message": message
                })),
            )
                .into_response();
            run_plugin_log(state, &request, status.as_u16()).await;
            Err(response)
        }
        Err(_) => {
            state.metrics.record_plugin_error(
                "engine",
                &route.name,
                PluginErrorType::Runtime.as_str(),
            );
            Err(axum::Json(serde_json::json!({ "error": "plugin_engine_error" })).into_response())
        }
    }
}

pub(super) async fn run_plugin_log(
    state: &SharedState,
    plugin_request: &PluginRequest<'_>,
    response_status: u16,
) {
    if let Some(plugin_engine) = state.plugin_engine.as_ref() {
        let _ = plugin_engine
            .execute_log(plugin_request, response_status)
            .await;
    }
}

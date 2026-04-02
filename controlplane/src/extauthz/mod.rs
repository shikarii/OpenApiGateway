// External authorization subsystem: HTTP ext_authz service for Envoy.
// Handles per-request auth validation, rate limiting, and overload protection.
mod handler;
mod helpers;

pub(crate) use handler::router;

#[cfg(test)]
#[path = "handler_tests.rs"]
mod handler_tests;

//! Narrow Tauri command gateway.

pub(crate) mod capture;
pub(crate) mod context;
pub(crate) mod library;
pub(crate) mod platform;

pub(crate) fn is_empty_input(input: &serde_json::Value) -> bool {
    input.as_object().is_some_and(serde_json::Map::is_empty)
}

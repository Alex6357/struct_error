//! Global registry for error types and united errors.
//!
//! Uses a process-wide `Mutex<HashMap>` to store metadata about `#[error]` and
//! `#[united_error]` definitions within the current compilation unit.
//!
//! # Limitations
//!
//! The registry is only valid within a single crate compilation. Types defined in
//! external crates will not be present, and will be treated as plain types.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Metadata for an atomic error type defined by `#[error]`.
pub struct ErrorMeta {
    /// The identifier of the error struct.
    pub _ident: String,
}

static ERROR_REGISTRY: OnceLock<Mutex<HashMap<String, ErrorMeta>>> = OnceLock::new();

static UNITED_REGISTRY: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();

fn error_registry() -> &'static Mutex<HashMap<String, ErrorMeta>> {
    ERROR_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn united_registry() -> &'static Mutex<HashMap<String, Vec<String>>> {
    UNITED_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registers an atomic error type.
pub fn register_error_type(ident: &str) {
    if let Ok(mut map) = error_registry().lock() {
        map.insert(
            ident.to_string(),
            ErrorMeta {
                _ident: ident.to_string(),
            },
        );
    }
}

/// Registers a united error and its member list.
pub fn register_united_error(ident: &str, members: Vec<String>) {
    if let Ok(mut map) = united_registry().lock() {
        map.insert(ident.to_string(), members);
    }
}

/// Looks up the member list for a united error by path tail segment.
pub fn get_united_members(path: &syn::Path) -> Option<Vec<String>> {
    let last_seg = path.segments.last()?;
    let ident = last_seg.ident.to_string();
    united_registry().lock().ok()?.get(&ident).cloned()
}

/// Checks whether the path refers to a united error.
pub fn is_united_error(path: &syn::Path) -> bool {
    get_united_members(path).is_some()
}

/// Resolves a list of error paths, automatically expanding united errors into
/// their constituent members.
pub fn resolve_error_paths(paths: &[syn::Path]) -> Vec<syn::Path> {
    let mut result = Vec::new();
    for path in paths {
        if let Some(members) = get_united_members(path) {
            for member in members {
                match syn::parse_str(&member) {
                    Ok(p) => result.push(p),
                    Err(_) => result.push(path.clone()),
                }
            }
        } else {
            result.push(path.clone());
        }
    }
    result
}

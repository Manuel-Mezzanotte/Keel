//! Language servers: the programs that give the editor errors, completions,
//! definitions and the rest, through the Language Server Protocol.
//!
//! - [`LanguageServer`] runs one server and carries its messages.
//! - [`text::content_change`] turns an edit into the small change servers want.
//! - [`servers`] knows which server goes with which language, where to find
//!   it and how to install it.

mod client;
#[doc(hidden)]
pub mod fake;
pub mod rpc;
pub mod servers;
pub mod text;

pub use client::{LanguageServer, Response, ResponseError, ServerCommand, ServerEvent, section_of};
pub use lsp_types;

use serde_json::{Map, Value};

/// Turns VS Code's flat settings (`"typescript.preferences.quoteStyle"`)
/// into the nested object servers ask sections of. Keys in brackets, such
/// as `"[typescript]"`, are per-language overrides and are left out.
pub fn nested_settings(flat: &Map<String, Value>) -> Value {
    let mut root = Value::Object(Map::new());
    for (key, value) in flat {
        if key.starts_with('[') {
            continue;
        }
        let mut node = &mut root;
        let mut parts = key.split('.').peekable();
        while let Some(part) = parts.next() {
            if !node.is_object() {
                *node = Value::Object(Map::new());
            }
            let object = node.as_object_mut().expect("an object");
            if parts.peek().is_none() {
                match (object.get_mut(part), value) {
                    // `"editor": {...}` and `"editor.tabSize"` both count.
                    (Some(Value::Object(existing)), Value::Object(new)) => {
                        for (inner_key, inner_value) in new {
                            existing.insert(inner_key.clone(), inner_value.clone());
                        }
                    }
                    _ => {
                        object.insert(part.to_owned(), value.clone());
                    }
                }
                break;
            }
            node = object
                .entry(part.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn flat_settings_become_sections() {
        let flat = json!({
            "typescript.preferences.quoteStyle": "single",
            "typescript.format.enable": true,
            "rust-analyzer": { "check": { "command": "clippy" } },
            "rust-analyzer.cargo.features": "all",
            "[typescript]": { "editor.tabSize": 4 },
            "editor.tabSize": 2,
        });
        let nested = nested_settings(flat.as_object().unwrap());
        assert_eq!(
            section_of(&nested, Some("typescript")),
            json!({ "preferences": { "quoteStyle": "single" }, "format": { "enable": true } })
        );
        assert_eq!(
            section_of(&nested, Some("rust-analyzer")),
            json!({ "check": { "command": "clippy" }, "cargo": { "features": "all" } })
        );
        assert_eq!(section_of(&nested, Some("editor.tabSize")), json!(2));
        assert_eq!(section_of(&nested, Some("eslint")), Value::Null);
        assert!(nested.get("[typescript]").is_none());
    }
}

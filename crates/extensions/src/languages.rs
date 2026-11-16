//! Languages extensions contribute (`contributes.languages`): which files
//! belong to them, and from their `language-configuration.json` how they
//! write comments.

use std::collections::HashMap;

use serde_json::Value;

use crate::InstalledExtension;

/// What Keel uses of a contributed language.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageContribution {
    pub id: String,
    /// Extensions with their dot, lower case: `.prisma`, `.d.ts`.
    pub extensions: Vec<String>,
    /// Whole file names, lower case: `.gitignore`, `dockerfile`.
    pub filenames: Vec<String>,
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

/// The languages of an extension. The same id may come more than once,
/// as VS Code allows; [`LanguageRegistry`] merges them.
pub fn language_contributions(extension: &InstalledExtension) -> Vec<LanguageContribution> {
    let Some(languages) = extension.manifest.raw()["contributes"]["languages"].as_array() else {
        return Vec::new();
    };
    languages
        .iter()
        .filter_map(|language| {
            let id = language["id"].as_str()?.to_owned();
            let configuration = language["configuration"]
                .as_str()
                .and_then(|path| std::fs::read_to_string(extension.root.join(path)).ok())
                .and_then(|text| settings::parse_jsonc(&text).ok())
                .unwrap_or(Value::Null);
            let comments = &configuration["comments"];
            let block = &comments["blockComment"];
            Some(LanguageContribution {
                id,
                extensions: strings(&language["extensions"]),
                filenames: strings(&language["filenames"]),
                line_comment: comments["lineComment"].as_str().map(str::to_owned),
                block_comment: match (block[0].as_str(), block[1].as_str()) {
                    (Some(open), Some(close)) => Some((open.to_owned(), close.to_owned())),
                    _ => None,
                },
            })
        })
        .collect()
}

/// A line comment and a block comment, either of which may be missing.
type Comments = (Option<String>, Option<(String, String)>);

/// [`Comments`] borrowed.
pub type CommentsRef<'a> = (Option<&'a str>, Option<(&'a str, &'a str)>);

/// The contributed languages of the running extensions.
#[derive(Clone, Debug, Default)]
pub struct LanguageRegistry {
    by_name: HashMap<String, String>,
    by_extension: HashMap<String, String>,
    comments: HashMap<String, Comments>,
}

impl LanguageRegistry {
    pub fn new(contributions: impl IntoIterator<Item = LanguageContribution>) -> Self {
        let mut registry = Self::default();
        for language in contributions {
            for name in language.filenames {
                registry.by_name.insert(name, language.id.clone());
            }
            for extension in language.extensions {
                registry.by_extension.insert(extension, language.id.clone());
            }
            let comments = registry.comments.entry(language.id).or_default();
            if language.line_comment.is_some() {
                comments.0 = language.line_comment;
            }
            if language.block_comment.is_some() {
                comments.1 = language.block_comment;
            }
        }
        registry
    }

    /// The language of a file named `name`: by whole name, then by the
    /// longest extension, as VS Code does.
    pub fn language_for(&self, name: &str) -> Option<&str> {
        let name = name.to_lowercase();
        if let Some(id) = self.by_name.get(&name) {
            return Some(id);
        }
        name.char_indices()
            .filter(|(_, c)| *c == '.')
            .find_map(|(at, _)| self.by_extension.get(&name[at..]))
            .map(String::as_str)
    }

    /// The line comment and block comment of `language`, if an extension
    /// gives them.
    pub fn comments(&self, language: &str) -> Option<CommentsRef<'_>> {
        let (line, block) = self.comments.get(language)?;
        Some((
            line.as_deref(),
            block
                .as_ref()
                .map(|(open, close)| (open.as_str(), close.as_str())),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_come_from_the_manifest_and_configuration() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("package.json"),
            r#"{ "name": "lang", "publisher": "someone", "version": "1.0.0",
                 "contributes": { "languages": [
                    { "id": "ignore", "filenames": [".gitignore", ".npmignore"],
                      "configuration": "./ignore.language-configuration.json" },
                    { "id": "prisma", "extensions": [".prisma"],
                      "configuration": "./prisma.json" },
                    { "id": "prisma", "aliases": ["Prisma"] },
                    { "id": "dts", "extensions": [".d.ts"] }
                 ] } }"#,
        )
        .unwrap();
        std::fs::write(
            folder.path().join("ignore.language-configuration.json"),
            r##"{ "comments": { "lineComment": "#" } }"##,
        )
        .unwrap();
        std::fs::write(
            folder.path().join("prisma.json"),
            r#"{ // comments as in VS Code
                "comments": { "lineComment": "//", "blockComment": ["/*", "*/"] },
                "brackets": [["{", "}"]],
            }"#,
        )
        .unwrap();
        let extension = InstalledExtension::load(folder.path()).unwrap();
        let contributions = language_contributions(&extension);
        assert_eq!(contributions.len(), 4);
        assert_eq!(contributions[0].line_comment.as_deref(), Some("#"));

        let registry = LanguageRegistry::new(contributions);
        assert_eq!(registry.language_for(".gitignore"), Some("ignore"));
        assert_eq!(registry.language_for("schema.Prisma"), Some("prisma"));
        assert_eq!(registry.language_for("index.d.ts"), Some("dts"));
        assert_eq!(registry.language_for("index.ts"), None);
        assert_eq!(
            registry.comments("prisma"),
            Some((Some("//"), Some(("/*", "*/"))))
        );
        assert_eq!(registry.comments("ignore"), Some((Some("#"), None)));
        assert_eq!(registry.comments("dts"), Some((None, None)));
        assert_eq!(registry.comments("rust"), None);
    }
}

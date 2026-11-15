//! Languages known to Keel and how files map to them.

use std::path::Path;

use gpui_component::highlighter::LanguageRegistry;

/// A language Keel can highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    TypeScript,
    Tsx,
    PlainText,
}

impl Language {
    /// Picks the language from a file path, by extension.
    pub fn for_path(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("ts" | "mts" | "cts") => Language::TypeScript,
            // The TSX grammar also reads plain JavaScript and JSX well.
            Some("tsx" | "jsx" | "js" | "mjs" | "cjs") => Language::Tsx,
            _ => Language::PlainText,
        }
    }

    /// The language for a VS Code language identifier, such as the one an
    /// extension passes to `setTextDocumentLanguage`.
    pub fn for_vscode_id(id: &str) -> Self {
        match id {
            "typescript" => Language::TypeScript,
            "typescriptreact" | "javascript" | "javascriptreact" => Language::Tsx,
            _ => Language::PlainText,
        }
    }

    /// The name gpui-component's highlighter knows this language by. Names it
    /// does not know, like the one for plain text, turn highlighting off.
    pub fn highlighter_name(self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::PlainText => "text",
        }
    }
}

/// The VS Code language identifier of a file, which extensions see as
/// `document.languageId`.
pub fn vscode_id_for_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("json") => "json",
        Some("md") => "markdown",
        Some("css") => "css",
        Some("html") => "html",
        Some("rs") => "rust",
        _ => "plaintext",
    }
}

/// JSX rules that complete the TypeScript ones for TSX files.
const JSX_HIGHLIGHTS: &str = include_str!("queries/jsx_highlights.scm");

/// Adjusts gpui-component's grammars before any editor is created.
///
/// Out of the box, TSX files only get a small set of type and keyword
/// colors. Keel gives them the full TypeScript rules plus JSX tags and
/// attributes.
pub fn register_grammars() {
    let registry = LanguageRegistry::singleton();
    let (Some(typescript), Some(mut tsx)) =
        (registry.language("typescript"), registry.language("tsx"))
    else {
        return;
    };
    tsx.highlights = format!("{JSX_HIGHLIGHTS}\n{}", typescript.highlights).into();
    tsx.injections = typescript.injections.clone();
    tsx.injection_languages = typescript.injection_languages.clone();
    registry.register("tsx", &tsx);
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use gpui_component::Rope;
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};

    use super::*;

    #[test]
    fn typescript_files() {
        for name in ["index.ts", "module.mts", "config.cts", "UPPER.TS"] {
            assert_eq!(
                Language::for_path(Path::new(name)),
                Language::TypeScript,
                "{name}"
            );
        }
    }

    #[test]
    fn tsx_and_javascript_files() {
        for name in ["app.tsx", "page.jsx", "script.js", "esm.mjs", "common.cjs"] {
            assert_eq!(Language::for_path(Path::new(name)), Language::Tsx, "{name}");
        }
    }

    #[test]
    fn everything_else_is_plain_text() {
        for name in ["README", "notes.md", "data.json", ".env"] {
            assert_eq!(
                Language::for_path(Path::new(name)),
                Language::PlainText,
                "{name}"
            );
        }
    }

    #[test]
    fn vscode_language_identifiers() {
        assert_eq!(vscode_id_for_path(Path::new("card.tsx")), "typescriptreact");
        assert_eq!(vscode_id_for_path(Path::new("index.ts")), "typescript");
        assert_eq!(vscode_id_for_path(Path::new("page.jsx")), "javascriptreact");
        assert_eq!(vscode_id_for_path(Path::new("notes")), "plaintext");
        assert_eq!(Language::for_vscode_id("typescriptreact"), Language::Tsx);
        assert_eq!(Language::for_vscode_id("typescript"), Language::TypeScript);
        assert_eq!(Language::for_vscode_id("markdown"), Language::PlainText);
    }

    #[test]
    fn the_tsx_rules_compile_against_the_tsx_grammar() {
        register_grammars();
        let tsx = LanguageRegistry::singleton().language("tsx").unwrap();
        let grammar = tsx.language.expect("the tsx grammar is compiled in");
        tree_sitter::Query::new(&grammar, &tsx.highlights).expect("valid highlight query");
    }

    /// The highlight color the editor would use for `needle` in `code`.
    fn color_of(language: &str, code: &str, needle: &str) -> Option<gpui::Hsla> {
        let mut highlighter = SyntaxHighlighter::new(language);
        let rope = Rope::from(code);
        highlighter.update(None, &rope, None);
        let theme = HighlightTheme::default_dark();
        let start = code.find(needle).expect("needle is in the code");
        let target: Range<usize> = start..start + needle.len();
        highlighter
            .styles(&(0..code.len()), &*theme)
            .into_iter()
            .find(|(range, _)| range.start <= target.start && target.end <= range.end)
            .and_then(|(_, style)| style.color)
    }

    fn theme_color(name: &str) -> Option<gpui::Hsla> {
        HighlightTheme::default_dark()
            .style(name)
            .and_then(|style| style.color)
    }

    #[test]
    fn tsx_highlights_jsx_tags_attributes_and_typescript() {
        register_grammars();
        let code = "const total: number = 1;\nexport const card = <article className=\"card\">{total}</article>;\n";

        assert_eq!(color_of("tsx", code, "article"), theme_color("tag"), "tag");
        assert_eq!(
            color_of("tsx", code, "className"),
            theme_color("attribute"),
            "attribute"
        );
        assert_eq!(
            color_of("tsx", code, "const"),
            theme_color("keyword"),
            "keyword"
        );
        assert_eq!(color_of("tsx", code, "number"), theme_color("type"), "type");
        assert_eq!(
            color_of("tsx", code, "\"card\""),
            theme_color("string"),
            "string"
        );
    }
}

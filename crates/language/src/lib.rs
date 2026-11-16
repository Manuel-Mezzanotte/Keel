//! Languages known to Keel and how files map to them.

use std::path::Path;

use gpui_component::highlighter::LanguageRegistry;

/// A language Keel can highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    TypeScript,
    Tsx,
    Json,
    Html,
    Css,
    Markdown,
    Yaml,
    Toml,
    Rust,
    Shell,
    PlainText,
}

impl Language {
    /// Picks the language from a file path: its name, then its extension.
    pub fn for_path(path: &Path) -> Self {
        Self::for_vscode_id(vscode_id_for_path(path))
    }

    /// The language for a VS Code language identifier, such as the one an
    /// extension passes to `setTextDocumentLanguage`.
    pub fn for_vscode_id(id: &str) -> Self {
        match id {
            "typescript" => Language::TypeScript,
            // The TSX grammar also reads plain JavaScript and JSX well.
            "typescriptreact" | "javascript" | "javascriptreact" => Language::Tsx,
            "json" | "jsonc" => Language::Json,
            "html" => Language::Html,
            "css" | "scss" | "less" | "tailwindcss" => Language::Css,
            "markdown" => Language::Markdown,
            "yaml" => Language::Yaml,
            "toml" => Language::Toml,
            "rust" => Language::Rust,
            "shellscript" | "dotenv" => Language::Shell,
            _ => Language::PlainText,
        }
    }

    /// The name gpui-component's highlighter knows this language by. Names it
    /// does not know, like the one for plain text, turn highlighting off.
    pub fn highlighter_name(self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::Json => "json",
            Language::Html => "html",
            Language::Css => "css",
            Language::Markdown => "markdown",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Rust => "rust",
            Language::Shell => "bash",
            Language::PlainText => "text",
        }
    }
}

/// The VS Code language identifier of a file, which extensions and language
/// servers see as `languageId`.
pub fn vscode_id_for_path(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    // Names first: these files have no extension, or one that misleads.
    match name.as_str() {
        ".env" => return "dotenv",
        "tsconfig.json" | "jsconfig.json" | ".eslintrc.json" | "devcontainer.json"
        | "settings.json" | "keybindings.json" | "extensions.json" | "launch.json"
        | "tasks.json" | ".babelrc" | ".swcrc" => return "jsonc",
        ".zshrc" | ".bashrc" | ".zprofile" | ".bash_profile" | ".profile" => {
            return "shellscript";
        }
        "cargo.lock" | ".prettierrc.toml" => return "toml",
        ".prettierrc" | ".eslintrc" => return "json",
        _ => {}
    }
    // `.env.local`, `.env.production`...
    if name.starts_with(".env.") {
        return "dotenv";
    }
    if name.starts_with("tsconfig.") && name.ends_with(".json") {
        return "jsonc";
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("json" | "webmanifest") => "json",
        Some("jsonc" | "json5") => "jsonc",
        Some("md" | "markdown" | "mdx") => "markdown",
        Some("css") => "css",
        Some("scss") => "scss",
        Some("less") => "less",
        Some("html" | "htm") => "html",
        Some("rs") => "rust",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("prisma") => "prisma",
        Some("sh" | "bash" | "zsh") => "shellscript",
        Some("env") => "dotenv",
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
    fn web_and_rust_files() {
        let cases = [
            ("notes.md", Language::Markdown),
            ("data.json", Language::Json),
            ("tsconfig.json", Language::Json),
            ("index.html", Language::Html),
            ("globals.css", Language::Css),
            ("theme.scss", Language::Css),
            ("docker-compose.yml", Language::Yaml),
            ("Cargo.toml", Language::Toml),
            ("Cargo.lock", Language::Toml),
            ("main.rs", Language::Rust),
            (".env", Language::Shell),
            (".env.local", Language::Shell),
            ("deploy.sh", Language::Shell),
        ];
        for (name, language) in cases {
            assert_eq!(Language::for_path(Path::new(name)), language, "{name}");
        }
    }

    #[test]
    fn everything_else_is_plain_text() {
        for name in ["README", "schema.prisma", "notes.txt", "Makefile"] {
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
        assert_eq!(
            vscode_id_for_path(Path::new("tsconfig.build.json")),
            "jsonc"
        );
        assert_eq!(vscode_id_for_path(Path::new(".env.production")), "dotenv");
        assert_eq!(vscode_id_for_path(Path::new("schema.prisma")), "prisma");
        assert_eq!(Language::for_vscode_id("markdown"), Language::Markdown);
        assert_eq!(Language::for_vscode_id("prisma"), Language::PlainText);
    }

    #[test]
    fn the_tsx_rules_compile_against_the_tsx_grammar() {
        register_grammars();
        let tsx = LanguageRegistry::singleton().language("tsx").unwrap();
        let grammar = tsx.language.expect("the tsx grammar is compiled in");
        tree_sitter::Query::new(&grammar, &tsx.highlights).expect("valid highlight query");
    }

    #[test]
    fn every_language_has_a_grammar() {
        register_grammars();
        let registry = LanguageRegistry::singleton();
        for language in [
            Language::TypeScript,
            Language::Tsx,
            Language::Json,
            Language::Html,
            Language::Css,
            Language::Markdown,
            Language::Yaml,
            Language::Toml,
            Language::Rust,
            Language::Shell,
        ] {
            let config = registry
                .language(language.highlighter_name())
                .unwrap_or_else(|| panic!("{language:?} is registered"));
            let grammar = config.language.expect("the grammar is compiled in");
            tree_sitter::Query::new(&grammar, &config.highlights)
                .unwrap_or_else(|error| panic!("{language:?}: {error}"));
        }
    }

    #[test]
    fn web_and_rust_code_gets_colors() {
        register_grammars();
        let cases = [
            ("rust", "fn main() { let total: u32 = 1; }", "fn", "keyword"),
            (
                "rust",
                "fn main() { let s = \"ciao\"; }",
                "\"ciao\"",
                "string",
            ),
            ("css", ".card { color: red; }", "color", "property"),
            ("html", "<div class=\"a\"></div>", "div", "tag"),
            ("json", "{ \"name\": \"keel\" }", "\"keel\"", "string"),
            ("toml", "[package]\nname = \"keel\"\n", "\"keel\"", "string"),
            ("yaml", "name: keel\n", "name", "property"),
            (
                "bash",
                "DATABASE_URL=\"file:./dev.db\"\n",
                "\"file:./dev.db\"",
                "string",
            ),
        ];
        for (language, code, needle, style) in cases {
            assert_eq!(
                color_of(language, code, needle),
                theme_color(style),
                "{language}: {needle} should be a {style}"
            );
        }
    }

    #[test]
    fn markdown_titles_code_and_links_get_colors() {
        register_grammars();
        let code = "# Keel\n\nUn IDE in `Rust`, vedi [il piano](docs/PIANO.md).\n";
        for needle in ["Keel", "`Rust`", "il piano"] {
            assert!(color_of("markdown", code, needle).is_some(), "{needle}");
        }
    }

    /// Times the first highlighting of Keel's largest Rust file, repeated
    /// eight times (about 24,000 lines).
    /// Run with `cargo test -p language --release -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_a_large_rust_file() {
        register_grammars();
        let file = include_str!("../../keel/src/workbench_window.rs").repeat(8);
        let rope = Rope::from(file.as_str());
        let start = std::time::Instant::now();
        let mut highlighter = SyntaxHighlighter::new("rust");
        highlighter.update(None, &rope, None);
        let parsed = start.elapsed();
        let theme = HighlightTheme::default_dark();
        let start = std::time::Instant::now();
        let styles = highlighter.styles(&(0..4000), &*theme);
        println!(
            "{} lines: parsed in {parsed:?}, first screen styled in {:?} ({} spans)",
            file.lines().count(),
            start.elapsed(),
            styles.len()
        );
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

//! Which server goes with which language, where to find it, and how to
//! install it when it is missing.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use crate::ServerCommand;
use crate::prerequisites::Prerequisite;

/// How a missing server is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    /// An npm package, in its own folder under Keel's `servers` folder.
    Npm {
        package: &'static str,
        version: &'static str,
    },
    /// A rustup component of the project's toolchain.
    Rustup { component: &'static str },
    /// A download from a release page. In the address `{arch}` becomes
    /// `aarch64` or `x86_64`, `{aarch_64}` `aarch_64` or `x86_64`, and
    /// `{arm64}` `arm64` or `amd64`, as projects name them.
    Download {
        url: &'static str,
        version: &'static str,
        unpack: Unpack,
        /// The SHA-256 of the Apple silicon download. A different one stops
        /// the install; on Intel Macs there is nothing to compare with.
        sha256: Option<&'static str>,
    },
    /// Part of something the user installs, such as the Xcode tools: Keel
    /// finds it but does not install it.
    System,
}

/// What a download is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unpack {
    /// The program itself.
    File,
    /// The program, compressed with gzip.
    Gzip,
    /// A `.tar.gz` or `.zip` archive, unpacked into a folder of its
    /// version; `program` is where the program is inside it.
    Archive { program: &'static str },
}

/// When a server is worth starting for a project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Whenever a file of its languages is open.
    Always,
    /// Only if the project has one of these files at its top, or one of
    /// these packages in its `package.json`.
    ProjectUses {
        files: &'static [&'static str],
        packages: &'static [&'static str],
    },
    /// Only if the project's Maven or Gradle build names this text, such
    /// as `spring-boot`.
    BuildMentions(&'static str),
}

/// A language server Keel knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerSpec {
    /// A short, stable name, used for folders and settings.
    pub id: &'static str,
    /// The name shown in the Output view and the status bar.
    pub name: &'static str,
    /// VS Code language identifiers it serves.
    pub languages: &'static [&'static str],
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub install: Install,
    pub activation: Activation,
    /// What must be on the machine first, such as Java.
    pub needs: &'static [Prerequisite],
}

impl ServerSpec {
    pub fn serves(&self, language_id: &str) -> bool {
        self.languages.contains(&language_id)
    }

    /// Whether the project at `root` wants this server.
    pub fn wanted_in(&self, root: &Path) -> bool {
        match self.activation {
            Activation::Always => true,
            Activation::BuildMentions(text) => build_files_mention(root, text),
            Activation::ProjectUses { files, packages } => {
                let has_file = files.iter().any(|name| match name.strip_suffix('*') {
                    Some(prefix) => std::fs::read_dir(root).is_ok_and(|entries| {
                        entries
                            .flatten()
                            .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                    }),
                    None => root.join(name).exists(),
                });
                has_file
                    || package_dependencies(root)
                        .iter()
                        .any(|name| packages.contains(&name.as_str()))
            }
        }
    }
}

/// The packages a project's `package.json` depends on, of every kind.
fn package_dependencies(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    ["dependencies", "devDependencies", "peerDependencies"]
        .iter()
        .filter_map(|kind| manifest[kind].as_object())
        .flat_map(|dependencies| dependencies.keys().cloned())
        .collect()
}

const WEB_LANGUAGES: &[&str] = &[
    "typescript",
    "typescriptreact",
    "javascript",
    "javascriptreact",
];

/// The languages of Spring Boot's server: its configuration files, which
/// Keel recognizes in Spring Boot projects, and Java.
pub const SPRING_BOOT_LANGUAGES: &[&str] = &[
    "spring-boot-properties",
    "spring-boot-properties-yaml",
    "java",
];

const SPRING_BOOT_SERVER: &str =
    "extension/language-server/spring-boot-language-server-2.4.1-SNAPSHOT-exec.jar";

/// The parts of Spring Boot's extension that jdtls loads
/// (`contributes.javaExtensions` in its `package.json`).
const SPRING_BOOT_JDTLS_BUNDLES: &[&str] = &[
    "extension/jars/io.projectreactor.reactor-core.jar",
    "extension/jars/org.reactivestreams.reactive-streams.jar",
    "extension/jars/jdt-ls-commons.jar",
    "extension/jars/jdt-ls-extension.jar",
    "extension/jars/sts-gradle-tooling.jar",
];

/// The jar that starts jdtls, inside its download.
const JDTLS_LAUNCHER: &str =
    "extension/server/plugins/org.eclipse.equinox.launcher_1.8.0.v20260804-1928.jar";

/// The Java options of VS Code's Java extension (`java.jdt.ls.vmargs`):
/// a small heap to start with, and a garbage collector that gives memory
/// back.
const JDTLS_VM_ARGS: &[&str] = &[
    "-XX:+UseParallelGC",
    "-XX:GCTimeRatio=4",
    "-XX:AdaptiveSizePolicyWeight=90",
    "-Dsun.zip.disableMemoryMapping=true",
    "-Xmx2G",
    "-Xms100m",
    "-Xlog:disable",
];

/// Every server Keel can run, in the order they start for a file.
pub const SERVERS: &[ServerSpec] = &[
    ServerSpec {
        id: "vtsls",
        name: "TypeScript",
        languages: WEB_LANGUAGES,
        program: "vtsls",
        args: &["--stdio"],
        install: Install::Npm {
            package: "@vtsls/language-server",
            version: "0.3.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "eslint",
        name: "ESLint",
        languages: WEB_LANGUAGES,
        program: "vscode-eslint-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            version: "4.10.0",
        },
        activation: Activation::ProjectUses {
            files: &[".eslintrc*", "eslint.config.*"],
            packages: &["eslint"],
        },
        needs: &[],
    },
    ServerSpec {
        id: "html",
        name: "HTML",
        languages: &["html"],
        program: "vscode-html-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            version: "4.10.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "css",
        name: "CSS",
        languages: &["css", "scss", "less"],
        program: "vscode-css-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            version: "4.10.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "json",
        name: "JSON",
        languages: &["json", "jsonc"],
        program: "vscode-json-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "vscode-langservers-extracted",
            version: "4.10.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "tailwindcss",
        name: "Tailwind CSS",
        languages: &[
            "typescriptreact",
            "javascriptreact",
            "html",
            "css",
            "scss",
            "less",
        ],
        program: "tailwindcss-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "@tailwindcss/language-server",
            version: "0.16.0",
        },
        activation: Activation::ProjectUses {
            files: &["tailwind.config.*"],
            packages: &["tailwindcss", "@tailwindcss/postcss", "@tailwindcss/vite"],
        },
        needs: &[],
    },
    ServerSpec {
        id: "emmet",
        name: "Emmet",
        languages: &[
            "html",
            "css",
            "scss",
            "less",
            "typescriptreact",
            "javascriptreact",
        ],
        program: "emmet-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "@olrtg/emmet-language-server",
            version: "2.8.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "prisma",
        name: "Prisma",
        languages: &["prisma"],
        program: "prisma-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "@prisma/language-server",
            version: "31.13.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "yaml",
        name: "YAML",
        languages: &["yaml"],
        program: "yaml-language-server",
        args: &["--stdio"],
        install: Install::Npm {
            package: "yaml-language-server",
            version: "2.0.0",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "taplo",
        name: "TOML",
        languages: &["toml"],
        program: "taplo",
        args: &["lsp", "stdio"],
        // The npm package is built without the language server.
        install: Install::Download {
            url: "https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-darwin-{arch}.gz",
            version: "0.10.0",
            unpack: Unpack::Gzip,
            sha256: Some("713734314c3e71894b9e77513c5349835eefbd52908445a0d73b0c7dc469347d"),
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "marksman",
        name: "Markdown",
        languages: &["markdown"],
        program: "marksman",
        args: &["server"],
        install: Install::Download {
            url: "https://github.com/artempyanykh/marksman/releases/download/2026-02-08/marksman-macos",
            version: "2026-02-08",
            unpack: Unpack::File,
            sha256: Some("6a801c17b5ac0dba69787c5282b3b3bd416e66c96253fae098d311c6bbd1833b"),
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "basedpyright",
        name: "Python",
        languages: &["python"],
        program: "basedpyright-langserver",
        args: &["--stdio"],
        // Pyright, as in Pylance, in its free build with the parts Pylance
        // keeps for itself (semantic colors, inlay hints...).
        install: Install::Npm {
            package: "basedpyright",
            version: "1.40.2",
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "ruff",
        name: "Ruff",
        languages: &["python"],
        program: "ruff",
        args: &["server"],
        install: Install::Download {
            url: "https://github.com/astral-sh/ruff/releases/download/0.17.5/ruff-{arch}-apple-darwin.tar.gz",
            version: "0.17.5",
            unpack: Unpack::Archive {
                program: "ruff-{arch}-apple-darwin/ruff",
            },
            sha256: Some("4ffadabec5c5fee1ddb2eecac0787ffe6fa18c665b73da9258b84211f40c5829"),
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "jdtls",
        name: "Java",
        languages: &["java"],
        // The launcher inside the download; Keel starts it with Java.
        program: JDTLS_LAUNCHER,
        args: &[],
        // VS Code's Java extension for this Mac: jdtls with the Java it
        // runs on and Lombok, so nothing else needs installing.
        install: Install::Download {
            url: "https://open-vsx.org/api/redhat/java/darwin-{x64}/1.58.0/file/redhat.java-1.58.0@darwin-{x64}.vsix",
            version: "1.58.0",
            unpack: Unpack::Archive {
                program: JDTLS_LAUNCHER,
            },
            sha256: Some("8783b3331d6b35e8ae80833e76974edbca2fa5f3a9d9943b598680087d2b24e8"),
        },
        activation: Activation::Always,
        needs: &[],
    },
    ServerSpec {
        id: "spring-boot",
        name: "Spring Boot",
        languages: SPRING_BOOT_LANGUAGES,
        program: SPRING_BOOT_SERVER,
        args: &[],
        // VS Code's Spring Boot extension: its server, and the parts jdtls
        // loads to answer it about the project's classes.
        install: Install::Download {
            url: "https://open-vsx.org/api/VMware/vscode-spring-boot/2.4.1/file/VMware.vscode-spring-boot-2.4.1.vsix",
            version: "2.4.1",
            unpack: Unpack::Archive {
                program: SPRING_BOOT_SERVER,
            },
            sha256: Some("6971f2bbc513a944e7f907e48ec68f64848d689630d5616bf54ef0803349f6ed"),
        },
        activation: Activation::BuildMentions("spring-boot"),
        needs: &[],
    },
    ServerSpec {
        id: "rust-analyzer",
        name: "rust-analyzer",
        languages: &["rust"],
        program: "rust-analyzer",
        args: &[],
        install: Install::Rustup {
            component: "rust-analyzer",
        },
        activation: Activation::Always,
        needs: &[],
    },
];

/// The settings VS Code's own extensions give a server unless the user
/// says otherwise. Without them some servers stay quiet: the CSS server,
/// for one, checks nothing unless `css.validate` is on.
pub fn default_settings(spec: &ServerSpec) -> Value {
    let css = json!({ "validate": true, "completion": { "triggerPropertyValueCompletion": true, "completePropertyWithSemicolon": true }, "hover": { "documentation": true, "references": true } });
    match spec.id {
        "jdtls" => json!({
            "java": {
                // VS Code asks before updating the project after a change
                // to `pom.xml` or `build.gradle`; Keel has nowhere to ask
                // and does it, as most people answer.
                "configuration": { "updateBuildConfiguration": "automatic" },
                "import": {
                    "maven": { "enabled": true },
                    "gradle": { "enabled": true, "wrapper": { "enabled": true } },
                    "exclusions": ["**/node_modules/**", "**/.metadata/**", "**/archetype-resources/**", "**/META-INF/maven/**"],
                },
                "maven": { "downloadSources": true },
                "autobuild": { "enabled": true },
                "completion": {
                    "enabled": true,
                    "importOrder": ["#", "java", "javax", "org", "com", ""],
                    "favoriteStaticMembers": ["org.junit.jupiter.api.Assertions.*", "org.mockito.Mockito.*", "org.mockito.ArgumentMatchers.*"],
                },
                "format": { "enabled": true },
                "signatureHelp": { "enabled": true },
                "errors": { "incompleteClasspath": { "severity": "warning" } },
                "server": { "launchMode": "Standard" },
            },
        }),
        "basedpyright" => json!({
            "python": { "analysis": { "autoImportCompletions": true } },
            "basedpyright": { "analysis": {
                // Pyright's own default, the one the `pyright` command
                // uses. basedpyright's "recommended" flags much more than
                // VS Code and Cursor do; projects choose in their
                // `pyproject.toml` or `pyrightconfig.json`.
                "typeCheckingMode": "standard",
                "autoImportCompletions": true,
                "diagnosticMode": "openFilesOnly",
                "useLibraryCodeForTypes": true,
                "autoSearchPaths": true,
            } },
        }),
        "vtsls" => json!({
            "vtsls": {
                // The project's own TypeScript, as Cursor offers.
                "autoUseWorkspaceTsdk": true,
                // The server filters what was typed and sends at most this
                // many items: without a limit an empty word brings
                // 15,000 of them, a second to send.
                "experimental": { "completion": { "enableServerSideFuzzyMatch": true, "entriesLimit": 1000 } },
            },
            "typescript": { "updateImportsOnFileMove": { "enabled": "always" } },
            "javascript": { "updateImportsOnFileMove": { "enabled": "always" } },
        }),
        "css" => json!({ "css": css, "scss": css, "less": css }),
        "html" => json!({
            "html": {
                "validate": { "scripts": true, "styles": true },
                "format": { "enable": true },
                "suggest": { "html5": true },
                "hover": { "documentation": true, "references": true },
            },
            "css": css,
            "javascript": { "validate": { "enable": true } },
        }),
        "json" => json!({
            "json": {
                "validate": { "enable": true },
                "format": { "enable": true },
                "schemaDownload": { "enable": true },
                "schemas": json_schemas(),
            },
        }),
        // ESLint asks for its whole configuration with an empty section,
        // so its settings are the root, shaped as the VS Code extension's.
        "eslint" => json!({
            "validate": "on",
            "packageManager": "npm",
            "useESLintClass": false,
            "experimental": { "useFlatConfig": false },
            "useFlatConfig": true,
            "codeAction": {
                "disableRuleComment": { "enable": true, "location": "separateLine", "commentStyle": "line" },
                "showDocumentation": { "enable": true },
            },
            "codeActionOnSave": { "enable": false, "mode": "all" },
            "format": false,
            "quiet": false,
            "onIgnoredFiles": "off",
            "options": {},
            "rulesCustomizations": [],
            "run": "onType",
            "problems": { "shortenToSingleLine": false },
            "nodePath": null,
            "workingDirectory": { "mode": "auto" },
        }),
        "tailwindcss" => json!({
            "editor": { "tabSize": 2 },
            "tailwindCSS": {
                "emmetCompletions": false,
                "classAttributes": ["class", "className", "ngClass", "class:list"],
                "classFunctions": [],
                "includeLanguages": {},
                "files": { "exclude": ["**/.git/**", "**/node_modules/**", "**/.hg/**", "**/.svn/**"] },
                "validate": true,
                "showPixelEquivalents": true,
                "rootFontSize": 16,
                "colorDecorators": true,
                "suggestions": true,
                "hovers": true,
                "codeActions": true,
                "lint": {
                    "cssConflict": "warning",
                    "invalidApply": "error",
                    "invalidScreen": "error",
                    "invalidVariant": "error",
                    "invalidConfigPath": "error",
                    "invalidTailwindDirective": "error",
                    "invalidSourceDirective": "error",
                    "recommendedVariantOrder": "warning",
                    "usedBlocklistedClass": "warning",
                },
                "experimental": { "classRegex": [] },
            },
        }),
        "prisma" => json!({
            "prisma": { "enableDiagnostics": true, "fileWatcher": false, "trace": { "server": "off" } },
        }),
        "taplo" => json!({
            "evenBetterToml": {
                "schema": {
                    "enabled": true,
                    "catalogs": ["https://www.schemastore.org/api/json/catalog.json"],
                },
                "formatter": {},
            },
        }),
        "yaml" => json!({
            "yaml": {
                "validate": true,
                "hover": true,
                "completion": true,
                "format": { "enable": true },
                "schemaStore": { "enable": true, "url": "https://www.schemastore.org/api/json/catalog.json" },
            },
            "redhat": { "telemetry": { "enabled": false } },
        }),
        _ => json!({}),
    }
}

/// The schemas VS Code's extensions tie to well-known files.
fn json_schemas() -> Value {
    let schema = |files: &[&str], url: &str| json!({ "fileMatch": files, "url": url });
    Value::Array(vec![
        schema(
            &["package.json"],
            "https://www.schemastore.org/package.json",
        ),
        schema(
            &["tsconfig.json", "tsconfig.*.json"],
            "https://www.schemastore.org/tsconfig.json",
        ),
        schema(
            &["jsconfig.json", "jsconfig.*.json"],
            "https://www.schemastore.org/jsconfig.json",
        ),
        schema(
            &[".eslintrc", ".eslintrc.json"],
            "https://www.schemastore.org/eslintrc.json",
        ),
        schema(
            &[".prettierrc", ".prettierrc.json"],
            "https://www.schemastore.org/prettierrc.json",
        ),
        schema(&["vercel.json"], "https://openapi.vercel.sh/vercel.json"),
        schema(&["components.json"], "https://ui.shadcn.com/schema.json"),
        schema(
            &[".vscode/settings.json", ".keel/settings.json"],
            "https://www.schemastore.org/vscode-settings.json",
        ),
    ])
}

/// Deep-merges `overlay` into `base`: objects key by key, anything else
/// replaced.
pub fn merge_settings(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        // Nothing to add: a missing section must not erase the defaults.
        (_, Value::Null) => {}
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(key) {
                    Some(existing) => merge_settings(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// What `workspace/configuration` answers a server: its defaults with the
/// user's settings (nested, see [`crate::nested_settings`]) on top. For
/// ESLint only its own section counts, at the root, plus the project folder
/// it wants to know.
pub fn settings_for(spec: &ServerSpec, user: &Value, root: &Path) -> Value {
    let mut settings = default_settings(spec);
    if spec.id == "eslint" {
        merge_settings(&mut settings, &user["eslint"]);
        settings["workspaceFolder"] = json!({
            "uri": workbench::uri::Uri::file(root).to_string(),
            "name": root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        });
        return settings;
    }
    // The project's own Python, as the Python extension picks it.
    if spec.id == "basedpyright"
        && user["python"]["pythonPath"].is_null()
        && let Some(python) = python_interpreter(root, user)
    {
        settings["python"]["pythonPath"] = json!(python.to_string_lossy());
    }
    // Tailwind's at-rules (`@theme`, `@apply`...) are not CSS: in its
    // projects the CSS server leaves them alone, as most set up VS Code.
    if matches!(spec.id, "css" | "html")
        && self::spec("tailwindcss").is_some_and(|tailwind| tailwind.wanted_in(root))
    {
        for language in ["css", "scss", "less"] {
            settings[language]["lint"]["unknownAtRules"] = json!("ignore");
        }
    }
    merge_settings(&mut settings, user);
    settings
}

/// The Python of a project, as VS Code's Python extension finds it:
/// `python.defaultInterpreterPath` if set, then a virtual environment in
/// the project (`.venv`, which uv and most tools make, `venv` or `env`),
/// then Poetry's own. `None` leaves the server to the `python3` on the
/// `PATH`.
pub fn python_interpreter(root: &Path, user: &Value) -> Option<PathBuf> {
    if let Some(path) = user["python"]["defaultInterpreterPath"].as_str() {
        let path = PathBuf::from(path);
        let path = if path.is_relative() {
            root.join(path)
        } else {
            path
        };
        if path.exists() {
            return Some(path);
        }
    }
    let in_project = [".venv", "venv", "env"]
        .iter()
        .map(|name| root.join(name))
        .find(|venv| venv.join("pyvenv.cfg").exists() && venv.join("bin/python").exists());
    if let Some(venv) = in_project {
        return Some(venv.join("bin/python"));
    }
    poetry_environment(root).map(|venv| venv.join("bin/python"))
}

/// Poetry keeps environments in its cache, named after the project:
/// `keel-AbCdEf12-py3.12`.
fn poetry_environment(root: &Path) -> Option<PathBuf> {
    if !root.join("poetry.lock").exists() {
        return None;
    }
    let manifest = std::fs::read_to_string(root.join("pyproject.toml")).ok()?;
    let name = manifest
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let value = line.strip_prefix("name")?.trim_start().strip_prefix('=')?;
            Some(value.trim().trim_matches(['"', '\'']).to_owned())
        })?
        .to_lowercase()
        .replace(['_', '.'], "-");
    let home = std::env::var("HOME").ok()?;
    let cache = Path::new(&home).join("Library/Caches/pypoetry/virtualenvs");
    let mut found: Vec<PathBuf> = std::fs::read_dir(cache)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|file| file.to_string_lossy().starts_with(&format!("{name}-")))
                && path.join("bin/python").exists()
        })
        .collect();
    found.sort();
    found.pop()
}

/// What a server gets in `initializationOptions`, as VS Code's extensions
/// send them.
pub fn initialization_options(
    spec: &ServerSpec,
    root: &Path,
    user: &Value,
    servers_dir: &Path,
) -> Option<Value> {
    match spec.id {
        "spring-boot" => Some(json!({
            "workspaceFolders": [workbench::uri::Uri::file(root).to_string()],
            "enableJdtClasspath": false,
        })),
        "jdtls" => Some(json!({
            "workspaceFolders": [workbench::uri::Uri::file(root).to_string()],
            "settings": settings_for(spec, user, root),
            "bundles": spring_boot_bundles(servers_dir, root),
            // What VS Code's Java extension can do beyond the protocol,
            // as far as Keel does it too.
            "extendedClientCapabilities": {
                "classFileContentsSupport": true,
                "executeClientCommandSupport": true,
                "resolveAdditionalTextEditsSupport": true,
                "progressReportProvider": false,
                "clientHoverProvider": false,
                "clientDocumentSymbolProvider": false,
                "shouldLanguageServerExitOnShutdown": true,
                "snippetEditSupport": false,
            },
        })),
        // Without this the HTML, CSS and JSON servers do not format.
        "html" | "css" => Some(json!({ "provideFormatter": true })),
        // The JSON server downloads schemas itself instead of asking.
        "json" => Some(json!({
            "provideFormatter": true,
            "handledSchemaProtocols": ["file", "http", "https"],
        })),
        "emmet" => Some(json!({ "showSuggestionsAsSnippets": true })),
        "taplo" => Some(json!({ "configurationSection": "evenBetterToml" })),
        _ => None,
    }
}

/// The servers for a file of `language_id` in the project at `root`.
pub fn servers_for(language_id: &str, root: &Path) -> Vec<&'static ServerSpec> {
    SERVERS
        .iter()
        .filter(|spec| spec.serves(language_id) && spec.wanted_in(root))
        .collect()
}

pub fn spec(id: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|spec| spec.id == id)
}

/// Where Keel keeps one server it installed.
pub fn install_dir(servers_dir: &Path, spec: &ServerSpec) -> PathBuf {
    match spec.install {
        // Servers from the same package share one folder.
        Install::Npm { package, .. } => {
            servers_dir.join(package.trim_start_matches('@').replace('/', "-"))
        }
        Install::Download { .. } | Install::Rustup { .. } | Install::System => {
            servers_dir.join(spec.id)
        }
    }
}

/// Finds an installed server: first the copy Keel installed, at the right
/// version, then the toolchain's (for rustup components), then `PATH`.
pub fn find(
    spec: &ServerSpec,
    servers_dir: &Path,
    root: &Path,
    env: &HashMap<String, String>,
) -> Option<PathBuf> {
    let dir = install_dir(servers_dir, spec);
    match spec.install {
        Install::Npm { package, version } => {
            let manifest = dir.join("node_modules").join(package).join("package.json");
            let installed = std::fs::read_to_string(manifest)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|manifest| manifest["version"].as_str().map(str::to_owned));
            let program = dir.join("node_modules/.bin").join(spec.program);
            if installed.as_deref() == Some(version) && program.exists() {
                return Some(program);
            }
        }
        Install::Download {
            version, unpack, ..
        } => {
            let program = downloaded_program(&dir, spec.program, version, unpack);
            let installed = std::fs::read_to_string(dir.join("version")).ok();
            if installed.as_deref().map(str::trim) == Some(version) && program.exists() {
                return Some(program);
            }
        }
        Install::System => {
            if let Some(program) = which_in(spec.program, env) {
                return Some(program);
            }
            // Programs of the Xcode tools that are not on the `PATH`.
            let output = Command::new("/usr/bin/xcrun")
                .args(["--find", spec.program])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            return (output.status.success() && path.exists()).then_some(path);
        }
        Install::Rustup { .. } => {
            let output = Command::new(which_in("rustup", env)?)
                .args(["which", spec.program])
                .current_dir(root)
                .envs(env)
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if output.status.success() {
                let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
                if path.exists() {
                    return Some(path);
                }
            }
            return None;
        }
    }
    which_in(spec.program, env)
}

/// Where a downloaded program is once unpacked.
fn downloaded_program(dir: &Path, program: &str, version: &str, unpack: Unpack) -> PathBuf {
    match unpack {
        Unpack::File | Unpack::Gzip => dir.join(program),
        Unpack::Archive { program } => dir.join(version).join(expand_arch(program)),
    }
}

/// Fills in the machine's architecture, in each project's spelling:
/// `{arch}`, `{aarch_64}`, `{arm64}` (Go) and `{x64}` (VS Code).
pub fn expand_arch(text: &str) -> String {
    let apple_silicon = std::env::consts::ARCH == "aarch64";
    text.replace("{arch}", if apple_silicon { "aarch64" } else { "x86_64" })
        .replace(
            "{aarch_64}",
            if apple_silicon { "aarch_64" } else { "x86_64" },
        )
        .replace("{arm64}", if apple_silicon { "arm64" } else { "amd64" })
        .replace("{x64}", if apple_silicon { "arm64" } else { "x64" })
}

/// A program on the `PATH` of `env`.
pub fn which_in(program: &str, env: &HashMap<String, String>) -> Option<PathBuf> {
    let path = env.get("PATH")?;
    std::env::split_paths(OsStr::new(path))
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// What installing `spec` runs, for the Output view and the tests:
/// the program and its arguments.
pub fn install_command(
    spec: &ServerSpec,
    servers_dir: &Path,
    env: &HashMap<String, String>,
) -> Result<(PathBuf, Vec<String>)> {
    let dir = install_dir(servers_dir, spec);
    match spec.install {
        Install::Npm { package, version } => {
            let npm =
                which_in("npm", env).context("npm is not installed: install Node.js first")?;
            Ok((
                npm,
                vec![
                    "install".to_owned(),
                    "--prefix".to_owned(),
                    dir.to_string_lossy().into_owned(),
                    "--no-audit".to_owned(),
                    "--no-fund".to_owned(),
                    "--loglevel=error".to_owned(),
                    format!("{package}@{version}"),
                ],
            ))
        }
        Install::Rustup { component } => {
            let rustup = which_in("rustup", env).context("rustup is not installed")?;
            Ok((
                rustup,
                vec![
                    "component".to_owned(),
                    "add".to_owned(),
                    component.to_owned(),
                ],
            ))
        }
        Install::Download { url, .. } => Ok((
            PathBuf::from("/usr/bin/curl"),
            vec![
                "--fail".to_owned(),
                "--silent".to_owned(),
                "--show-error".to_owned(),
                "--location".to_owned(),
                "--output".to_owned(),
                dir.join(DOWNLOAD).to_string_lossy().into_owned(),
                expand_arch(url),
            ],
        )),
        Install::System => bail!(
            "{} comes with the system: Keel does not install it",
            spec.program
        ),
    }
}

/// The name of a download before it is checked and unpacked.
const DOWNLOAD: &str = "download.part";

/// Installs `spec`. Slow: run it away from the interface. Returns what the
/// installer printed, for the Output view.
pub fn install(
    spec: &ServerSpec,
    servers_dir: &Path,
    root: &Path,
    env: &HashMap<String, String>,
) -> Result<String> {
    // One install at a time per folder: two windows, or jdtls waiting for
    // Spring Boot's parts, must not write the same download.
    static INSTALLING: OnceLock<
        std::sync::Mutex<HashMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>>,
    > = OnceLock::new();
    let dir = install_dir(servers_dir, spec);
    let lock = INSTALLING
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(dir.clone())
        .or_default()
        .clone();
    let _installing = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if find(spec, servers_dir, root, env).is_some_and(|program| program.starts_with(&dir)) {
        return Ok(String::new());
    }
    std::fs::create_dir_all(&dir)?;
    let (program, args) = install_command(spec, servers_dir, env)?;
    let output = Command::new(&program)
        .args(&args)
        .current_dir(if matches!(spec.install, Install::Rustup { .. }) {
            root
        } else {
            &dir
        })
        .envs(env)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", program.display()))?;
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!("{} failed: {}", program.display(), printed.trim());
    }
    if let Install::Download {
        version,
        unpack,
        sha256,
        ..
    } = spec.install
    {
        finish_download(&dir, spec.program, version, unpack, sha256)?;
    }
    Ok(printed)
}

/// Checks a download and puts the program in place.
fn finish_download(
    dir: &Path,
    program: &str,
    version: &str,
    unpack: Unpack,
    sha256: Option<&str>,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let download = dir.join(DOWNLOAD);
    if let Some(expected) = sha256.filter(|_| std::env::consts::ARCH == "aarch64") {
        let actual = sha256_of(&download)?;
        if actual != expected {
            let _ = std::fs::remove_file(&download);
            bail!("the download is not the expected one (SHA-256 {actual}, expected {expected})");
        }
    }
    let target = downloaded_program(dir, program, version, unpack);
    match unpack {
        Unpack::File => std::fs::rename(&download, &target)?,
        Unpack::Gzip => {
            let output = Command::new("/usr/bin/gunzip")
                .arg("--stdout")
                .arg(&download)
                .stdin(Stdio::null())
                .output()?;
            if !output.status.success() {
                bail!(
                    "gunzip failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            std::fs::write(&target, output.stdout)?;
            std::fs::remove_file(&download)?;
        }
        Unpack::Archive { .. } => {
            // A folder per version: a new one never mixes with the old.
            let folder = dir.join(version);
            std::fs::create_dir_all(&folder)?;
            // macOS's `tar` reads `.zip` files too.
            let output = Command::new("/usr/bin/tar")
                .arg("-xf")
                .arg(&download)
                .arg("-C")
                .arg(&folder)
                .stdin(Stdio::null())
                .output()?;
            if !output.status.success() {
                bail!(
                    "unpacking failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            std::fs::remove_file(&download)?;
        }
    }
    if !target.exists() {
        bail!("{} is not in the download", target.display());
    }
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(dir.join("version"), version)?;
    Ok(())
}

/// The SHA-256 of a file, in hexadecimal.
fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest as _, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The command that starts `spec` from `program`.
pub fn command(
    spec: &ServerSpec,
    program: PathBuf,
    root: &Path,
    env: &HashMap<String, String>,
    extra_env: &[(String, String)],
) -> ServerCommand {
    let mut variables: Vec<(String, String)> = env
        .iter()
        .filter(|(key, _)| !extra_env.iter().any(|(extra, _)| extra == *key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    variables.extend(extra_env.iter().cloned());
    let (program, args) = match spec.id {
        "jdtls" => jdtls_command(&program, root, extra_env),
        "spring-boot" => spring_boot_command(&program, extra_env),
        _ => (
            program,
            spec.args.iter().map(|arg| (*arg).to_owned()).collect(),
        ),
    };
    ServerCommand {
        program,
        args,
        env: variables,
        cwd: root.to_path_buf(),
    }
}

/// The Java of a prerequisite, or the one on the `PATH`.
fn java_program(extra_env: &[(String, String)]) -> PathBuf {
    extra_env
        .iter()
        .find(|(key, _)| key == "JAVA_HOME")
        .map_or_else(
            || PathBuf::from("java"),
            |(_, home)| Path::new(home).join("bin/java"),
        )
}

/// The Java that comes with VS Code's Java extension:
/// `extension/jre/<version>-macosx-<arch>/bin/java`.
fn bundled_java(extension: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(extension.join("jre"))
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("bin/java"))
        .filter(|java| java.exists())
        .collect();
    found.sort();
    found.pop()
}

/// Whether the project's build files name `text`, such as `lombok`.
fn build_files_mention(root: &Path, text: &str) -> bool {
    ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .any(|name| {
            std::fs::read_to_string(root.join(name)).is_ok_and(|build| build.contains(text))
        })
}

/// Starts jdtls as the Java extension of VS Code does, without needing
/// Python for the `bin/jdtls` script. Its settings and the index of each
/// project stay in Keel's folder of the server: `config` and
/// `workspaces/<project>-<hash>`.
fn jdtls_command(
    launcher: &Path,
    root: &Path,
    extra_env: &[(String, String)],
) -> (PathBuf, Vec<String>) {
    // `<servers>/jdtls/<version>/extension/server/plugins/<launcher>.jar`
    let extension = launcher
        .ancestors()
        .nth(3)
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let version_dir = extension.parent().unwrap_or(&extension);
    let server_dir = version_dir.parent().unwrap_or(version_dir);
    let shared = extension
        .join("server")
        .join(if std::env::consts::ARCH == "aarch64" {
            "config_mac_arm"
        } else {
            "config_mac"
        });
    let mut args: Vec<String> = JDTLS_VM_ARGS.iter().map(|arg| (*arg).to_owned()).collect();
    // Lombok writes code at compile time: without its agent, projects that
    // use it are full of false errors. VS Code adds it the same way.
    if build_files_mention(root, "lombok")
        && let Ok(entries) = std::fs::read_dir(extension.join("lombok"))
        && let Some(jar) = entries
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|extension| extension == "jar"))
    {
        args.push(format!("-javaagent:{}", jar.display()));
    }
    args.extend(
        [
            "-Declipse.application=org.eclipse.jdt.ls.core.id1",
            "-Dosgi.bundles.defaultStartLevel=4",
            "-Declipse.product=org.eclipse.jdt.ls.core.product",
            "-Dosgi.checkConfiguration=true",
            &format!("-Dosgi.sharedConfiguration.area={}", shared.display()),
            "-Dosgi.sharedConfiguration.area.readOnly=true",
            "-Dosgi.configuration.cascaded=true",
            // Needed from Java 24 on, harmless before.
            "-Djdk.xml.maxGeneralEntitySizeLimit=0",
            "-Djdk.xml.totalEntitySizeLimit=0",
            "--add-modules=ALL-SYSTEM",
            "--add-opens",
            "java.base/java.util=ALL-UNNAMED",
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-jar",
        ]
        .map(str::to_owned),
    );
    args.push(launcher.to_string_lossy().into_owned());
    args.push("-configuration".to_owned());
    args.push(server_dir.join("config").to_string_lossy().into_owned());
    args.push("-data".to_owned());
    args.push(
        server_dir
            .join("workspaces")
            .join(project_folder_name(root))
            .to_string_lossy()
            .into_owned(),
    );
    let java = bundled_java(&extension).unwrap_or_else(|| java_program(extra_env));
    (java, args)
}

/// Starts Spring Boot's server as its extension does, on the Java that
/// comes with jdtls when there is one.
fn spring_boot_command(jar: &Path, extra_env: &[(String, String)]) -> (PathBuf, Vec<String>) {
    // `<servers>/spring-boot/<version>/extension/language-server/<jar>`
    let servers = jar.ancestors().nth(5).unwrap_or(Path::new("."));
    let java = installed_jdtls_extension(servers)
        .and_then(|extension| bundled_java(&extension))
        .unwrap_or_else(|| java_program(extra_env));
    let args = [
        "-Xmx1024m",
        "-Dsts.lsp.client=vscode",
        "-Dspring.config.location=classpath:/application.properties",
        "-Djdk.util.zip.disableZip64ExtraFieldValidation=true",
        "-Dspring.main.web-application-type=NONE",
        "-Xlog:jni+resolve=off",
        "-jar",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .chain([jar.to_string_lossy().into_owned()])
    .collect();
    (java, args)
}

/// The `extension` folder of the installed jdtls, if any.
fn installed_jdtls_extension(servers_dir: &Path) -> Option<PathBuf> {
    let spec = spec("jdtls")?;
    let Install::Download { version, .. } = spec.install else {
        return None;
    };
    let extension = install_dir(servers_dir, spec)
        .join(version)
        .join("extension");
    extension.exists().then_some(extension)
}

/// The parts jdtls loads for Spring Boot, when the project uses it and its
/// server is installed.
fn spring_boot_bundles(servers_dir: &Path, root: &Path) -> Vec<String> {
    let Some(spring) = spec("spring-boot") else {
        return Vec::new();
    };
    let Install::Download { version, .. } = spring.install else {
        return Vec::new();
    };
    let folder = install_dir(servers_dir, spring).join(version);
    if !spring.wanted_in(root) || !folder.exists() {
        return Vec::new();
    }
    SPRING_BOOT_JDTLS_BUNDLES
        .iter()
        .map(|bundle| folder.join(bundle))
        .filter(|bundle| bundle.exists())
        .map(|bundle| bundle.to_string_lossy().into_owned())
        .collect()
}

/// What Spring Boot's server asks the editor about Java, and the jdtls
/// command that answers, as VS Code's Spring Boot extension passes them on:
/// `(command, arguments)` for `workspace/executeCommand` on jdtls.
pub fn spring_boot_relay(method: &str, params: &Value) -> Option<(&'static str, Vec<Value>)> {
    let command = match method {
        "sts/addClasspathListener" => "sts.java.addClasspathListener",
        "sts/removeClasspathListener" => "sts.java.removeClasspathListener",
        "sts/javaType" => "sts.java.type",
        "sts/javadocHoverLink" => "sts.java.javadocHoverLink",
        "sts/javaLocation" => "sts.java.location",
        "sts/javadoc" => "sts.java.javadoc",
        "sts/javaSearchTypes" => "sts.java.search.types",
        "sts/javaSearchPackages" => "sts.java.search.packages",
        "sts/javaSubTypes" => "sts.java.hierarchy.subtypes",
        "sts/javaSuperTypes" => "sts.java.hierarchy.supertypes",
        "sts/javaCodeComplete" => "sts.java.code.completions",
        "sts/project/gav" => "sts.project.gav",
        _ => return None,
    };
    // The classpath listeners pass only the command to call back.
    let arguments = if method.ends_with("ClasspathListener") {
        vec![params["callbackCommandId"].clone()]
    } else {
        vec![params.clone()]
    };
    Some((command, arguments))
}

/// A folder name for a project, readable and unique: `Keel-1a2b3c4d5e6f`.
fn project_folder_name(root: &Path) -> String {
    use sha2::{Digest as _, Sha256};
    let hash = Sha256::digest(root.to_string_lossy().as_bytes());
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let short: String = hash
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{name}-{short}")
}

/// The environment of the user's login shell, read once. An app opened from
/// the Finder gets a bare `PATH`, without Homebrew, nvm or `~/.cargo/bin`,
/// so servers and their `node` would not be found. Falls back to Keel's
/// own environment if the shell does not answer within five seconds.
pub fn login_environment() -> &'static HashMap<String, String> {
    static ENVIRONMENT: OnceLock<HashMap<String, String>> = OnceLock::new();
    ENVIRONMENT.get_or_init(|| {
        let mut env: HashMap<String, String> = std::env::vars().collect();
        if let Some(shell_env) = read_shell_environment(Duration::from_secs(5)) {
            env.extend(shell_env);
        }
        env
    })
}

const MARKER: &str = "__KEEL_ENVIRONMENT__";

fn read_shell_environment(timeout: Duration) -> Option<HashMap<String, String>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned());
    let mut child = Command::new(shell)
        .args([
            "-l",
            "-i",
            "-c",
            &format!("printf '{MARKER}'; /usr/bin/env -0"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = Vec::new();
    child.stdout.take()?.read_to_end(&mut output).ok()?;
    parse_environment(&String::from_utf8_lossy(&output))
}

/// Reads `env -0` output, after the marker the shell printed.
fn parse_environment(output: &str) -> Option<HashMap<String, String>> {
    let (_, variables) = output.split_once(MARKER)?;
    Some(
        variables
            .split('\0')
            .filter_map(|entry| entry.split_once('='))
            .filter(|(key, _)| !key.is_empty() && !matches!(*key, "SHLVL" | "PWD" | "OLDPWD" | "_"))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_files_get_typescript_and_emmet() {
        let root = tempfile::tempdir().unwrap();
        let ids = |language: &str| -> Vec<&str> {
            servers_for(language, root.path())
                .iter()
                .map(|spec| spec.id)
                .collect()
        };
        assert_eq!(ids("typescriptreact"), ["vtsls", "emmet"]);
        assert_eq!(ids("typescript"), ["vtsls"]);
        assert_eq!(ids("rust"), ["rust-analyzer"]);
        assert_eq!(ids("toml"), ["taplo"]);
        assert_eq!(ids("python"), ["basedpyright", "ruff"]);
        assert_eq!(ids("plaintext"), Vec::<&str>::new());
    }

    #[test]
    fn eslint_and_tailwind_only_where_the_project_uses_them() {
        let root = tempfile::tempdir().unwrap();
        let has = |id: &str| spec(id).unwrap().wanted_in(root.path());
        assert!(!has("eslint") && !has("tailwindcss"));
        std::fs::write(
            root.path().join("package.json"),
            r#"{ "devDependencies": { "eslint": "^9", "@tailwindcss/postcss": "^4" } }"#,
        )
        .unwrap();
        assert!(has("eslint") && has("tailwindcss"));

        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("eslint.config.mjs"), "export default []").unwrap();
        std::fs::write(other.path().join("tailwind.config.ts"), "export default {}").unwrap();
        assert!(spec("eslint").unwrap().wanted_in(other.path()));
        assert!(spec("tailwindcss").unwrap().wanted_in(other.path()));
    }

    #[test]
    fn servers_from_one_package_share_a_folder() {
        let dir = Path::new("/data/servers");
        assert_eq!(
            install_dir(dir, spec("html").unwrap()),
            install_dir(dir, spec("css").unwrap())
        );
        assert_eq!(
            install_dir(dir, spec("vtsls").unwrap()),
            Path::new("/data/servers/vtsls-language-server")
        );
    }

    #[test]
    fn an_installed_npm_server_is_found_only_at_its_version() {
        let servers = tempfile::tempdir().unwrap();
        let spec = spec("yaml").unwrap();
        let dir = install_dir(servers.path(), spec);
        let package = dir.join("node_modules/yaml-language-server");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/.bin")).unwrap();
        std::fs::write(dir.join("node_modules/.bin/yaml-language-server"), "").unwrap();
        let empty = HashMap::from([("PATH".to_owned(), String::new())]);
        std::fs::write(package.join("package.json"), r#"{ "version": "1.0.0" }"#).unwrap();
        assert_eq!(find(spec, servers.path(), servers.path(), &empty), None);
        std::fs::write(package.join("package.json"), r#"{ "version": "2.0.0" }"#).unwrap();
        assert_eq!(
            find(spec, servers.path(), servers.path(), &empty),
            Some(dir.join("node_modules/.bin/yaml-language-server"))
        );
    }

    #[test]
    fn programs_are_found_on_the_path() {
        let bin = tempfile::tempdir().unwrap();
        let program = bin.path().join("marksman");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let env = HashMap::from([(
            "PATH".to_owned(),
            format!("/nowhere:{}", bin.path().display()),
        )]);
        // Not executable yet.
        assert_eq!(which_in("marksman", &env), None);
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(which_in("marksman", &env), Some(program));
    }

    #[test]
    fn install_commands() {
        let env = HashMap::from([("PATH".to_owned(), "/nowhere".to_owned())]);
        let servers = Path::new("/data/servers");
        let error = install_command(spec("vtsls").unwrap(), servers, &env).unwrap_err();
        assert!(error.to_string().contains("npm"), "{error}");
        let (program, args) = install_command(spec("marksman").unwrap(), servers, &env).unwrap();
        assert_eq!(program, Path::new("/usr/bin/curl"));
        assert_eq!(
            args.last().unwrap(),
            "https://github.com/artempyanykh/marksman/releases/download/2026-02-08/marksman-macos"
        );
        assert_eq!(args[args.len() - 2], "/data/servers/marksman/download.part");
        let (_, args) = install_command(spec("taplo").unwrap(), servers, &env).unwrap();
        assert!(
            args.last()
                .unwrap()
                .ends_with(&format!("taplo-darwin-{}.gz", std::env::consts::ARCH))
        );
    }

    /// A server spec for a download from `archive`, a local file.
    fn download_spec(archive: &Path, unpack: Unpack, sha256: Option<&str>) -> &'static ServerSpec {
        let url: &'static str = Box::leak(format!("file://{}", archive.display()).into_boxed_str());
        let sha256 = sha256.map(|hash| &*Box::leak(hash.to_owned().into_boxed_str()));
        Box::leak(Box::new(ServerSpec {
            id: "tool",
            name: "Tool",
            languages: &["tool"],
            program: "tool",
            args: &[],
            install: Install::Download {
                url,
                version: "1.0",
                unpack,
                sha256,
            },
            activation: Activation::Always,
            needs: &[],
        }))
    }

    #[test]
    fn archives_are_checked_unpacked_and_found() {
        let work = tempfile::tempdir().unwrap();
        let source = work.path().join("source/tool-{arch}");
        let source = PathBuf::from(expand_arch(&source.to_string_lossy()));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("tool"), "#!/bin/sh\necho ok\n").unwrap();
        let env = HashMap::from([("PATH".to_owned(), String::new())]);
        for (name, flags) in [("tool.tar.gz", "-czf"), ("tool.zip", "--format=zip -cf")] {
            let archive = work.path().join(name);
            let mut tar = Command::new("/usr/bin/tar");
            tar.args(flags.split(' '))
                .arg(&archive)
                .arg("-C")
                .arg(work.path().join("source"));
            tar.arg(source.file_name().unwrap());
            assert!(tar.status().unwrap().success());
            let hash = sha256_of(&archive).unwrap();
            let unpack = Unpack::Archive {
                program: "tool-{arch}/tool",
            };
            let servers = work.path().join(format!("servers-{name}"));

            // A different SHA-256 stops the install on Apple silicon.
            let wrong = download_spec(&archive, unpack, Some(&"0".repeat(64)));
            let result = install(wrong, &servers, work.path(), &env);
            if std::env::consts::ARCH == "aarch64" {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("not the expected one"), "{error}");
                assert_eq!(find(wrong, &servers, work.path(), &env), None);
            }

            let spec = download_spec(&archive, unpack, Some(&hash));
            install(spec, &servers, work.path(), &env).unwrap();
            let program = find(spec, &servers, work.path(), &env).expect("installed");
            assert_eq!(
                program,
                install_dir(&servers, spec)
                    .join("1.0")
                    .join(expand_arch("tool-{arch}/tool"))
            );
            let output = Command::new(&program).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n", "{name}");
            assert!(!install_dir(&servers, spec).join(DOWNLOAD).exists());
        }
    }

    #[test]
    fn single_files_and_gzip_are_installed_too() {
        let work = tempfile::tempdir().unwrap();
        let plain = work.path().join("tool");
        std::fs::write(&plain, "#!/bin/sh\necho ok\n").unwrap();
        let env = HashMap::from([("PATH".to_owned(), String::new())]);
        let spec = download_spec(&plain, Unpack::File, None);
        install(spec, &work.path().join("a"), work.path(), &env).unwrap();
        assert!(find(spec, &work.path().join("a"), work.path(), &env).is_some());

        let gzip = work.path().join("tool.gz");
        let compressed = Command::new("/usr/bin/gzip")
            .args(["--stdout"])
            .arg(&plain)
            .output()
            .unwrap();
        std::fs::write(&gzip, compressed.stdout).unwrap();
        let spec = download_spec(&gzip, Unpack::Gzip, None);
        install(spec, &work.path().join("b"), work.path(), &env).unwrap();
        let program = find(spec, &work.path().join("b"), work.path(), &env).unwrap();
        let output = Command::new(&program).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
    }

    #[test]
    fn architectures_in_each_projects_spelling() {
        let expanded = expand_arch("{arch} {aarch_64} {arm64}");
        if std::env::consts::ARCH == "aarch64" {
            assert_eq!(expanded, "aarch64 aarch_64 arm64");
        } else {
            assert_eq!(expanded, "x86_64 x86_64 amd64");
        }
    }

    #[test]
    fn extra_variables_replace_the_shell_ones() {
        let env = HashMap::from([
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("HOME".to_owned(), "/Users/m".to_owned()),
        ]);
        let extra = [("PATH".to_owned(), "/jdk/bin:/usr/bin".to_owned())];
        let command = command(
            spec("taplo").unwrap(),
            PathBuf::from("taplo"),
            Path::new("/p"),
            &env,
            &extra,
        );
        let path: Vec<_> = command
            .env
            .iter()
            .filter(|(key, _)| key == "PATH")
            .collect();
        assert_eq!(path, [&("PATH".to_owned(), "/jdk/bin:/usr/bin".to_owned())]);
        assert!(command.env.iter().any(|(key, _)| key == "HOME"));
    }

    #[test]
    fn python_comes_from_the_projects_environment() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let settings = |user: Value| settings_for(spec("basedpyright").unwrap(), &user, root);
        // No environment: the server uses the `python3` on the `PATH`.
        assert_eq!(settings(json!({}))["python"]["pythonPath"], Value::Null);
        assert_eq!(
            settings(json!({}))["basedpyright"]["analysis"]["typeCheckingMode"],
            json!("standard")
        );

        // A folder named like an environment, but not one, is skipped.
        std::fs::create_dir_all(root.join("env/bin")).unwrap();
        std::fs::write(root.join("env/bin/python"), "").unwrap();
        assert_eq!(settings(json!({}))["python"]["pythonPath"], Value::Null);
        std::fs::write(root.join("env/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        let env_python = root.join("env/bin/python").to_string_lossy().into_owned();
        assert_eq!(
            settings(json!({}))["python"]["pythonPath"],
            json!(env_python)
        );

        // `.venv`, as uv makes it, comes first.
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        std::fs::write(root.join(".venv/bin/python"), "").unwrap();
        std::fs::write(root.join(".venv/pyvenv.cfg"), "uv = 0.13\n").unwrap();
        let venv_python = root.join(".venv/bin/python").to_string_lossy().into_owned();
        assert_eq!(
            settings(json!({}))["python"]["pythonPath"],
            json!(venv_python)
        );

        // The user's choice wins, also relative to the project.
        let chosen = json!({ "python": { "defaultInterpreterPath": "env/bin/python" } });
        assert_eq!(settings(chosen)["python"]["pythonPath"], json!(env_python));
        let fixed = json!({ "python": { "pythonPath": "/opt/python" } });
        assert_eq!(
            settings(fixed)["python"]["pythonPath"],
            json!("/opt/python")
        );
    }

    #[test]
    fn spring_boot_questions_go_to_jdtls() {
        let params = json!({ "callbackCommandId": "sts4.classpath.abc" });
        assert_eq!(
            spring_boot_relay("sts/addClasspathListener", &params),
            Some((
                "sts.java.addClasspathListener",
                vec![json!("sts4.classpath.abc")]
            ))
        );
        let params = json!({ "projectUri": "file:///p", "fqName": "eu.prova.App" });
        assert_eq!(
            spring_boot_relay("sts/javaType", &params),
            Some(("sts.java.type", vec![params.clone()]))
        );
        assert_eq!(spring_boot_relay("sts/moveCursor", &params), None);
    }

    #[test]
    fn spring_boot_only_in_its_projects() {
        let root = tempfile::tempdir().unwrap();
        let spring = spec("spring-boot").unwrap();
        assert!(!spring.wanted_in(root.path()));
        std::fs::write(
            root.path().join("pom.xml"),
            "<parent><artifactId>spring-boot-starter-parent</artifactId></parent>",
        )
        .unwrap();
        assert!(spring.wanted_in(root.path()));
        let ids: Vec<&str> = servers_for("java", root.path())
            .iter()
            .map(|spec| spec.id)
            .collect();
        assert_eq!(ids, ["jdtls", "spring-boot"]);
    }

    #[test]
    fn jdtls_runs_on_its_own_java_with_its_index_in_keels_folder() {
        let servers = tempfile::tempdir().unwrap();
        let extension = servers.path().join("jdtls/1.58.0/extension");
        let java = extension.join("jre/25.0.4.1-macosx-aarch64/bin/java");
        std::fs::create_dir_all(java.parent().unwrap()).unwrap();
        std::fs::write(&java, "").unwrap();
        std::fs::create_dir_all(extension.join("lombok")).unwrap();
        std::fs::write(extension.join("lombok/lombok-1.18.39.jar"), "").unwrap();
        let launcher = servers.path().join("jdtls/1.58.0").join(JDTLS_LAUNCHER);
        let project = tempfile::tempdir().unwrap();
        let env = HashMap::new();
        let command = command(
            spec("jdtls").unwrap(),
            launcher.clone(),
            project.path(),
            &env,
            &[],
        );
        assert_eq!(command.program, java);
        let args = command.args.join(" ");
        assert!(
            args.contains(&format!("-jar {}", launcher.display())),
            "{args}"
        );
        assert!(args.contains(&format!(
            "-configuration {}",
            servers.path().join("jdtls/config").display()
        )));
        assert!(args.contains(&format!(
            "-data {}",
            servers.path().join("jdtls/workspaces").display()
        )));
        assert!(!args.contains("lombok"), "only for projects that use it");
        std::fs::write(
            project.path().join("pom.xml"),
            "<artifactId>lombok</artifactId>",
        )
        .unwrap();
        let command = super::command(spec("jdtls").unwrap(), launcher, project.path(), &env, &[]);
        assert!(
            command
                .args
                .iter()
                .any(|arg| arg.starts_with("-javaagent:") && arg.ends_with("lombok-1.18.39.jar"))
        );

        // Spring Boot's server runs on the same Java.
        let jar = servers
            .path()
            .join("spring-boot/2.4.1")
            .join(SPRING_BOOT_SERVER);
        let spring = super::command(
            spec("spring-boot").unwrap(),
            jar.clone(),
            project.path(),
            &env,
            &[],
        );
        assert_eq!(spring.program, java);
        assert_eq!(spring.args.last().unwrap(), &jar.to_string_lossy());
    }

    #[test]
    fn user_settings_go_over_the_defaults() {
        let user = json!({ "css": { "validate": false }, "editor": { "tabSize": 2 } });
        let settings = settings_for(spec("css").unwrap(), &user, Path::new("/progetto"));
        assert_eq!(settings["css"]["validate"], json!(false));
        assert_eq!(settings["css"]["hover"]["documentation"], json!(true));
        assert_eq!(settings["scss"]["validate"], json!(true));
        assert_eq!(settings["editor"]["tabSize"], json!(2));
    }

    #[test]
    fn eslint_gets_its_own_section_at_the_root() {
        let user = json!({ "eslint": { "run": "onSave" }, "editor": { "tabSize": 2 } });
        let settings = settings_for(
            spec("eslint").unwrap(),
            &user,
            Path::new("/Users/m/Hospitality"),
        );
        assert_eq!(settings["run"], json!("onSave"));
        assert_eq!(settings["validate"], json!("on"));
        assert_eq!(settings["workspaceFolder"]["name"], json!("Hospitality"));
        assert!(settings.get("editor").is_none());
        // Without any ESLint setting, the defaults stay whole.
        let settings = settings_for(spec("eslint").unwrap(), &json!({}), Path::new("/p"));
        assert_eq!(settings["nodePath"], Value::Null);
        assert_eq!(settings["run"], json!("onType"));
    }

    #[test]
    fn the_shell_environment_is_read_after_the_marker() {
        let output = format!(
            "Benvenuto!\n{MARKER}PATH=/opt/homebrew/bin:/usr/bin\0HOME=/Users/m\0SHLVL=2\0EMPTY=\0"
        );
        let env = parse_environment(&output).unwrap();
        assert_eq!(env["PATH"], "/opt/homebrew/bin:/usr/bin");
        assert_eq!(env["EMPTY"], "");
        assert!(!env.contains_key("SHLVL"));
        assert_eq!(parse_environment("no marker"), None);
    }
}

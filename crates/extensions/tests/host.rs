//! The extension host seen from Keel: load extensions, activate them, run
//! their commands and answer what they ask.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use extensions::{ExtensionHost, HostEvent, InstalledExtension, Vsix};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use serde_json::{Value, json};

/// Runs `work` while answering the host's calls with `answer`, and records
/// every event the host sends.
async fn drive<T>(
    host: &ExtensionHost,
    events: &mut UnboundedReceiver<HostEvent>,
    seen: &mut Vec<HostEvent>,
    answer: impl Fn(&str, &Value) -> Value,
    work: impl Future<Output = T>,
) -> T {
    let mut work = std::pin::pin!(work);
    let deadline = tokio::time::sleep(Duration::from_secs(30));
    let mut deadline = std::pin::pin!(deadline);
    loop {
        tokio::select! {
            result = &mut work => {
                while let Ok(event) = events.try_recv() {
                    seen.push(event);
                }
                return result;
            }
            Some(event) = events.next() => {
                if let HostEvent::Call { id, method, params } = &event {
                    host.reply(*id, Ok(answer(method, params))).unwrap();
                }
                seen.push(event);
            }
            _ = &mut deadline => panic!("the extension host did not answer in time"),
        }
    }
}

fn quiet_keel(_method: &str, _params: &Value) -> Value {
    Value::Null
}

fn notifications<'a>(seen: &'a [HostEvent], wanted: &str) -> Vec<&'a Value> {
    seen.iter()
        .filter_map(|event| match event {
            HostEvent::Notify { method, params } if method == wanted => Some(params),
            _ => None,
        })
        .collect()
}

const HELLO_PACKAGE: &str = r#"{
    "name": "hello",
    "publisher": "keel-tests",
    "version": "0.0.1",
    "main": "./extension.js",
    "activationEvents": ["onCommand:hello.greet", "onCommand:hello.later", "onCommand:hello.escape", "onCommand:hello.fs"],
    "contributes": {
        "configuration": { "properties": { "hello.name": { "type": "string", "default": "Keel" } } }
    }
}"#;

const HELLO_EXTENSION: &str = r#"
const vscode = require("vscode");
const path = require("path");
const crypto = require("crypto");
const helper = require("./lib/helper");

exports.activate = function (context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("hello.greet", async (who) => {
      const name = who ?? vscode.workspace.getConfiguration("hello").get("name");
      const answer = await vscode.window.showInformationMessage(`Ciao ${name}`, "Grazie", "Chiudi");
      return {
        answer,
        file: path.basename("/a/b/booking.ts"),
        extension: path.extname("card.tsx"),
        hash: crypto.createHash("sha256").update("keel", "utf8").digest("hex"),
        bytes: Buffer.byteLength("è"),
        text: Buffer.from(Buffer.from("perché", "utf8")).toString("utf8"),
        doubled: helper.double(21),
      };
    }),
    vscode.commands.registerCommand("hello.later", () =>
      new Promise((resolve) => setTimeout(() => resolve("dopo"), 20))),
    vscode.commands.registerCommand("hello.escape", () => {
      try {
        require("/etc/hosts");
        return "escaped";
      } catch (error) {
        return error.message;
      }
    }),
    vscode.commands.registerCommand("hello.fs", async (folder) => {
      const file = vscode.Uri.joinPath(vscode.Uri.file(folder), "nested", "note.txt");
      await vscode.workspace.fs.writeFile(file, Buffer.from("ciao è", "utf8"));
      const text = Buffer.from(await vscode.workspace.fs.readFile(file)).toString("utf8");
      const stat = await vscode.workspace.fs.stat(file);
      let missing;
      try {
        await vscode.workspace.fs.readFile(vscode.Uri.joinPath(vscode.Uri.file(folder), "missing.txt"));
      } catch (error) {
        missing = error.code;
      }
      return { text, type: stat.type, size: stat.size, missing };
    }),
  );
};
"#;

/// Writes the test extension to a folder and returns it, unpacked.
fn hello_extension(folder: &Path) -> InstalledExtension {
    let root = folder.join("hello");
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::write(root.join("package.json"), HELLO_PACKAGE).unwrap();
    fs::write(root.join("extension.js"), HELLO_EXTENSION).unwrap();
    fs::write(
        root.join("lib/helper.js"),
        "exports.double = (n) => n * 2;\n",
    )
    .unwrap();
    InstalledExtension::load(&root).unwrap()
}

struct Setup {
    host: ExtensionHost,
    events: UnboundedReceiver<HostEvent>,
    seen: Vec<HostEvent>,
    folder: tempfile::TempDir,
}

async fn start_with(extension: impl FnOnce(&Path) -> InstalledExtension) -> Setup {
    let folder = tempfile::tempdir().unwrap();
    let installed = extension(folder.path());
    let (host, mut events) = ExtensionHost::start().unwrap();
    let mut seen = Vec::new();
    let storage = folder.path().join("storage");
    drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.load_extension(&installed, &storage),
    )
    .await
    .unwrap();
    Setup {
        host,
        events,
        seen,
        folder,
    }
}

#[tokio::test]
async fn a_command_activates_its_extension_and_uses_node_basics() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(hello_extension).await;

    // Keel answers the message with the first button, "Grazie".
    let answer = |method: &str, _params: &Value| match method {
        "window/showMessage" => json!(0),
        _ => Value::Null,
    };
    let result = drive(
        &host,
        &mut events,
        &mut seen,
        answer,
        host.execute_command("hello.greet", vec![]),
    )
    .await
    .unwrap();

    assert_eq!(
        result,
        json!({
            "answer": "Grazie",
            "file": "booking.ts",
            "extension": ".tsx",
            "hash": sha2_check::sha256_hex("keel"),
            "bytes": 2,
            "text": "perché",
            "doubled": 42,
        })
    );

    let messages = seen.iter().find_map(|event| match event {
        HostEvent::Call { method, params, .. } if method == "window/showMessage" => Some(params),
        _ => None,
    });
    assert_eq!(messages.unwrap()["message"], "Ciao Keel");
    assert!(
        notifications(&seen, "extensions/activated")
            .iter()
            .any(|params| params["id"] == "keel-tests.hello")
    );
    assert!(
        notifications(&seen, "commands/registered")
            .iter()
            .any(|params| params["command"] == "hello.greet")
    );
}

mod sha2_check {
    /// SHA-256 computed independently of the extension host.
    pub fn sha256_hex(text: &str) -> String {
        use std::process::Command;
        let output = Command::new("shasum")
            .args(["-a", "256"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(text.as_bytes())?;
                child.wait_with_output()
            })
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    }
}

#[tokio::test]
async fn timers_fire() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(hello_extension).await;
    let result = drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.execute_command("hello.later", vec![]),
    )
    .await
    .unwrap();
    assert_eq!(result, "dopo");
}

#[tokio::test]
async fn extensions_cannot_load_modules_from_outside_their_folder() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(hello_extension).await;
    let result = drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.execute_command("hello.escape", vec![]),
    )
    .await
    .unwrap();
    assert!(
        result
            .as_str()
            .unwrap()
            .contains("Cannot find module '/etc/hosts'"),
        "{result}"
    );
}

#[tokio::test]
async fn workspace_fs_reads_and_writes_files() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder,
    } = start_with(hello_extension).await;
    let target = folder.path().join("files");
    let result = drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.execute_command("hello.fs", vec![json!(target)]),
    )
    .await
    .unwrap();

    assert_eq!(
        result,
        json!({ "text": "ciao è", "type": 1, "size": 7, "missing": "FileNotFound" })
    );
    assert_eq!(
        fs::read_to_string(target.join("nested/note.txt")).unwrap(),
        "ciao è"
    );
}

#[tokio::test]
async fn a_failing_activation_is_reported() {
    let Setup { host, mut events, mut seen, folder: _folder } = start_with(|folder| {
        let root = folder.join("broken");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{ "name": "broken", "publisher": "keel-tests", "version": "0.0.1", "main": "./extension.js" }"#,
        )
        .unwrap();
        fs::write(root.join("extension.js"), "exports.activate = () => { throw new Error('rotto'); };").unwrap();
        InstalledExtension::load(&root).unwrap()
    })
    .await;

    let error = drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.activate("keel-tests.broken"),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("rotto"), "{error}");
    let failures = notifications(&seen, "extensions/activationFailed");
    assert!(failures[0]["error"].as_str().unwrap().contains("rotto"));
}

fn vsauto_ghost_package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix")
}

#[tokio::test]
async fn vsauto_ghost_activates_and_registers_what_it_needs() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(|folder| {
        Vsix::open(vsauto_ghost_package())
            .unwrap()
            .extract(&folder.join("vsauto-ghost"))
            .unwrap()
    })
    .await;

    drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.activate("manuel-local.vsauto-ghost"),
    )
    .await
    .unwrap();

    let registered: Vec<&str> = notifications(&seen, "commands/registered")
        .iter()
        .filter_map(|params| params["command"].as_str())
        .collect();
    for command in [
        "vsauto.start",
        "vsauto.startWithPrompts",
        "vsauto.stop",
        "vsauto.togglePause",
        "vsauto.fakeKey",
        "vsauto.backspaceKey",
        "vsauto.acceptPreviewAllOrFakeKey",
        "vsauto.requestOrExpandSuggestion",
        "vsauto.acceptCompletionPopup",
        "type",
    ] {
        assert!(
            registered.contains(&command),
            "{command} is not registered: {registered:?}"
        );
    }
    assert_eq!(notifications(&seen, "decorations/createType").len(), 1);
    assert!(!notifications(&seen, "fileSystem/registered").is_empty());
    assert!(!notifications(&seen, "completion/registered").is_empty());
    assert!(notifications(&seen, "extensions/activationFailed").is_empty());
}

const MIRROR_PACKAGE: &str = r#"{
    "name": "mirror",
    "publisher": "keel-tests",
    "version": "0.0.1",
    "main": "./extension.js",
    "activationEvents": ["*"]
}"#;

const MIRROR_EXTENSION: &str = r#"
const vscode = require("vscode");

exports.activate = function (context) {
  const ghost = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor("editorGhostText.foreground"),
    fontStyle: "italic",
  });
  context.subscriptions.push(
    vscode.commands.registerCommand("mirror.edit", async () => {
      const editor = vscode.window.activeTextEditor;
      const document = editor.document;
      const before = document.getText();
      const position = document.positionAt(before.indexOf("più"));
      const edit = new vscode.WorkspaceEdit();
      edit.insert(document.uri, new vscode.Position(1, 0), "// ghost\n");
      const applied = await vscode.workspace.applyEdit(edit);
      editor.selection = new vscode.Selection(1, 2, 1, 8);
      editor.setDecorations(ghost, [new vscode.Range(1, 0, 1, 8)]);
      return {
        before,
        firstLine: document.lineAt(0).text,
        position: [position.line, position.character],
        offset: document.offsetAt(new vscode.Position(2, 2)),
        applied,
        after: document.getText(),
        version: document.version,
        lineCount: document.lineCount,
        languageId: document.languageId,
        visible: vscode.window.visibleTextEditors.length,
        tabs: vscode.window.tabGroups.all.length,
      };
    }),
    vscode.languages.registerCompletionItemProvider({ language: "typescript" }, {
      provideCompletionItems: () => [
        new vscode.CompletionItem("alpha", vscode.CompletionItemKind.Variable),
        new vscode.CompletionItem("beta", vscode.CompletionItemKind.Function),
      ],
    }),
  );
};
"#;

fn mirror_extension(folder: &Path) -> InstalledExtension {
    let root = folder.join("mirror");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), MIRROR_PACKAGE).unwrap();
    fs::write(root.join("extension.js"), MIRROR_EXTENSION).unwrap();
    InstalledExtension::load(&root).unwrap()
}

#[tokio::test]
async fn the_bridge_mirrors_documents_editors_and_edits() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(mirror_extension).await;
    let uri = "file:///tmp/app.ts";
    let text = "const a = 1;\nè più bello\n";
    host.notify(
        "documents/opened",
        json!({ "uri": uri, "languageId": "typescript", "version": 1, "text": text, "isDirty": false }),
    )
    .unwrap();
    host.notify(
        "editors/changed",
        json!({
            "editors": [{
                "id": "editor-1",
                "uri": uri,
                "viewColumn": 1,
                "selections": [{ "anchor": { "line": 0, "character": 0 }, "active": { "line": 0, "character": 0 } }],
            }],
            "activeId": "editor-1",
        }),
    )
    .unwrap();

    // Keel applies the edit, sends the new text, then answers.
    let edited = "const a = 1;\n// ghost\nè più bello\n";
    let answer = |method: &str, _params: &Value| match method {
        "workspace/applyEdit" => {
            host.notify(
                "documents/changed",
                json!({ "uri": uri, "version": 2, "text": edited, "isDirty": true }),
            )
            .unwrap();
            json!(true)
        }
        _ => Value::Null,
    };
    let result = drive(
        &host,
        &mut events,
        &mut seen,
        answer,
        host.execute_command("mirror.edit", vec![]),
    )
    .await
    .unwrap();

    assert_eq!(
        result,
        json!({
            "before": text,
            "firstLine": "const a = 1;",
            "position": [1, 2],
            "offset": 24,
            "applied": true,
            "after": edited,
            "version": 2,
            "lineCount": 4,
            "languageId": "typescript",
            "visible": 1,
            "tabs": 1,
        })
    );

    let edits = seen.iter().find_map(|event| match event {
        HostEvent::Call { method, params, .. } if method == "workspace/applyEdit" => Some(params),
        _ => None,
    });
    assert_eq!(
        edits.unwrap()["entries"][0]["edits"][0],
        json!({ "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 0 } }, "newText": "// ghost\n" })
    );
    assert_eq!(
        notifications(&seen, "editor/setSelections")[0]["selections"],
        json!([{ "anchor": { "line": 1, "character": 2 }, "active": { "line": 1, "character": 8 } }])
    );
    let decorations = notifications(&seen, "editor/setDecorations");
    assert_eq!(
        decorations[0]["ranges"][0]["range"],
        json!({ "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 8 } })
    );
    assert_eq!(
        notifications(&seen, "decorations/createType")[0]["options"]["color"],
        json!({ "themeColor": "editorGhostText.foreground" })
    );

    let completions = drive(
        &host,
        &mut events,
        &mut seen,
        quiet_keel,
        host.call(
            "completion/provide",
            json!({ "uri": uri, "position": { "line": 0, "character": 0 }, "triggerCharacter": null }),
        ),
    )
    .await
    .unwrap();
    let labels: Vec<&str> = completions
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["alpha", "beta"]);
}

#[tokio::test]
async fn user_settings_override_the_extension_defaults() {
    let Setup {
        host,
        mut events,
        mut seen,
        folder: _folder,
    } = start_with(hello_extension).await;
    host.notify(
        "configuration/changed",
        json!({ "values": { "hello.name": "Manuel" } }),
    )
    .unwrap();

    let answer = |method: &str, _params: &Value| match method {
        "window/showMessage" => json!(1),
        _ => Value::Null,
    };
    drive(
        &host,
        &mut events,
        &mut seen,
        answer,
        host.execute_command("hello.greet", vec![]),
    )
    .await
    .unwrap();

    let message = seen.iter().find_map(|event| match event {
        HostEvent::Call { method, params, .. } if method == "window/showMessage" => Some(params),
        _ => None,
    });
    assert_eq!(message.unwrap()["message"], "Ciao Manuel");
}

/// An extension stuck in an endless loop cannot keep Keel from closing
/// the window: dropping the host stops it within a few seconds.
#[test]
fn a_stuck_extension_does_not_hold_the_window() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("stuck");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "stuck", "publisher": "keel-tests", "version": "1.0.0", "main": "./extension.js", "activationEvents": ["*"] }"#,
    )
    .unwrap();
    fs::write(
        root.join("extension.js"),
        "exports.activate = () => { while (true) {} };",
    )
    .unwrap();
    let extension = InstalledExtension::load(&root).unwrap();
    let (host, _events) = ExtensionHost::start().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(host.load_extension(&extension, folder.path()))
        .unwrap();
    // Never answered: the loop holds the host's thread.
    // A free host answers a ping at once...
    let ping =
        runtime.block_on(async { tokio::time::timeout(Duration::from_secs(2), host.ping()).await });
    assert!(matches!(ping, Ok(Ok(()))), "{ping:?}");
    let activation = host.activate(&extension.id());
    std::thread::sleep(Duration::from_millis(200));
    assert!(host.is_running());
    // ...a stuck one never does, and says what it was doing.
    let ping = runtime
        .block_on(async { tokio::time::timeout(Duration::from_millis(500), host.ping()).await });
    assert!(ping.is_err(), "the stuck host answered: {ping:?}");
    assert_eq!(host.waiting_for(), ["extensions/activate keel-tests.stuck"]);
    let started = std::time::Instant::now();
    drop(host);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let result = runtime.block_on(activation);
    assert!(result.is_err());
}

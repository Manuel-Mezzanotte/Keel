//! Checks on gpui-component's editor for the behaviour Keel relies on:
//! accents, multiple cursors, line endings and large files (phase 0.3).

use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, IntoElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window,
};
use gpui_component::input::{Editor, EditorState};

/// A window that only holds an editor.
struct Host {
    editor: Entity<EditorState>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.editor).size_full()
    }
}

fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    language: &'static str,
    text: String,
) -> (Entity<EditorState>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        language::register_grammars();
    });
    let (host, cx) = cx.add_window_view(|window, cx| {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .default_value(text)
        });
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        Host { editor }
    });
    cx.run_until_parked();
    let editor = host.read_with(cx, |host, _| host.editor.clone());
    (editor, cx)
}

fn value(editor: &Entity<EditorState>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.value().to_string())
}

#[gpui::test]
fn italian_letters_are_typed_as_they_are(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, "typescript", String::new());

    cx.simulate_input("perché è già più così");

    assert_eq!(value(&editor, cx), "perché è già più così");
}

#[gpui::test]
fn dead_keys_compose_accented_letters(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, "typescript", "x".to_owned());

    // macOS sends a dead key as marked text, then replaces it with the
    // composed letter once the second key arrives.
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_and_mark_text_in_range(None, "`", None, window, cx);
        assert!(editor.marked_text_range(window, cx).is_some());
        editor.replace_text_in_range(None, "è", window, cx);
        assert!(editor.marked_text_range(window, cx).is_none());
    });

    assert_eq!(value(&editor, cx), "èx");
}

#[gpui::test]
fn multiple_cursors_type_on_every_line(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, "typescript", "one;\ntwo;\nthree;\n".to_owned());

    cx.simulate_keystrokes("cmd-alt-down cmd-alt-down");
    cx.simulate_input("// ");

    assert_eq!(value(&editor, cx), "// one;\n// two;\n// three;\n");
}

#[gpui::test]
fn windows_line_endings_are_kept(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, "typescript", "one;\r\ntwo;\r\n".to_owned());

    cx.simulate_input("x");

    assert_eq!(value(&editor, cx), "xone;\r\ntwo;\r\n");
}

#[gpui::test]
fn undo_and_redo_restore_the_text(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, "typescript", "const a = 1;".to_owned());

    cx.simulate_input("x");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(value(&editor, cx), "const a = 1;");
    cx.simulate_keystrokes("cmd-shift-z");
    assert_eq!(value(&editor, cx), "xconst a = 1;");
}

/// A TypeScript file of about `lines` lines.
fn large_typescript_file(lines: usize) -> String {
    let mut text = String::with_capacity(lines * 48);
    for index in 0..lines / 4 {
        text.push_str(&format!(
            "export function total{index}(a: number, b: number): number {{\n  // è più veloce\n  return a + b * {index};\n}}\n"
        ));
    }
    text
}

fn measure(lines: usize, cx: &mut TestAppContext) -> (Duration, Duration) {
    let text = large_typescript_file(lines);
    let started = Instant::now();
    let (editor, cx) = open_editor(cx, "typescript", text);
    let opened = started.elapsed();

    let started = Instant::now();
    editor.update_in(cx, |editor, window, cx| {
        editor.set_cursor_position(gpui_component::input::Position::new(0, 0), window, cx);
    });
    cx.simulate_input("x");
    cx.run_until_parked();
    let typed = started.elapsed();
    assert!(value(&editor, cx).starts_with("xexport function total0"));
    (opened, typed)
}

/// Run with `cargo test --release -p keel -- --ignored --nocapture large_file`.
#[gpui::test]
#[ignore = "timing check, run on demand in release mode"]
fn large_file_50k_lines(cx: &mut TestAppContext) {
    let (opened, typed) = measure(50_000, cx);
    println!("50,000 lines: opened in {opened:?}, one keystroke in {typed:?}");
}

/// Run with `cargo test --release -p keel -- --ignored --nocapture large_file`.
#[gpui::test]
#[ignore = "timing check, run on demand in release mode"]
fn large_file_200k_lines(cx: &mut TestAppContext) {
    let (opened, typed) = measure(200_000, cx);
    println!("200,000 lines: opened in {opened:?}, one keystroke in {typed:?}");
}

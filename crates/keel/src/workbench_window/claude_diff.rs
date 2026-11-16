//! Edits Claude Code proposes from the terminal (`openDiff`), shown in Keel
//! as differences to accept or reject.

use futures::channel::oneshot;

use super::*;

impl Workbench {
    pub(super) fn answer_claude_diff_tool(
        &mut self,
        name: &str,
        _arguments: &Value,
        reply: oneshot::Sender<Value>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let mut result = ai::ide::text_result(format!("Keel does not offer {name} yet"));
        result["isError"] = json!(true);
        let _ = reply.send(result);
    }
}

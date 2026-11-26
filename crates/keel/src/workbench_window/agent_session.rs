//! Claude's panel, beyond the chat: the session's options (mode, model,
//! effort, fast mode), who Claude works as, and logging in and out.
//!
//! - **Logging in** runs the agent's own login (`claude auth login`, for a
//!   Claude subscription or for the Anthropic Console) in a terminal of the
//!   bottom panel, where the browser opens; Keel watches for the login to
//!   land and then opens the chat.
//! - **An API key of one's own** is kept in the login keychain, never in a
//!   file, and given to the agent as `ANTHROPIC_API_KEY` when it starts.
//! - **Logging out** is the agent's: the next start asks again.
//! - **The chats are Claude Code's**: one continued in the terminal, or in
//!   Claude Code anywhere else, is read again when Keel comes back to it.

use std::time::Duration;

use ai::session_meta::{self, Account, ConfigOption};
use anyhow::Result;
use gpui_component::input::InputState;

use super::agent_panel::{AgentStatus, AgentView};
use super::*;

/// Where the API key is kept in the login keychain.
const KEY_SERVICE: &str = "Keel · Anthropic API key";
const KEY_ACCOUNT: &str = "keel";

/// The user's API key, from the keychain.
pub(super) fn saved_api_key() -> Option<String> {
    #[cfg(not(test))]
    {
        security_framework::passwords::get_generic_password(KEY_SERVICE, KEY_ACCOUNT)
            .ok()
            .and_then(|key| String::from_utf8(key).ok())
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())
    }
    #[cfg(test)]
    {
        None
    }
}

fn keep_api_key(key: &str) -> Result<()> {
    security_framework::passwords::set_generic_password(KEY_SERVICE, KEY_ACCOUNT, key.as_bytes())?;
    Ok(())
}

fn forget_api_key() -> Result<()> {
    security_framework::passwords::delete_generic_password(KEY_SERVICE, KEY_ACCOUNT)?;
    Ok(())
}

/// Whether `key` looks like an Anthropic API key.
pub(super) fn looks_like_api_key(key: &str) -> bool {
    let key = key.trim();
    key.starts_with("sk-ant-") && key.len() > 20 && !key.contains(char::is_whitespace)
}

/// How long the plan's limits are good for before `/usage` runs again.
const USAGE_FRESH: Duration = Duration::from_secs(60);
/// A Claude app chat used this recently may still be open there.
const APP_CHAT_BUSY: i64 = 10 * 60;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Empties the folder where Claude Code kept the `/usage` sessions: they
/// are Keel's own, and only their files are taken away.
fn clear_usage_folder(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            let _ = std::fs::remove_file(&path);
        } else if path.is_dir() {
            // Only if empty, such as `memory`.
            let _ = std::fs::remove_dir(&path);
        }
    }
    let _ = std::fs::remove_dir(folder);
}

/// How often Keel asks whether the login landed, and for how long.
const LOGIN_POLL: Duration = Duration::from_secs(2);
const LOGIN_WAIT: Duration = Duration::from_secs(600);

impl Workbench {
    /// Takes the options of a session's answer or update: a full list
    /// replaces Keel's, a partial one updates the options it names.
    pub(super) fn apply_agent_options(&mut self, message: &Value) {
        let options = session_meta::config_options(message);
        if options.is_empty() {
            return;
        }
        let known = &mut self.agent_panel.options;
        if known.is_empty() || options.len() > 1 {
            *known = options;
            return;
        }
        for option in options {
            match known.iter_mut().find(|known| known.id == option.id) {
                // An update may name only the value chosen: the list stays.
                Some(known) if option.values.len() <= 1 => known.current = option.current,
                Some(known) => *known = option,
                None => known.push(option),
            }
        }
    }

    /// An option's value changed in the agent.
    pub(super) fn agent_option_changed(&mut self, id: &str, value: String) {
        if let Some(option) = self
            .agent_panel
            .options
            .iter_mut()
            .find(|option| option.id == id || option.category.as_deref() == Some(id))
        {
            option.current = Some(value);
        }
    }

    /// The option of `category` (`mode`, `model`, `thought_level`...).
    pub(super) fn agent_option(&self, category: &str) -> Option<&ConfigOption> {
        self.agent_panel
            .options
            .iter()
            .find(|option| option.category.as_deref() == Some(category) || option.id == category)
    }

    /// Changes an option of the session, such as the model.
    pub(super) fn set_agent_option(&mut self, id: String, value: String, cx: &mut Context<Self>) {
        let (Some(agent), Some(session)) = (
            self.agent_panel.agent.clone(),
            self.agent_panel.session.clone(),
        ) else {
            return;
        };
        let id = self
            .agent_option(&id)
            .map_or(id.clone(), |option| option.id.clone());
        self.agent_option_changed(&id, value.clone());
        let changed = agent.set_config_option(&session, &id, &value);
        cx.spawn(async move |this, cx| {
            let changed = changed.await;
            this.update(cx, |this, cx| {
                match changed {
                    Ok(answer) => this.apply_agent_options(&answer),
                    Err(error) => this.agent_panel.notice = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The session did not open: a missing login asks for one, anything
    /// else is told.
    pub(super) fn agent_failed_to_open(&mut self, error: &anyhow::Error) {
        let login = error
            .downcast_ref::<ai::acp::AgentError>()
            .is_some_and(|error| session_meta::needs_login(error.code, &error.message))
            || format!("{error:#}").to_lowercase().contains("login");
        if login {
            self.agent_panel.needs_login = true;
            self.agent_panel.status = AgentStatus::Ready;
        } else {
            self.agent_panel.status = AgentStatus::Failed(format!("{error:#}"));
        }
    }

    /// Who Claude works as changed: a login that landed opens the chat.
    pub(super) fn agent_account_changed(
        &mut self,
        account: Option<Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = account else {
            return;
        };
        let signed_in = account.signed_in;
        self.agent_panel.account = Some(account);
        if !signed_in {
            self.agent_panel.needs_login = true;
            return;
        }
        if self.agent_panel.needs_login || self.agent_panel.login.is_some() {
            self.agent_panel.needs_login = false;
            if let Some((method, _)) = self.agent_panel.login.take()
                && let Some(agent) = self.agent_panel.agent.clone()
            {
                // The agent reads the new login before the session opens.
                let told = agent.authenticate(&method);
                cx.spawn_in(window, async move |this, cx| {
                    let _ = told.await;
                    this.update_in(cx, |this, window, cx| {
                        if this.agent_panel.session.is_none() {
                            this.new_agent_conversation(window, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            } else if self.agent_panel.session.is_none() && self.agent_panel.agent.is_some() {
                self.new_agent_conversation(window, cx);
            }
        }
        cx.notify();
    }

    /// Runs the login of `method` in a terminal of the bottom panel, and
    /// waits for it to land.
    pub(super) fn login_agent(
        &mut self,
        method: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self
            .agent_panel
            .auth_methods
            .iter()
            .find(|known| known.id == method)
            .and_then(|known| known.terminal.clone())
        else {
            return;
        };
        let (program, args) = command;
        let mut spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        spawn.env.extend(lsp::servers::login_environment().clone());
        spawn.command = Some((program.display().to_string(), args));
        if let Some(view) = self.add_terminal(spawn, None, true, window, cx) {
            let strings = self.strings();
            self.name_terminal(&view, strings.agent_login_terminal.to_owned());
        }
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        let wait = cx.spawn_in(window, async move |this, cx| {
            let started = std::time::Instant::now();
            while started.elapsed() < LOGIN_WAIT {
                cx.background_executor().timer(LOGIN_POLL).await;
                let Ok(status) = agent.auth_status().await else {
                    continue;
                };
                let account = session_meta::account(&status);
                if account.as_ref().is_some_and(|account| account.signed_in) {
                    this.update_in(cx, |this, window, cx| {
                        this.agent_account_changed(account, window, cx)
                    })
                    .ok();
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.agent_panel.login = None;
                cx.notify();
            })
            .ok();
        });
        self.agent_panel.login = Some((method, wait));
        cx.notify();
    }

    /// Logs Claude out; the panel then offers the ways in.
    pub(super) fn logout_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_panel.uses_api_key {
            self.forget_agent_api_key(window, cx);
            return;
        }
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        if self.agent_panel.running {
            self.cancel_agent(cx);
        }
        let out = agent.logout();
        cx.spawn_in(window, async move |this, cx| {
            let out = out.await;
            this.update(cx, |this, cx| {
                match out {
                    Ok(_) => {
                        // The chat stays in Claude Code's files, for later.
                        this.agent_panel.needs_login = true;
                        this.agent_panel.session = None;
                        this.agent_panel.chat_folder = None;
                        this.agent_panel.reset_thread();
                        if let Some(account) = &mut this.agent_panel.account {
                            account.signed_in = false;
                        }
                    }
                    Err(error) => this.agent_panel.notice = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens the field for an API key of one's own.
    pub(super) fn ask_agent_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(strings.agent_key_placeholder)
                .masked(true)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.agent_panel.key_input = Some(input);
        cx.notify();
    }

    /// Keeps the key typed in the keychain and starts Claude again with it.
    pub(super) fn save_agent_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.agent_panel.key_input.clone() else {
            return;
        };
        let key = input.read(cx).value().trim().to_owned();
        if !looks_like_api_key(&key) {
            self.agent_panel.notice = Some(self.strings().agent_key_invalid.to_owned());
            cx.notify();
            return;
        }
        if let Err(error) = keep_api_key(&key) {
            self.agent_panel.notice = Some(format!("{error:#}"));
            cx.notify();
            return;
        }
        self.agent_panel.key_input = None;
        self.agent_panel.notice = None;
        self.restart_agent(window, cx);
    }

    /// Takes the key out of the keychain: Claude goes back to the account.
    pub(super) fn forget_agent_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = forget_api_key() {
            self.log(format!("Claude: {error:#}"));
        }
        self.restart_agent(window, cx);
    }

    /// The file Claude Code keeps the chat `id` in.
    fn agent_chat_file(&self, id: &str) -> Option<PathBuf> {
        self.chat_file_in(id, &self.agent_chat_folder())
    }

    /// The file of the chat `id` of the folder `cwd`; none in the tests.
    fn chat_file_in(&self, id: &str, cwd: &Path) -> Option<PathBuf> {
        if self.agent_panel.fake {
            return None;
        }
        let claude = ai::history::claude_dir()?;
        let folder = ai::history::project_folder(&claude, cwd);
        Some(folder.join(format!("{id}.jsonl")))
    }

    /// The open chat, as the session keeps it: one with something said,
    /// or the one that waits to open again.
    pub(super) fn open_chat_state(&self) -> Option<crate::session::ChatState> {
        let panel = &self.agent_panel;
        if let Some((id, folder)) = &panel.restored {
            return Some(crate::session::ChatState {
                id: id.clone(),
                folder: folder.clone(),
            });
        }
        let id = panel.session.clone()?;
        (!panel.thread.entries.is_empty()).then(|| crate::session::ChatState {
            id,
            folder: panel.chat_folder.clone(),
        })
    }

    /// Opens again the chat the window had open when it closed, if its file
    /// is still there. Whether it did.
    pub(super) fn resume_restored_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((id, folder)) = self.agent_panel.restored.take() else {
            return false;
        };
        let cwd = folder.clone().unwrap_or_else(|| self.agent_folder());
        if self
            .chat_file_in(&id, &cwd)
            .is_some_and(|file| !file.is_file())
        {
            return false;
        }
        self.open_agent_chat(id, folder, window, cx);
        true
    }

    /// Notes when the open chat's file changed, as Keel has it now.
    pub(super) fn mark_agent_chat_synced(&mut self) {
        self.agent_panel.synced = self
            .agent_panel
            .session
            .as_deref()
            .and_then(|id| self.agent_chat_file(id))
            .and_then(|file| {
                std::fs::metadata(file)
                    .and_then(|meta| meta.modified())
                    .ok()
            });
    }

    /// Reads the open chat again when Claude Code changed it elsewhere, in
    /// the terminal or another editor; the list of chats too.
    pub(super) fn sync_agent_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = &self.agent_panel;
        if panel.running || !panel.permissions.is_empty() || panel.agent.is_none() {
            return;
        }
        self.refresh_agent_history(window, cx);
        let Some(id) = self.agent_panel.session.clone() else {
            return;
        };
        let changed = self.agent_chat_file(&id).and_then(|file| {
            std::fs::metadata(file)
                .and_then(|meta| meta.modified())
                .ok()
        });
        let newer = match (changed, self.agent_panel.synced) {
            (Some(changed), Some(known)) => changed > known,
            (Some(_), None) => !self.agent_panel.thread.entries.is_empty(),
            _ => false,
        };
        if newer {
            self.reload_agent_chat(id, window, cx);
        }
    }

    /// Closes the chat in the agent and opens it again from its file.
    fn reload_agent_chat(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        let closed = agent.close_session(&id);
        cx.spawn_in(window, async move |this, cx| {
            // An agent without `session/close` reads it again all the same.
            let _ = closed.await;
            this.update_in(cx, |this, window, cx| {
                if this.agent_panel.session.as_deref() == Some(id.as_str()) {
                    this.load_agent_conversation(id, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// A chat of the Claude app used there in the last minutes: writing in
    /// both places at once would split it, so the panel says so.
    pub(super) fn note_app_chat(&mut self, id: &str) {
        let now = unix_now();
        let panel = &self.agent_panel;
        let busy = panel
            .chats
            .iter()
            .chain(panel.other_chats.iter().flat_map(|(_, chats)| chats))
            .any(|chat| chat.id == id && chat.app && now - chat.updated < APP_CHAT_BUSY);
        if busy {
            self.agent_panel.notice = Some(self.strings().agent_app_chat_busy.to_owned());
        }
    }

    /// A `usage_update` of the open chat: how full its context is, what it
    /// cost, and maybe how far a limit of the plan is.
    pub(super) fn agent_usage_update(&mut self, update: &Value) {
        if let Some(context) = session_meta::context_usage(update) {
            let cost = context.cost.or(self
                .agent_panel
                .context
                .as_ref()
                .and_then(|known| known.cost));
            self.agent_panel.context = Some(session_meta::ContextUsage { cost, ..context });
        }
        let limit = &update["_meta"]["_claude/rateLimit"];
        if limit.is_object() {
            self.agent_panel
                .plan_usage
                .get_or_insert_default()
                .take_rate_limit(limit);
        }
    }

    /// Asks Claude Code for the plan's limits: `/usage` in a session of its
    /// own, away from the project, deleted as soon as it answered. It is
    /// Claude Code's local command: no tokens are spent.
    pub(super) fn refresh_plan_usage(&mut self, force: bool, cx: &mut Context<Self>) {
        let panel = &self.agent_panel;
        let fresh = panel
            .usage_read
            .is_some_and(|read| read.elapsed() < USAGE_FRESH);
        if panel.reading_usage || (fresh && !force) || panel.uses_api_key {
            return;
        }
        let Some(agent) = panel.agent.clone() else {
            return;
        };
        let folder = self
            .data_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join("claude-usage");
        let _ = std::fs::create_dir_all(&folder);
        self.agent_panel.reading_usage = true;
        let fake = self.agent_panel.fake;
        let opened = agent.new_session(&folder);
        cx.spawn(async move |this, cx| {
            let answer = async {
                let session = opened.await?;
                let id = session["sessionId"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("no session"))?
                    .to_owned();
                this.update(cx, |this, _| {
                    this.agent_panel.usage_session = Some((id.clone(), String::new()))
                })?;
                let turn = agent.prompt(&id, vec![ai::acp::text_block("/usage")]).await;
                let _ = agent.delete_session(&id).await;
                turn.map(|_| id)
            }
            .await;
            // What Claude Code still left of the throwaway session.
            if !fake && let Some(claude) = ai::history::claude_dir() {
                let left = ai::history::project_folder(&claude, &folder);
                cx.background_spawn(async move { clear_usage_folder(&left) })
                    .await;
            }
            this.update(cx, |this, cx| {
                let panel = &mut this.agent_panel;
                panel.reading_usage = false;
                let text = panel.usage_session.take().map(|(_, text)| text);
                match (
                    answer,
                    text.and_then(|text| session_meta::plan_usage(&text, unix_now())),
                ) {
                    (Ok(_), Some(usage)) => {
                        panel.plan_usage = Some(usage);
                        panel.usage_read = Some(std::time::Instant::now());
                    }
                    (answer, _) => {
                        if let Err(error) = answer {
                            this.log(format!("Claude: /usage: {error:#}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The bin on a chat: the first click asks, the second deletes it, in
    /// Claude Code, for good. A chat of the Claude app is the app's to
    /// delete.
    pub(super) fn ask_delete_agent_chat(&mut self, id: String, cx: &mut Context<Self>) {
        let app = self
            .agent_panel
            .chats
            .iter()
            .any(|chat| chat.id == id && chat.app);
        if app {
            self.agent_panel.deleting = None;
            self.agent_panel.notice = Some(self.strings().agent_app_chat_delete.to_owned());
            self.agent_panel.show_chats = false;
        } else if self.agent_panel.deleting.as_deref() == Some(id.as_str()) {
            self.agent_panel.deleting = None;
            self.delete_agent_chat(id, cx);
        } else {
            self.agent_panel.deleting = Some(id);
        }
        cx.notify();
    }

    /// Deletes a chat in Claude Code; with the agent off, once it runs.
    pub(super) fn delete_agent_chat(&mut self, id: String, cx: &mut Context<Self>) {
        self.agent_panel.chats.retain(|chat| chat.id != id);
        self.agent_panel.deleted_chats.insert(id.clone());
        let Some(agent) = self.agent_panel.agent.clone() else {
            self.agent_panel.pending_deletes.push(id);
            cx.notify();
            return;
        };
        if self.agent_panel.session.as_deref() == Some(id.as_str()) {
            if self.agent_panel.running {
                self.cancel_agent(cx);
            }
            self.agent_panel.reset_thread();
            self.agent_panel.session = None;
            self.agent_panel.chat_folder = None;
        }
        let deleted = agent.delete_session(&id);
        cx.spawn(async move |this, cx| {
            let deleted = deleted.await;
            this.update(cx, |this, cx| {
                if let Err(error) = deleted {
                    this.agent_panel.notice = Some(format!("{error:#}"));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Goes on with a chat here, in this window's panel. A chat of another
    /// project (`folder`) keeps working in its own folder, where it began:
    /// its history speaks of that folder's files, and Claude Code keeps
    /// chats by folder.
    pub(super) fn open_agent_chat(
        &mut self,
        id: String,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel.chat_folder = folder.filter(|folder| *folder != self.agent_folder());
        self.layout.agent = true;
        self.agent_panel.view = AgentView::Agent;
        self.load_agent_conversation(id, window, cx);
        self.session_changed(cx);
        cx.notify();
    }

    /// Opens the folder of the open chat as a project, in its own window;
    /// the chat stays here.
    pub(super) fn open_chat_project(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.agent_panel.chat_folder.clone() else {
            return;
        };
        cx.defer(move |cx| crate::windows::open(Some(folder), cx));
    }

    /// Stops the agent and starts it again, as after a new key.
    pub(super) fn restart_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_panel.running {
            self.cancel_agent(cx);
        }
        self.agent_panel.agent = None;
        self.agent_panel.session = None;
        self.agent_panel.chat_folder = None;
        self.agent_panel.account = None;
        self.agent_panel.needs_login = false;
        self.agent_panel.login = None;
        self.agent_panel.status = AgentStatus::Off;
        self.agent_panel.reset_thread();
        self.ensure_agent(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_looks_like_a_key_is_kept() {
        assert!(looks_like_api_key(
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz"
        ));
        assert!(looks_like_api_key(
            "  sk-ant-api03-abcdefghijklmnopqrstuvwxyz\n"
        ));
        assert!(!looks_like_api_key("sk-ant-short"));
        assert!(!looks_like_api_key("my password"));
        assert!(!looks_like_api_key("sk-ant-api03-abc def ghi jkl mno pqr"));
    }
}

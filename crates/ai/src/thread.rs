//! A conversation with the agent, built from ACP's `session/update`
//! notifications: messages streamed in pieces, the agent's reasoning, tool
//! calls with their status and results (file differences among them), the
//! plan, and the commands and modes the agent offers.

use std::path::PathBuf;

use serde_json::Value;

/// What a tool call shows.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolContent {
    Text(String),
    /// A file's change: `old` is none for a new file.
    Diff {
        path: PathBuf,
        old: Option<String>,
        new: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    fn parse(value: &Value) -> Option<Self> {
        Some(match value.as_str()? {
            "pending" => Self::Pending,
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub title: String,
    /// `read`, `edit`, `delete`, `move`, `search`, `execute`, `think`,
    /// `fetch`, `other`.
    pub kind: String,
    pub status: ToolStatus,
    pub content: Vec<ToolContent>,
    /// Files it touches, with a line when known.
    pub locations: Vec<(PathBuf, Option<u32>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub content: String,
    /// `pending`, `in_progress`, `completed`.
    pub status: String,
}

/// One block of the conversation, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    User(String),
    Agent(String),
    Thought(String),
    Tool(ToolCall),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thread {
    pub entries: Vec<Entry>,
    /// The latest plan: each update replaces it.
    pub plan: Vec<PlanItem>,
    /// Slash commands the agent offers: name and description.
    pub commands: Vec<(String, String)>,
    pub mode: Option<String>,
    pub title: Option<String>,
    /// The agent's id of the last message in each answer, by entry: a
    /// chat forks from there (`session/fork`).
    pub message_ids: std::collections::HashMap<usize, String>,
}

/// The text of a content block; links and resources by their name.
fn block_text(block: &Value) -> String {
    match block["type"].as_str() {
        Some("text") => block["text"].as_str().unwrap_or_default().to_owned(),
        Some("resource_link") => format!("@{}", block["name"].as_str().unwrap_or_default()),
        Some("resource") => block["resource"]["uri"]
            .as_str()
            .map(|uri| format!("@{}", uri.rsplit('/').next().unwrap_or(uri)))
            .unwrap_or_default(),
        Some("image") => "[image]".to_owned(),
        _ => String::new(),
    }
}

fn parse_content(items: &Value) -> Vec<ToolContent> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| match item["type"].as_str()? {
            "diff" => Some(ToolContent::Diff {
                path: PathBuf::from(item["path"].as_str()?),
                old: item["oldText"].as_str().map(str::to_owned),
                new: item["newText"].as_str().unwrap_or_default().to_owned(),
            }),
            "content" => {
                let text = block_text(&item["content"]);
                (!text.is_empty()).then_some(ToolContent::Text(text))
            }
            _ => None,
        })
        .collect()
}

fn parse_locations(items: &Value) -> Vec<(PathBuf, Option<u32>)> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some((
                PathBuf::from(item["path"].as_str()?),
                item["line"].as_u64().map(|line| line as u32),
            ))
        })
        .collect()
}

impl Thread {
    /// Adds what the user sent.
    pub fn push_user(&mut self, text: impl Into<String>) {
        self.entries.push(Entry::User(text.into()));
    }

    /// Appends a streamed piece to the last block of the same kind, or
    /// starts one.
    fn append(&mut self, kind: fn(String) -> Entry, text: String) {
        let same = matches!(
            (self.entries.last(), kind(String::new())),
            (Some(Entry::Agent(_)), Entry::Agent(_))
                | (Some(Entry::Thought(_)), Entry::Thought(_))
                | (Some(Entry::User(_)), Entry::User(_))
        );
        if same {
            if let Some(Entry::Agent(existing) | Entry::Thought(existing) | Entry::User(existing)) =
                self.entries.last_mut()
            {
                existing.push_str(&text);
            }
        } else {
            self.entries.push(kind(text));
        }
    }

    pub fn tool(&self, id: &str) -> Option<&ToolCall> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }

    fn tool_mut(&mut self, id: &str) -> Option<&mut ToolCall> {
        self.entries.iter_mut().find_map(|entry| match entry {
            Entry::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }

    /// Applies the `update` of a `session/update` notification.
    pub fn apply(&mut self, update: &Value) {
        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                self.append(Entry::Agent, block_text(&update["content"]));
                if let Some(id) = update["messageId"].as_str().filter(|id| !id.is_empty()) {
                    self.message_ids
                        .insert(self.entries.len() - 1, id.to_owned());
                }
            }
            "agent_thought_chunk" => self.append(Entry::Thought, block_text(&update["content"])),
            // Sent when a past conversation is replayed.
            "user_message_chunk" => self.append(Entry::User, block_text(&update["content"])),
            "tool_call" => {
                let id = update["toolCallId"].as_str().unwrap_or_default().to_owned();
                let call = ToolCall {
                    title: update["title"].as_str().unwrap_or_default().to_owned(),
                    kind: update["kind"].as_str().unwrap_or("other").to_owned(),
                    status: ToolStatus::parse(&update["status"]).unwrap_or(ToolStatus::Pending),
                    content: parse_content(&update["content"]),
                    locations: parse_locations(&update["locations"]),
                    id: id.clone(),
                };
                // Sent again for a call already shown: it replaces it.
                match self.tool_mut(&id) {
                    Some(existing) => *existing = call,
                    None => self.entries.push(Entry::Tool(call)),
                }
            }
            "tool_call_update" => {
                let id = update["toolCallId"].as_str().unwrap_or_default().to_owned();
                if self.tool(&id).is_none() {
                    self.entries.push(Entry::Tool(ToolCall {
                        id: id.clone(),
                        title: String::new(),
                        kind: "other".to_owned(),
                        status: ToolStatus::Pending,
                        content: Vec::new(),
                        locations: Vec::new(),
                    }));
                }
                let Some(tool) = self.tool_mut(&id) else {
                    return;
                };
                if let Some(title) = update["title"].as_str() {
                    tool.title = title.to_owned();
                }
                if let Some(kind) = update["kind"].as_str() {
                    tool.kind = kind.to_owned();
                }
                if let Some(status) = ToolStatus::parse(&update["status"]) {
                    tool.status = status;
                }
                if update["content"].is_array() {
                    tool.content = parse_content(&update["content"]);
                }
                if update["locations"].is_array() {
                    tool.locations = parse_locations(&update["locations"]);
                }
            }
            "plan" => {
                self.plan = update["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|entry| PlanItem {
                        content: entry["content"].as_str().unwrap_or_default().to_owned(),
                        status: entry["status"].as_str().unwrap_or("pending").to_owned(),
                    })
                    .collect();
            }
            "available_commands_update" => {
                self.commands = update["availableCommands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|command| {
                        Some((
                            command["name"].as_str()?.to_owned(),
                            command["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                        ))
                    })
                    .collect();
            }
            "current_mode_update" => {
                self.mode = update["currentModeId"].as_str().map(str::to_owned);
            }
            "session_info_update" => {
                if let Some(title) = update["title"].as_str() {
                    self.title = Some(title.to_owned());
                }
            }
            _ => {}
        }
    }

    /// The files the agent's tool calls change or changed, in order.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in &self.entries {
            let Entry::Tool(tool) = entry else { continue };
            if tool.status == ToolStatus::Failed {
                continue;
            }
            for content in &tool.content {
                if let ToolContent::Diff { path, .. } = content
                    && !files.contains(path)
                {
                    files.push(path.clone());
                }
            }
        }
        files
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn answers_keep_the_id_of_their_last_message() {
        let mut thread = Thread::default();
        thread.push_user("Ciao");
        thread.apply(&json!({ "sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": { "type": "text", "text": "Ci" } }));
        thread.apply(&json!({ "sessionUpdate": "agent_message_chunk", "messageId": "m2", "content": { "type": "text", "text": "ao" } }));
        thread.push_user("Poi");
        thread.apply(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Ok" } }));
        assert_eq!(thread.message_ids.get(&1).map(String::as_str), Some("m2"));
        assert_eq!(thread.message_ids.get(&3), None);
    }

    use serde_json::json;

    use super::*;

    #[test]
    fn updates_build_the_conversation() {
        let mut thread = Thread::default();
        thread.push_user("Aggiungi il prezzo");
        for update in [
            json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "Guardo " } }),
            json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "il file." } }),
            json!({ "sessionUpdate": "plan", "entries": [
                { "content": "Leggere booking.ts", "priority": "high", "status": "completed" },
                { "content": "Cambiare il prezzo", "priority": "high", "status": "in_progress" } ] }),
            json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit booking.ts", "kind": "edit",
                    "status": "pending", "locations": [{ "path": "/p/booking.ts", "line": 2 }],
                    "content": [{ "type": "diff", "path": "/p/booking.ts", "oldText": "90", "newText": "95" }] }),
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed" }),
            json!({ "sessionUpdate": "tool_call", "toolCallId": "t2", "title": "Write notes.md", "kind": "edit", "status": "completed",
                    "content": [{ "type": "diff", "path": "/p/notes.md", "newText": "# Note" }] }),
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Fatto: " } }),
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "**95**." } }),
            json!({ "sessionUpdate": "available_commands_update", "availableCommands": [{ "name": "review", "description": "Review" }] }),
            json!({ "sessionUpdate": "current_mode_update", "currentModeId": "plan" }),
            json!({ "sessionUpdate": "session_info_update", "title": "Prezzo" }),
            json!({ "sessionUpdate": "something_new" }),
        ] {
            thread.apply(&update);
        }
        assert_eq!(thread.entries.len(), 5);
        assert_eq!(thread.entries[0], Entry::User("Aggiungi il prezzo".into()));
        assert_eq!(thread.entries[1], Entry::Thought("Guardo il file.".into()));
        let Entry::Tool(tool) = &thread.entries[2] else {
            panic!()
        };
        assert_eq!(
            (tool.kind.as_str(), &tool.status),
            ("edit", &ToolStatus::Completed)
        );
        assert_eq!(tool.locations, [(PathBuf::from("/p/booking.ts"), Some(2))]);
        assert_eq!(thread.entries[4], Entry::Agent("Fatto: **95**.".into()));
        assert_eq!(thread.plan[1].status, "in_progress");
        assert_eq!(
            thread.commands,
            [("review".to_owned(), "Review".to_owned())]
        );
        assert_eq!(thread.mode.as_deref(), Some("plan"));
        assert_eq!(thread.title.as_deref(), Some("Prezzo"));
        assert_eq!(
            thread.changed_files(),
            [PathBuf::from("/p/booking.ts"), PathBuf::from("/p/notes.md")]
        );
    }

    #[test]
    fn a_replayed_conversation_has_the_users_messages() {
        let mut thread = Thread::default();
        thread.apply(&json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": "Ciao" } }));
        thread.apply(&json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "resource_link", "name": "a.ts", "uri": "file:///a.ts" } }));
        thread.apply(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Ciao!" } }));
        assert_eq!(
            thread.entries,
            [
                Entry::User("Ciao@a.ts".into()),
                Entry::Agent("Ciao!".into())
            ]
        );
    }
}

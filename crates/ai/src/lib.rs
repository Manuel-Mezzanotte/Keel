//! Claude Code in Keel.
//!
//! - [`ide`]: the connection Claude Code makes to VS Code, Cursor and
//!   JetBrains, so Claude Code in a terminal of Keel sees the open files,
//!   the selection and the problems, and shows its edits in Keel.

//! - [`acp`]: the Agent Client Protocol, to talk to Claude Code as an agent
//!   (through `claude-agent-acp`) in the agent panel; [`thread`] builds the
//!   conversation from what it says.

pub mod acp;
pub mod fake_agent;
pub mod history;
pub mod ide;
pub mod session_meta;
pub mod thread;

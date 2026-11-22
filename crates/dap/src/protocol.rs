//! The parts of the protocol's messages Keel reads.

use serde::{Deserialize, Serialize};

/// What an adapter can do.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_conditional_breakpoints: bool,
    pub supports_hit_conditional_breakpoints: bool,
    pub supports_log_points: bool,
    pub supports_set_variable: bool,
    pub supports_restart_request: bool,
    pub supports_terminate_request: bool,
    pub support_terminate_debuggee: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_completions_request: bool,
    pub supports_exception_info_request: bool,
    pub exception_breakpoint_filters: Vec<ExceptionFilter>,
}

/// A kind of exception an adapter can stop at.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExceptionFilter {
    pub filter: String,
    pub label: String,
    pub default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Source {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A source the adapter has and the disk has not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    pub source: Option<Source>,
    /// From 1, as Keel says it starts.
    pub line: u32,
    pub column: u32,
    /// `label` for a line that is no frame, `subtle` for one aside.
    pub presentation_hint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scope {
    pub name: String,
    pub presentation_hint: Option<String>,
    pub variables_reference: i64,
    /// Not read until it is opened.
    pub expensive: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub evaluate_name: Option<String>,
    /// What it holds, to read when it is opened; 0 for nothing.
    pub variables_reference: i64,
}

/// A breakpoint as the adapter took it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Breakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub source: Option<Source>,
    pub line: Option<u32>,
}

/// The result of an expression.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Evaluated {
    pub result: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub variables_reference: i64,
}

/// A breakpoint as it is asked.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

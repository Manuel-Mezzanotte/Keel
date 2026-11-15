//! VS Code extensions in Keel: VSIX packages, the V8 extension host and the
//! `vscode` bridge that extensions talk to.

mod host;
mod manifest;
mod vsix;

pub use host::{ExtensionHost, HostEvent};
pub use manifest::{CommandContribution, KeybindingContribution, Manifest};
pub use vsix::{InstalledExtension, Vsix};

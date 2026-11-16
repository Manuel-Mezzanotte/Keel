//! VS Code extensions in Keel: VSIX packages, the V8 extension host and the
//! `vscode` bridge that extensions talk to.

pub mod color_theme;
mod host;
pub mod icon_theme;
pub mod languages;
mod manifest;
pub mod marketplace;
mod node;
mod vsix;

pub use host::{ExtensionHost, HostEvent};
pub use manifest::{CommandContribution, KeybindingContribution, Manifest};
pub use vsix::{InstalledExtension, Vsix};

mod auth;
mod bridge;
mod components;
mod daemon_config;
mod dashboard;
mod directory;
mod network;
mod recovery;
mod server;
mod server_network;
mod server_support;
mod ssh_tunnel;
mod stream;

use std::path::PathBuf;

pub use server::run;

/// Command used to launch the app-server that backs the browser UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerCommand {
    /// Re-execute the standalone web binary in its hidden app-server mode.
    Standalone { executable: PathBuf },
    /// Re-execute the full Codex CLI using its public `app-server` subcommand.
    CodexCli { executable: PathBuf },
}

/// Settings supplied by the `codex web` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebOptions {
    pub port: u16,
    pub open_browser: bool,
    pub cwd: Option<PathBuf>,
    pub app_server_command: AppServerCommand,
    pub config_overrides: Vec<String>,
    pub strict_config: bool,
    pub reset_token: bool,
    pub daemon_config: Option<PathBuf>,
}

use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use codex_arg0::Arg0DispatchPaths;
use codex_arg0::arg0_dispatch_or_else;
use codex_config::LoaderOverrides;
use codex_utils_cli::CliConfigOverrides;
use codex_web::AppServerCommand;
use codex_web::WebOptions;

/// Run Codex sessions in a local browser interface.
#[derive(Debug, Parser)]
#[command(name = "typeduck-codex-web", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// TCP port to listen on. Uses an available port when omitted.
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// Do not open the session in the default browser.
    #[arg(long)]
    no_open: bool,

    /// Tell the agent to use the specified directory as its working root.
    #[arg(long = "cd", short = 'C', value_name = "DIR")]
    cwd: Option<PathBuf>,

    /// Override a configuration value that would otherwise be loaded from config.toml.
    #[arg(short = 'c', value_name = "KEY=VALUE")]
    config_overrides: Vec<String>,

    /// Error out when config.toml contains unknown fields.
    #[arg(long, default_value_t = false)]
    strict_config: bool,

    /// Replace the persistent browser bootstrap token.
    #[arg(long, default_value_t = false)]
    reset_token: bool,

    /// Load ordered accounts and proxy settings from this daemon config.
    #[arg(long, value_name = "PATH")]
    daemon_config: Option<PathBuf>,

    /// Use an external Codex executable instead of the embedded app server.
    #[arg(long, value_name = "PATH", hide = true)]
    codex_bin: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Internal stdio app-server entrypoint used by the standalone binary.
    #[command(name = "__app-server", hide = true)]
    AppServer(AppServerArgs),
}

#[derive(Debug, Parser)]
struct AppServerArgs {
    /// Override a configuration value that would otherwise be loaded from config.toml.
    #[arg(short = 'c', value_name = "KEY=VALUE")]
    config_overrides: Vec<String>,

    /// Error out when config.toml contains unknown fields.
    #[arg(long, default_value_t = false)]
    strict_config: bool,

    /// Read OPENAI_API_KEY for a private fallback runtime.
    #[arg(long, default_value_t = false, hide = true)]
    enable_openai_api_key_env: bool,
}

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(|arg0_paths| async move { main_with_arg0(arg0_paths).await })
}

async fn main_with_arg0(arg0_paths: Arg0DispatchPaths) -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::AppServer(args)) => {
            codex_app_server::run_main_with_runtime_options(
                arg0_paths,
                CliConfigOverrides {
                    raw_overrides: args.config_overrides,
                },
                LoaderOverrides::default(),
                args.strict_config,
                /*default_analytics_enabled*/ false,
                codex_app_server::AppServerRuntimeOptions {
                    enable_codex_api_key_env: args.enable_openai_api_key_env,
                    ..Default::default()
                },
            )
            .await?;
        }
        None => {
            let app_server_command = match cli.codex_bin {
                Some(executable) => AppServerCommand::CodexCli { executable },
                None => AppServerCommand::Standalone {
                    executable: std::env::current_exe()?,
                },
            };
            codex_web::run(WebOptions {
                port: cli.port,
                open_browser: !cli.no_open,
                cwd: cli.cwd,
                app_server_command,
                config_overrides: cli.config_overrides,
                strict_config: cli.strict_config,
                reset_token: cli.reset_token,
                daemon_config: cli.daemon_config,
            })
            .await?;
        }
    }
    Ok(())
}

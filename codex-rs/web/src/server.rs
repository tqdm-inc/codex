use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::RwLock;

use http::HeaderValue;
use http::header::CACHE_CONTROL;
use http::header::CONTENT_SECURITY_POLICY;
use http::header::CONTENT_TYPE;
use http::header::COOKIE;
use http::header::HOST;
use http::header::ORIGIN;
use http::header::SET_COOKIE;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use topcoat::Result;
use topcoat::context::Cx;
use topcoat::context::app_context;
use topcoat::router::Body;
use topcoat::router::IntoResponse;
use topcoat::router::Response;
use topcoat::router::Router;
use topcoat::router::RouterBuilderDiscoverExt;
use topcoat::router::forbidden;
use topcoat::router::headers;
use topcoat::router::not_found;
use topcoat::router::page;
use topcoat::router::path_param;
use topcoat::router::route;
use topcoat::router::to_bytes;
use topcoat::router::unauthorized;
use topcoat::router::uri;

use crate::WebOptions;
use crate::auth::WebAuth;
use crate::bridge::AppServerBridge;
use crate::components::DashboardDocumentData;
use crate::components::DocumentData;
use crate::components::bootstrap_document;
use crate::components::dashboard_document;
use crate::components::document;
use crate::components::thread_title;
use crate::components::transcript_fragment;
use crate::daemon_config::AccountProfile;
use crate::daemon_config::DaemonConfig;
use crate::dashboard;
use crate::directory;
use crate::directory::DirectoryListRequest;
use crate::network::authority;
use crate::network::bind_listeners;
use crate::recovery::RecoveryManager;
use crate::server_support::canonicalize_cwd;
use crate::server_support::chronological_turns;
use crate::server_support::generate_secret;
use crate::server_support::mcp_callback_url;
use crate::server_support::query_value;
use crate::server_support::thread_with_initial_turns;
use crate::ssh_tunnel::ManagedSshTunnel;
use crate::stream;

const MAX_RPC_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_AUTH_BODY_BYTES: usize = 4 * 1024;
const APP_JS: &str = include_str!("../assets/app.js");
const APP_CSS: &str = include_str!("../assets/app.css");
const BOOTSTRAP_JS: &str = include_str!("../assets/bootstrap.js");
const HOME_JS: &str = include_str!("../assets/home.js");
const SETTINGS_JS: &str = include_str!("../assets/settings.js");

pub(crate) struct WebState {
    pub(crate) auth: WebAuth,
    allowed_authorities: HashSet<String>,
    instance_id: String,
    cwd: String,
    pub(crate) runtime: RwLock<ActiveRuntime>,
    pub(crate) runtime_launch: RuntimeLaunch,
    pub(crate) ssh_tunnel: AsyncMutex<Option<ManagedSshTunnel>>,
    runtime_switch: Arc<Semaphore>,
    auth_bridges: Arc<AsyncMutex<HashMap<String, Arc<AppServerBridge>>>>,
    pub(crate) events: Arc<stream::LiveEventHub>,
    shutdown: CancellationToken,
    mcp_callback_port: Option<u16>,
    pub(crate) daemon_config: RwLock<DaemonConfig>,
    pub(crate) recovery: Arc<RecoveryManager>,
}

#[derive(Clone)]
pub(crate) struct ActiveRuntime {
    pub(crate) bridge: Arc<AppServerBridge>,
    pub(crate) account_name: String,
    pub(crate) account_label: String,
    pub(crate) proxy_active: bool,
}

#[derive(Clone)]
pub(crate) struct RuntimeLaunch {
    pub(crate) command: crate::AppServerCommand,
    pub(crate) cwd: std::path::PathBuf,
    pub(crate) codex_home: std::path::PathBuf,
    pub(crate) ssh_proxy: Arc<RwLock<Option<String>>>,
    pub(crate) config_overrides: Vec<String>,
    pub(crate) strict_config: bool,
}

#[topcoat::router::path_param]
struct ThreadId(str);

#[topcoat::router::path_param]
struct InstanceId(str);

#[topcoat::router::path_param]
struct CallbackId(str);

#[topcoat::router::path_param]
struct SessionId(str);

pub async fn run(options: WebOptions) -> std::io::Result<()> {
    let explicit_cwd = options.cwd.is_some();
    let cwd = options
        .cwd
        .map(canonicalize_cwd)
        .transpose()?
        .unwrap_or(std::env::current_dir()?.canonicalize()?);
    let auth = WebAuth::load(options.reset_token)?;
    let daemon_config = DaemonConfig::load(options.daemon_config.as_deref())?;
    let recovery = RecoveryManager::load(daemon_config.recovery_state_path());
    let ssh_tunnel = match daemon_config.ssh_tunnel.as_ref() {
        Some(config) => Some(ManagedSshTunnel::start(config).await?),
        None => None,
    };
    let requested_port = if options.port == 0 {
        daemon_config.port.unwrap_or(0)
    } else {
        options.port
    };
    let (listeners, _port) = bind_listeners(requested_port).await?;
    let addresses = listeners
        .iter()
        .filter_map(|listener| listener.local_addr().ok())
        .collect::<Vec<_>>();
    let mut config_overrides = options.config_overrides;
    let callback_url = daemon_config
        .mcp_oauth_callback_url
        .clone()
        .or_else(|| mcp_callback_url(&addresses));
    let mcp_callback_port = if let Some(callback_url) = callback_url {
        let callback_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let callback_port = callback_listener.local_addr()?.port();
        drop(callback_listener);
        config_overrides.push(format!("mcp_oauth_callback_port={callback_port}"));
        config_overrides.push(format!("mcp_oauth_callback_url={callback_url:?}"));
        Some(callback_port)
    } else {
        None
    };
    let runtime_launch = RuntimeLaunch {
        command: options.app_server_command.clone(),
        cwd: cwd.clone(),
        codex_home: daemon_config.codex_home.clone(),
        ssh_proxy: Arc::new(RwLock::new(
            ssh_tunnel
                .as_ref()
                .map(ManagedSshTunnel::proxy_url)
                .map(ToOwned::to_owned),
        )),
        config_overrides,
        strict_config: options.strict_config,
    };
    let preferred_account = recovery.preferred_account();
    let (account_profile, bridge, proxy_active) = start_account_bridge(
        &daemon_config.accounts,
        &runtime_launch,
        preferred_account.as_deref(),
    )
    .await?;
    let shutdown_token = CancellationToken::new();
    let event_hub = stream::LiveEventHub::start(Arc::clone(&bridge)).await;
    recovery.attach(Arc::clone(&bridge), account_profile.name.clone());
    let mut allowed_authorities = addresses
        .iter()
        .map(|address| authority(*address))
        .collect::<HashSet<_>>();
    if let Some(port) = addresses.first().map(std::net::SocketAddr::port) {
        allowed_authorities.insert(format!("localhost:{port}"));
    }
    let state = Arc::new(WebState {
        auth,
        allowed_authorities,
        instance_id: generate_secret()[..16].to_string(),
        cwd: directory::path_string(&cwd)?,
        runtime: RwLock::new(ActiveRuntime {
            bridge: Arc::clone(&bridge),
            account_name: account_profile.name.clone(),
            account_label: account_profile.label.clone(),
            proxy_active,
        }),
        runtime_launch,
        ssh_tunnel: AsyncMutex::new(ssh_tunnel),
        runtime_switch: Arc::new(Semaphore::new(1)),
        auth_bridges: Arc::new(AsyncMutex::new(HashMap::new())),
        events: event_hub,
        shutdown: shutdown_token.clone(),
        mcp_callback_port,
        daemon_config: RwLock::new(daemon_config),
        recovery: Arc::clone(&recovery),
    });
    let recovery_state = Arc::clone(&state);
    tokio::spawn(async move {
        let runtime = match active_runtime(&recovery_state) {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::warn!(%error, "could not access runtime for session recovery");
                return;
            }
        };
        crate::recovery::restore_sessions(
            &recovery_state.recovery,
            &runtime.bridge,
            &runtime.account_name,
        )
        .await;
    });
    let router = Router::builder()
        .discover()
        .app_context(Arc::clone(&state))
        .build();
    let service = topcoat::router::RouterService::new(router);
    let mut servers = JoinSet::new();
    for listener in listeners {
        let service = service.clone();
        let shutdown_token = shutdown_token.clone();
        servers.spawn(async move {
            topcoat::serve_until(listener, service, async move {
                shutdown_token.cancelled().await;
            })
            .await
        });
    }

    let startup_path = if explicit_cwd {
        let return_to = format!("/new?cwd={}", urlencoding::encode(&state.cwd));
        format!("/?returnTo={}", urlencoding::encode(&return_to))
    } else {
        "/".to_string()
    };
    let urls = addresses
        .iter()
        .map(|address| {
            format!(
                "http://{}{}#bootstrap={}",
                authority(*address),
                startup_path,
                state.auth.bootstrap_token()
            )
        })
        .collect::<Vec<_>>();
    println!("\nCodex Web\n");
    for url in &urls {
        println!("  {url}");
    }
    println!("\nThis private link grants the server user's Codex and filesystem access.\n");
    println!("Account: {}", account_profile.label);
    println!(
        "OpenAI proxy: {}\n",
        if proxy_active { "enabled" } else { "disabled" }
    );

    if options.open_browser
        && let Some(url) = urls.iter().find(|url| url.starts_with("http://127.0.0.1:"))
        && let Err(error) = webbrowser::open(url)
    {
        tracing::warn!(%error, "failed to open Codex Web in a browser");
    }

    tokio::select! {
        signal = tokio::signal::ctrl_c() => signal?,
        result = servers.join_next() => {
            match result {
                Some(Ok(Ok(()))) | None => {}
                Some(Ok(Err(error))) => return Err(error),
                Some(Err(error)) => return Err(Error::other(error)),
            }
        }
    }
    shutdown_token.cancel();
    // Event streams are intentionally long-lived, so a graceful listener drain can wait forever
    // for browser connections after Ctrl-C. Stop accepting work, then cancel those connection
    // tasks so the CLI returns promptly.
    servers.abort_all();
    while let Some(result) = servers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "Codex Web listener stopped"),
            Err(error) if error.is_cancelled() => {}
            Err(error) => tracing::warn!(%error, "Codex Web listener task failed"),
        }
    }
    active_runtime(&state)
        .map_err(|error| Error::other(error.to_string()))?
        .bridge
        .shutdown()
        .await;
    let mut ssh_tunnel = state.ssh_tunnel.lock().await.take();
    if let Some(tunnel) = ssh_tunnel.as_mut() {
        tunnel.shutdown().await;
    }
    Ok(())
}

async fn start_account_bridge(
    accounts: &[AccountProfile],
    launch: &RuntimeLaunch,
    preferred_account: Option<&str>,
) -> std::io::Result<(AccountProfile, Arc<AppServerBridge>, bool)> {
    let mut failures = Vec::new();
    let mut candidates = Vec::with_capacity(accounts.len());
    if let Some(preferred_account) = preferred_account
        && let Some(profile) = accounts
            .iter()
            .find(|profile| profile.name == preferred_account)
    {
        candidates.push(profile);
    }
    candidates.extend(
        accounts
            .iter()
            .filter(|profile| Some(profile.name.as_str()) != preferred_account),
    );
    for profile in candidates {
        let (bridge, proxy_active) = match start_profile_bridge(profile, launch).await {
            Ok(runtime) => runtime,
            Err(error) => {
                failures.push(format!("{}: {error}", profile.name));
                continue;
            }
        };
        let account = bridge
            .request("account/read", json!({ "refreshToken": false }))
            .await;
        let available = account.as_ref().is_ok_and(|result| {
            result
                .get("account")
                .is_some_and(|account| !account.is_null())
                || result.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(false)
        });
        if available && !account_rate_limited(&bridge).await {
            return Ok((profile.clone(), bridge, proxy_active));
        }
        failures.push(format!(
            "{}: {}",
            profile.name,
            if available {
                "rate limit exhausted"
            } else {
                "not signed in"
            }
        ));
        bridge.shutdown().await;
    }

    let profile = accounts
        .first()
        .ok_or_else(|| Error::new(std::io::ErrorKind::InvalidInput, "no accounts configured"))?;
    tracing::warn!(attempts = %failures.join("; "), "all configured accounts need attention; using the first account for login");
    let (bridge, proxy_active) = start_profile_bridge(profile, launch).await?;
    Ok((profile.clone(), bridge, proxy_active))
}

pub(crate) async fn start_profile_bridge(
    profile: &AccountProfile,
    launch: &RuntimeLaunch,
) -> std::io::Result<(Arc<AppServerBridge>, bool)> {
    std::fs::create_dir_all(&launch.codex_home)?;
    let credentials = account_credentials(profile)?;
    if matches!(credentials, AccountCredentials::Missing) {
        return Err(Error::new(
            std::io::ErrorKind::PermissionDenied,
            "account is not signed in",
        ));
    }
    if matches!(credentials, AccountCredentials::ApiKey(_))
        && matches!(launch.command, crate::AppServerCommand::CodexCli { .. })
    {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "API-key fallbacks require the embedded app server",
        ));
    }
    let ssh_proxy = launch
        .ssh_proxy
        .read()
        .map_err(|_| Error::other("SSH proxy lock is poisoned"))?
        .clone();
    let proxy = if profile.use_ssh_tunnel {
        ssh_proxy.as_deref()
    } else {
        profile.proxy.as_deref()
    };
    let mut overrides = launch.config_overrides.clone();
    if proxy.is_some() {
        overrides.push("features.respect_system_proxy=true".to_string());
    }
    let bridge = AppServerBridge::start(
        &launch.command,
        &launch.cwd,
        &launch.codex_home,
        proxy,
        credentials.api_key(),
        &overrides,
        launch.strict_config,
    )
    .await?;
    if let Some(params) = credentials.chatgpt_login_params()
        && let Err(error) = bridge.request("account/login/start", params).await
    {
        bridge.shutdown().await;
        return Err(error);
    }
    if matches!(credentials, AccountCredentials::Chatgpt { .. }) {
        attach_account_refresh(Arc::clone(&bridge), profile.clone(), launch.clone());
    }
    Ok((bridge, proxy.is_some()))
}

fn attach_account_refresh(
    bridge: Arc<AppServerBridge>,
    profile: AccountProfile,
    launch: RuntimeLaunch,
) {
    tokio::spawn(async move {
        let mut events = bridge.subscribe();
        while let Ok(event) = events.recv().await {
            if event.get("method").and_then(Value::as_str)
                != Some("account/chatgptAuthTokens/refresh")
            {
                continue;
            }
            let Some(id) = event.get("id").cloned() else {
                continue;
            };
            let result = refresh_fallback_account(&profile, &launch).await;
            let response = match result {
                Ok(credentials) => json!({ "jsonrpc": "2.0", "id": id, "result": credentials }),
                Err(error) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32000, "message": error.to_string() }
                }),
            };
            if let Err(error) = bridge.respond(response).await {
                tracing::warn!(%error, account = profile.name, "failed to return refreshed account credentials");
            }
        }
    });
}

async fn refresh_fallback_account(
    profile: &AccountProfile,
    launch: &RuntimeLaunch,
) -> std::io::Result<Value> {
    let auth_bridge = start_profile_auth_bridge(profile, launch).await?;
    let refresh = auth_bridge
        .request("account/read", json!({ "refreshToken": true }))
        .await;
    auth_bridge.shutdown().await;
    refresh?;
    match account_credentials(profile)? {
        AccountCredentials::Chatgpt {
            access_token,
            account_id,
            plan_type,
        } => Ok(json!({
            "accessToken": access_token,
            "chatgptAccountId": account_id,
            "chatgptPlanType": plan_type,
        })),
        AccountCredentials::Primary
        | AccountCredentials::ApiKey(_)
        | AccountCredentials::Missing => Err(Error::new(
            ErrorKind::PermissionDenied,
            "ChatGPT credentials are unavailable",
        )),
    }
}

enum AccountCredentials {
    Primary,
    ApiKey(String),
    Chatgpt {
        access_token: String,
        account_id: String,
        plan_type: Option<String>,
    },
    Missing,
}

impl AccountCredentials {
    fn api_key(&self) -> Option<&str> {
        match self {
            Self::ApiKey(api_key) => Some(api_key),
            Self::Primary | Self::Chatgpt { .. } | Self::Missing => None,
        }
    }

    fn chatgpt_login_params(&self) -> Option<Value> {
        match self {
            Self::Chatgpt {
                access_token,
                account_id,
                plan_type,
            } => Some(json!({
                "type": "chatgptAuthTokens",
                "accessToken": access_token,
                "chatgptAccountId": account_id,
                "chatgptPlanType": plan_type,
            })),
            Self::Primary | Self::ApiKey(_) | Self::Missing => None,
        }
    }
}

fn account_credentials(profile: &AccountProfile) -> std::io::Result<AccountCredentials> {
    use crate::daemon_config::AccountRole;
    use codex_config::types::AuthCredentialsStoreMode;
    use codex_config::types::AuthKeyringBackendKind;

    if profile.role == AccountRole::Primary {
        return Ok(AccountCredentials::Primary);
    }
    let Some(auth) = codex_login::load_auth_dot_json(
        &profile.credential_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?
    else {
        return Ok(AccountCredentials::Missing);
    };
    if let Some(api_key) = auth.openai_api_key {
        return Ok(AccountCredentials::ApiKey(api_key));
    }
    let Some(tokens) = auth.tokens else {
        return Ok(AccountCredentials::Missing);
    };
    let Some(account_id) = tokens
        .account_id
        .or(tokens.id_token.chatgpt_account_id.clone())
    else {
        return Ok(AccountCredentials::Missing);
    };
    Ok(AccountCredentials::Chatgpt {
        access_token: tokens.access_token,
        account_id,
        plan_type: tokens.id_token.get_chatgpt_plan_type_raw(),
    })
}

async fn switch_account(state: &WebState, name: &str) -> std::io::Result<()> {
    let _switch = Arc::clone(&state.runtime_switch)
        .acquire_owned()
        .await
        .map_err(|_| Error::other("account switch gate is closed"))?;
    let current = active_runtime(state).map_err(|error| Error::other(error.to_string()))?;
    if has_active_turn(&current.bridge).await {
        return Err(Error::new(
            std::io::ErrorKind::WouldBlock,
            "finish or stop the active turn before switching accounts",
        ));
    }
    let profile = state
        .daemon_config
        .read()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .accounts
        .iter()
        .find(|account| account.name == name)
        .cloned()
        .ok_or_else(|| Error::new(std::io::ErrorKind::NotFound, "account profile not found"))?;
    let (bridge, proxy_active) = start_profile_bridge(&profile, &state.runtime_launch).await?;
    let account = bridge
        .request("account/read", json!({ "refreshToken": false }))
        .await?;
    if account.get("account").is_none_or(Value::is_null)
        && account.get("requiresOpenaiAuth").and_then(Value::as_bool) != Some(false)
    {
        bridge.shutdown().await;
        return Err(Error::new(
            std::io::ErrorKind::PermissionDenied,
            "account is not signed in",
        ));
    }
    if account_rate_limited(&bridge).await {
        bridge.shutdown().await;
        return Err(Error::new(
            std::io::ErrorKind::PermissionDenied,
            "account rate limit is exhausted",
        ));
    }
    state.events.attach(Arc::clone(&bridge)).await;
    state
        .recovery
        .attach(Arc::clone(&bridge), profile.name.clone());
    {
        let mut runtime = state
            .runtime
            .write()
            .map_err(|_| Error::other("active runtime lock is poisoned"))?;
        *runtime = ActiveRuntime {
            bridge: Arc::clone(&bridge),
            account_name: profile.name.clone(),
            account_label: profile.label,
            proxy_active,
        };
    }
    state.events.runtime_changed(&profile.name).await;
    current.bridge.shutdown().await;
    Ok(())
}

pub(crate) async fn has_active_turn(bridge: &Arc<AppServerBridge>) -> bool {
    let mut cursor = Value::Null;
    loop {
        let Ok(loaded) = bridge
            .request(
                "thread/loaded/list",
                json!({ "limit": 100, "cursor": cursor }),
            )
            .await
        else {
            return true;
        };
        let Some(thread_ids) = loaded.get("data").and_then(Value::as_array) else {
            return true;
        };
        for thread_id in thread_ids.iter().filter_map(Value::as_str) {
            let Ok(thread) = bridge
                .request(
                    "thread/read",
                    json!({ "threadId": thread_id, "includeTurns": false }),
                )
                .await
            else {
                return true;
            };
            if thread_is_active(&thread) {
                return true;
            }
        }
        cursor = loaded.get("nextCursor").cloned().unwrap_or(Value::Null);
        if cursor.is_null() {
            return false;
        }
    }
}

async fn ensure_healthy_account(state: &WebState) -> std::io::Result<()> {
    let current = active_runtime(state).map_err(|error| Error::other(error.to_string()))?;
    let account = current
        .bridge
        .request("account/read", json!({ "refreshToken": false }))
        .await;
    let available = account.as_ref().is_ok_and(|result| {
        result
            .get("account")
            .is_some_and(|account| !account.is_null())
            || result.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(false)
    });
    if available && !account_rate_limited(&current.bridge).await {
        return Ok(());
    }
    let accounts = state
        .daemon_config
        .read()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .accounts
        .clone();
    let mut failures = Vec::new();
    for profile in accounts
        .iter()
        .filter(|profile| profile.name != current.account_name)
    {
        match switch_account(state, &profile.name).await {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(format!("{}: {error}", profile.name)),
        }
    }
    Err(Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!(
            "no healthy Codex account is available{}",
            if failures.is_empty() {
                String::new()
            } else {
                format!(": {}", failures.join("; "))
            }
        ),
    ))
}

fn thread_is_active(thread: &Value) -> bool {
    thread
        .pointer("/thread/status/type")
        .or_else(|| thread.pointer("/status/type"))
        .or_else(|| thread.get("status"))
        .and_then(Value::as_str)
        == Some("active")
}

async fn manage_account_auth_request(
    state: &WebState,
    request: AccountAuthRequest,
) -> std::io::Result<Value> {
    use crate::daemon_config::AccountRole;
    use codex_config::types::AuthCredentialsStoreMode;
    use codex_config::types::AuthKeyringBackendKind;

    let name = match &request {
        AccountAuthRequest::ChatgptDeviceCode { name }
        | AccountAuthRequest::ApiKey { name, .. }
        | AccountAuthRequest::Logout { name }
        | AccountAuthRequest::Status { name } => name,
    };
    let profile = state
        .daemon_config
        .read()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .accounts
        .iter()
        .find(|account| account.name == *name)
        .cloned()
        .ok_or_else(|| Error::new(std::io::ErrorKind::NotFound, "account profile not found"))?;
    match request {
        AccountAuthRequest::ChatgptDeviceCode { .. } => {
            let bridge = start_auth_bridge(state, &profile).await?;
            let result = bridge
                .request(
                    "account/login/start",
                    json!({ "type": "chatgptDeviceCode" }),
                )
                .await?;
            let previous = state.auth_bridges.lock().await.insert(profile.name, bridge);
            if let Some(previous) = previous {
                previous.shutdown().await;
            }
            Ok(result)
        }
        AccountAuthRequest::ApiKey { api_key, .. } => {
            if profile.role == AccountRole::Primary {
                let bridge = start_auth_bridge(state, &profile).await?;
                let result = bridge
                    .request(
                        "account/login/start",
                        json!({ "type": "apiKey", "apiKey": api_key }),
                    )
                    .await?;
                bridge.shutdown().await;
                Ok(result)
            } else {
                crate::daemon_config::prepare_credential_home(&profile.credential_home)?;
                codex_login::login_with_api_key(
                    &profile.credential_home,
                    &api_key,
                    AuthCredentialsStoreMode::File,
                    AuthKeyringBackendKind::default(),
                )?;
                Ok(json!({ "saved": true }))
            }
        }
        AccountAuthRequest::Logout { .. } => {
            let pending_bridge = state.auth_bridges.lock().await.remove(&profile.name);
            if let Some(bridge) = pending_bridge {
                bridge.shutdown().await;
            }
            if profile.role == AccountRole::Primary {
                let bridge = start_auth_bridge(state, &profile).await?;
                bridge.request("account/logout", json!({})).await?;
                bridge.shutdown().await;
            } else {
                let auth_route_config = codex_login::AuthRouteConfig::from_http_client_factory(
                    codex_http_client::HttpClientFactory::new(
                        codex_http_client::OutboundProxyPolicy::RespectSystemProxy,
                    ),
                );
                codex_login::logout_with_revoke(
                    &profile.credential_home,
                    AuthCredentialsStoreMode::File,
                    AuthKeyringBackendKind::default(),
                    &auth_route_config,
                )
                .await?;
            }
            Ok(json!({ "signedOut": true }))
        }
        AccountAuthRequest::Status { .. } => {
            let pending_bridge = state.auth_bridges.lock().await.get(&profile.name).cloned();
            if let Some(bridge) = pending_bridge {
                return account_status(&bridge).await;
            }
            let active = active_runtime(state).map_err(|error| Error::other(error.to_string()))?;
            if active.account_name == profile.name {
                return account_status(&active.bridge).await;
            }
            let (bridge, _) = start_profile_bridge(&profile, &state.runtime_launch).await?;
            let result = account_status(&bridge).await;
            bridge.shutdown().await;
            result
        }
    }
}

async fn account_status(bridge: &Arc<AppServerBridge>) -> std::io::Result<Value> {
    let (account, rate_limits) = tokio::join!(
        bridge.request("account/read", json!({ "refreshToken": true })),
        bridge.request("account/rateLimits/read", json!({})),
    );
    let account = account?;
    Ok(json!({
        "account": account.get("account").cloned().unwrap_or(Value::Null),
        "requiresOpenaiAuth": account.get("requiresOpenaiAuth").cloned().unwrap_or(Value::Bool(true)),
        "rateLimits": rate_limits.ok().and_then(|value| value.get("rateLimits").cloned().or(Some(value))),
    }))
}

async fn start_auth_bridge(
    state: &WebState,
    profile: &AccountProfile,
) -> std::io::Result<Arc<AppServerBridge>> {
    start_profile_auth_bridge(profile, &state.runtime_launch).await
}

async fn start_profile_auth_bridge(
    profile: &AccountProfile,
    launch: &RuntimeLaunch,
) -> std::io::Result<Arc<AppServerBridge>> {
    use crate::daemon_config::AccountRole;

    let ssh_proxy = launch
        .ssh_proxy
        .read()
        .map_err(|_| Error::other("SSH proxy lock is poisoned"))?
        .clone();
    let proxy = if profile.use_ssh_tunnel {
        ssh_proxy.as_deref()
    } else {
        profile.proxy.as_deref()
    };
    let mut overrides = Vec::new();
    if profile.role == AccountRole::Fallback {
        overrides.push("cli_auth_credentials_store=\"file\"".to_string());
    }
    if proxy.is_some() {
        overrides.push("features.respect_system_proxy=true".to_string());
    }
    if profile.role == AccountRole::Fallback {
        crate::daemon_config::prepare_credential_home(&profile.credential_home)?;
    } else {
        std::fs::create_dir_all(&profile.credential_home)?;
    }
    AppServerBridge::start(
        &launch.command,
        &launch.cwd,
        &profile.credential_home,
        proxy,
        /*api_key*/ None,
        &overrides,
        launch.strict_config,
    )
    .await
}

async fn account_rate_limited(bridge: &Arc<AppServerBridge>) -> bool {
    let Ok(result) = bridge.request("account/rateLimits/read", json!({})).await else {
        return false;
    };
    let snapshot = result.get("rateLimits").unwrap_or(&result);
    snapshot
        .get("rateLimitReachedType")
        .is_some_and(|value| !value.is_null())
        || snapshot.get("spendControlReached").and_then(Value::as_bool) == Some(true)
        || ["primary", "secondary"].into_iter().any(|window| {
            snapshot
                .get(window)
                .and_then(|window| window.get("usedPercent"))
                .and_then(Value::as_i64)
                .is_some_and(|used| used >= 100)
        })
}

#[page("/")]
async fn home(cx: &Cx) -> Result {
    public_state(cx)?;
    bootstrap_document(cx).await
}

#[page("/i/{instance_id}")]
async fn dashboard_page(cx: &Cx) -> Result {
    let state = authorized_state(cx)?;
    let runtime = active_runtime(state)?;
    let data = dashboard::load(&runtime.bridge).await;
    let accounts = account_profiles(state)?;
    let base_path = base_path(state);
    dashboard_document(
        cx,
        DashboardDocumentData {
            base_path: &base_path,
            projects: &data.projects,
            threads: &data.threads,
            account: data.account.as_ref(),
            truncated: data.truncated,
            profile_label: &runtime.account_label,
            proxy_active: runtime.proxy_active,
            mcp_servers: &data.mcp_servers,
            accounts: &accounts,
            active_account: &runtime.account_name,
        },
    )
    .await
}

#[page("/i/{instance_id}/new")]
async fn new_thread_page(cx: &Cx) -> Result {
    let state = authorized_state(cx)?;
    let selected_cwd = query_value(uri(cx).query().unwrap_or_default(), "cwd")
        .map(|encoded| urlencoding::decode(&encoded).map(std::borrow::Cow::into_owned))
        .transpose()
        .map_err(Error::other)?
        .map(|path| directory::canonical_directory(&path))
        .transpose()?
        .map(|path| directory::path_string(&path))
        .transpose()?;
    render_page(cx, state, None, selected_cwd.as_deref()).await
}

#[page("/i/{instance_id}/thread/{thread_id}")]
async fn thread_page(cx: &Cx) -> Result {
    let state = authorized_state(cx)?;
    render_page(cx, state, Some(path_param::<ThreadId>(cx)), None).await
}

#[route(GET "/assets/app.js")]
async fn app_js(cx: &Cx) -> Result<Response> {
    public_state(cx)?;
    static_asset(cx, "text/javascript; charset=utf-8", APP_JS)
}

#[route(GET "/assets/app.css")]
async fn app_css(cx: &Cx) -> Result<Response> {
    public_state(cx)?;
    static_asset(cx, "text/css; charset=utf-8", APP_CSS)
}

#[route(GET "/assets/bootstrap.js")]
async fn bootstrap_js(cx: &Cx) -> Result<Response> {
    public_state(cx)?;
    static_asset(cx, "text/javascript; charset=utf-8", BOOTSTRAP_JS)
}

#[route(GET "/assets/home.js")]
async fn home_js(cx: &Cx) -> Result<Response> {
    public_state(cx)?;
    static_asset(cx, "text/javascript; charset=utf-8", HOME_JS)
}

#[route(GET "/assets/settings.js")]
async fn settings_js(cx: &Cx) -> Result<Response> {
    public_state(cx)?;
    static_asset(cx, "text/javascript; charset=utf-8", SETTINGS_JS)
}

#[route(POST "/auth/exchange")]
async fn exchange_token(cx: &Cx, body: Body) -> Result<Response> {
    let state = public_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let payload: Value = serde_json::from_slice(&bytes)?;
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(unauthorized)?;
    if !state.auth.verify_bootstrap(token) {
        return Err(unauthorized().into());
    }
    let base_path = base_path(state);
    let mut response = json_response(
        cx,
        json!({
            "authenticated": true,
            "basePath": base_path,
        }),
    )?;
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&state.auth.session_cookie(&base_path)?).map_err(Error::other)?,
    );
    Ok(response)
}

#[route(GET "/i/{instance_id}/events")]
async fn event_stream(cx: &Cx) -> Result<Response> {
    let state = authorized_state(cx)?;
    let last_event_id = headers(cx)
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            query_value(uri(cx).query().unwrap_or_default(), "after")
                .and_then(|value| value.parse().ok())
        });
    Ok(stream::response(Arc::clone(&state.events), last_event_id).await)
}

#[route(GET "/oauth/callback/{callback_id}")]
async fn mcp_oauth_callback(cx: &Cx) -> Result<Response> {
    let state = public_state(cx)?;
    let callback_port = state
        .mcp_callback_port
        .ok_or_else(|| Error::other("remote MCP OAuth callback is not configured"))?;
    let callback_id = path_param::<CallbackId>(cx);
    let query = uri(cx).query().unwrap_or_default();
    if !valid_callback_id(callback_id) || query.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(not_found().into());
    }
    let callback_path = format!("/oauth/callback/{callback_id}?{query}");
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, callback_port))
        .await
        .map_err(Error::other)?;
    stream
        .write_all(
            format!(
                "GET {callback_path} HTTP/1.1\r\nHost: 127.0.0.1:{callback_port}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .map_err(Error::other)?;
    let mut encoded = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut encoded)
        .await
        .map_err(Error::other)?;
    let forwarded = String::from_utf8_lossy(&encoded);
    let body = forwarded
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("MCP OAuth callback returned an invalid response")
        .to_string();
    let mut response = body.into_response(cx)?;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    Ok(response)
}

#[route(GET "/i/{instance_id}/api/session/{session_id}")]
async fn session_fragment(cx: &Cx) -> Result<Response> {
    let state = authorized_state(cx)?;
    let runtime = active_runtime(state)?;
    let bridge = runtime.bridge;
    let session_id = path_param::<SessionId>(cx);
    let base_seq = state.events.watermark();
    let active_request = async {
        if session_id == "new" {
            None
        } else {
            let paginated = bridge
                .request(
                    "thread/resume",
                    json!({
                        "threadId": session_id,
                        "excludeTurns": true,
                        "initialTurnsPage": {
                            "limit": 30,
                            "sortDirection": "desc",
                            "itemsView": "full"
                        }
                    }),
                )
                .await;
            match paginated {
                Ok(response) => Some(response),
                Err(_) => bridge
                    .request("thread/resume", json!({ "threadId": session_id }))
                    .await
                    .ok(),
            }
        }
    };
    let (active, approvals) = tokio::join!(active_request, bridge.outstanding_server_requests(),);
    let active_thread = active.as_ref().and_then(|response| response.get("thread"));
    let hydrated_thread = active.as_ref().and_then(thread_with_initial_turns);
    let display_thread = hydrated_thread.as_ref().or(active_thread);
    let fragment = transcript_fragment(cx, display_thread, &approvals).await?;
    let active_turn_id = display_thread
        .and_then(|active_thread| active_thread.get("turns"))
        .and_then(Value::as_array)
        .and_then(|turns| {
            turns
                .iter()
                .rev()
                .find(|turn| turn.get("status").and_then(Value::as_str) == Some("inProgress"))
        })
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = json!({
        "html": fragment.render(cx),
        "baseSeq": base_seq,
        "instanceId": state.instance_id,
        "nextCursor": active.as_ref().and_then(|response| response.pointer("/initialTurnsPage/nextCursor")),
        "threadId": display_thread
            .and_then(|active_thread| active_thread.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "activeTurnId": active_turn_id,
        "title": display_thread.map(thread_title).unwrap_or("New task"),
        "cwd": display_thread
            .and_then(|active_thread| active_thread.get("cwd"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "model": active
            .as_ref()
            .and_then(|response| response.get("model"))
            .and_then(Value::as_str),
        "reasoningEffort": active
            .as_ref()
            .and_then(|response| response.get("reasoningEffort"))
            .and_then(Value::as_str),
        "permissionMode": permission_mode(active.as_ref()),
    });
    let mut response = serde_json::to_vec(&payload)?.into_response(cx)?;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[route(GET "/i/{instance_id}/api/session/{session_id}/turns")]
async fn earlier_turns(cx: &Cx) -> Result<Response> {
    let state = authorized_state(cx)?;
    let bridge = active_runtime(state)?.bridge;
    let session_id = path_param::<SessionId>(cx);
    let cursor = query_value(uri(cx).query().unwrap_or_default(), "cursor");
    let page = bridge
        .request(
            "thread/turns/list",
            json!({
                "threadId": session_id,
                "cursor": cursor,
                "limit": 30,
                "sortDirection": "desc",
                "itemsView": "full"
            }),
        )
        .await
        .map_err(Error::other)?;
    let thread_value = json!({ "turns": chronological_turns(page.get("data")) });
    let fragment = transcript_fragment(cx, Some(&thread_value), &[]).await?;
    json_response(
        cx,
        json!({
            "html": fragment.render(cx),
            "nextCursor": page.get("nextCursor").cloned().unwrap_or(Value::Null)
        }),
    )
}

#[route(POST "/i/{instance_id}/api/shutdown")]
async fn shutdown_process(cx: &Cx) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    state.shutdown.cancel();
    json_response(cx, json!({ "stopping": true }))
}

#[route(POST "/i/{instance_id}/api/rpc")]
async fn rpc(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_RPC_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let mut message: Value = serde_json::from_slice(&bytes)?;
    if message.get("method").and_then(Value::as_str) == Some("turn/start") {
        ensure_healthy_account(state).await.map_err(Error::other)?;
    }
    if message.get("method").and_then(Value::as_str) == Some("thread/start")
        && let Some(cwd) = message
            .pointer("/params/cwd")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    {
        let canonical = directory::canonical_directory(&cwd)?;
        if let Some(params) = message.get_mut("params").and_then(Value::as_object_mut) {
            params.insert(
                "cwd".to_string(),
                Value::String(directory::path_string(&canonical)?),
            );
        }
    }
    let runtime = active_runtime(state)?;
    let bridge = runtime.bridge;
    let recoverable_thread = message
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| *method == "turn/start")
        .and_then(|_| message.pointer("/params/threadId"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if let Some(thread_id) = recoverable_thread.as_deref()
        && let Err(error) = state.recovery.mark_turn_start(
            thread_id,
            &runtime.account_name,
            message.get("params").unwrap_or(&Value::Null),
        )
    {
        tracing::warn!(%error, %thread_id, "failed to persist active session for recovery");
    }
    let envelope = if let Some(method) = message.get("method").and_then(Value::as_str) {
        bridge
            .request_envelope(
                method,
                message.get("params").cloned().unwrap_or_else(|| json!({})),
            )
            .await
            .map_err(Error::other)?
    } else {
        bridge.respond(message).await.map_err(Error::other)?;
        json!({ "result": {} })
    };
    if envelope.get("error").is_some()
        && let Some(thread_id) = recoverable_thread.as_deref()
        && let Err(error) = state.recovery.mark_idle(thread_id)
    {
        tracing::warn!(%error, %thread_id, "failed to remove rejected turn from recovery state");
    }
    let mut response = serde_json::to_vec(&envelope)?.into_response(cx)?;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum AccountProfilesRequest {
    Add {
        name: String,
        label: String,
        proxy: Option<String>,
        use_ssh_tunnel: bool,
    },
    Remove {
        name: String,
    },
    Move {
        name: String,
        direction: AccountMoveDirection,
    },
    Update {
        name: String,
        label: String,
        proxy: Option<String>,
        use_ssh_tunnel: bool,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountActivateRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum AccountAuthRequest {
    ChatgptDeviceCode { name: String },
    ApiKey { name: String, api_key: String },
    Logout { name: String },
    Status { name: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum AccountMoveDirection {
    Up,
    Down,
}

#[route(POST "/i/{instance_id}/api/account-profiles")]
async fn update_account_profiles(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let active_runtime = active_runtime(state)?;
    let active_account = active_runtime.account_name.clone();
    let bytes = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let request: AccountProfilesRequest = serde_json::from_slice(&bytes)?;
    let restart_active = if let AccountProfilesRequest::Update {
        name,
        proxy,
        use_ssh_tunnel,
        ..
    } = &request
    {
        let config = state
            .daemon_config
            .read()
            .map_err(|_| Error::other("daemon config lock is poisoned"))?;
        config
            .accounts
            .iter()
            .find(|account| account.name == *name)
            .is_some_and(|account| {
                account.name == active_account
                    && (account.proxy.as_ref() != proxy.as_ref()
                        || account.use_ssh_tunnel != *use_ssh_tunnel)
            })
    } else {
        false
    };
    if restart_active && has_active_turn(&active_runtime.bridge).await {
        return json_response(
            cx,
            json!({ "error": "stop the active turn before changing its connection" }),
        );
    }
    {
        let mut config = state
            .daemon_config
            .write()
            .map_err(|_| Error::other("daemon config lock is poisoned"))?;
        let mut accounts = config.accounts.clone();
        match request {
            AccountProfilesRequest::Add {
                name,
                label,
                proxy,
                use_ssh_tunnel,
            } => {
                let credential_home = config.credential_home(&name);
                accounts.push(AccountProfile {
                    label: if label.trim().is_empty() {
                        name.clone()
                    } else {
                        label
                    },
                    name,
                    role: crate::daemon_config::AccountRole::Fallback,
                    credential_home,
                    proxy: proxy.filter(|value| !value.trim().is_empty()),
                    use_ssh_tunnel,
                });
            }
            AccountProfilesRequest::Remove { name } => {
                let Some(account) = accounts.iter().find(|account| account.name == name) else {
                    return json_response(cx, json!({ "error": "account profile not found" }));
                };
                if account.role == crate::daemon_config::AccountRole::Primary {
                    return json_response(
                        cx,
                        json!({ "error": "the primary account cannot be removed" }),
                    );
                }
                if account.name == active_account {
                    return json_response(
                        cx,
                        json!({ "error": "switch away from the active account before removing it" }),
                    );
                }
                accounts.retain(|account| account.name != name);
            }
            AccountProfilesRequest::Move { name, direction } => {
                let Some(index) = accounts.iter().position(|account| account.name == name) else {
                    return json_response(cx, json!({ "error": "account profile not found" }));
                };
                if accounts[index].role == crate::daemon_config::AccountRole::Primary {
                    return json_response(
                        cx,
                        json!({ "error": "the primary account stays first" }),
                    );
                }
                let target = match direction {
                    AccountMoveDirection::Up => index.saturating_sub(1).max(1),
                    AccountMoveDirection::Down => (index + 1).min(accounts.len() - 1),
                };
                accounts.swap(index, target);
            }
            AccountProfilesRequest::Update {
                name,
                label,
                proxy,
                use_ssh_tunnel,
            } => {
                let Some(account) = accounts.iter_mut().find(|account| account.name == name) else {
                    return json_response(cx, json!({ "error": "account profile not found" }));
                };
                account.label = if label.trim().is_empty() { name } else { label };
                account.proxy = proxy.filter(|value| !value.trim().is_empty());
                account.use_ssh_tunnel = use_ssh_tunnel;
            }
        }
        if let Err(error) = config.save_accounts(accounts) {
            return json_response(cx, json!({ "error": error.to_string() }));
        }
    }
    if restart_active && let Err(error) = switch_account(state, &active_account).await {
        return json_response(cx, json!({ "error": error.to_string() }));
    }
    json_response(cx, json!({ "restartRequired": false }))
}

#[route(POST "/i/{instance_id}/api/account-activate")]
async fn activate_account(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let request: AccountActivateRequest = serde_json::from_slice(&bytes)?;
    match switch_account(state, &request.name).await {
        Ok(()) => json_response(cx, json!({ "activeAccount": request.name })),
        Err(error) => json_response(cx, json!({ "error": error.to_string() })),
    }
}

#[route(POST "/i/{instance_id}/api/account-auth")]
async fn manage_account_auth(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let request: AccountAuthRequest = serde_json::from_slice(&bytes)?;
    match manage_account_auth_request(state, request).await {
        Ok(value) => json_response(cx, value),
        Err(error) => json_response(cx, json!({ "error": error.to_string() })),
    }
}

#[route(POST "/i/{instance_id}/api/fs/list")]
async fn list_directory(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let request: DirectoryListRequest = serde_json::from_slice(&bytes)?;
    match tokio::task::spawn_blocking(move || directory::list(request)).await {
        Ok(Ok(listing)) => {
            let payload = serde_json::to_value(listing)?;
            json_response(cx, payload)
        }
        Ok(Err(error)) => json_response(cx, json!({ "error": error.to_string() })),
        Err(error) => Err(Error::other(error).into()),
    }
}

async fn render_page(
    cx: &Cx,
    state: &WebState,
    thread_id: Option<&str>,
    selected_cwd: Option<&str>,
) -> Result {
    let runtime = active_runtime(state)?;
    let bridge = runtime.bridge.clone();
    let base_seq = state.events.watermark();
    let threads_request = bridge.request(
        "thread/list",
        json!({ "limit": 100, "sortKey": "recency_at", "sortDirection": "desc" }),
    );
    let models_request = bridge.request("model/list", json!({ "limit": 100 }));
    let collaboration_modes_request = bridge.request("collaborationMode/list", json!({}));
    let mcp_servers_request = bridge.request(
        "mcpServerStatus/list",
        json!({ "limit": 100, "detail": "toolsAndAuthOnly" }),
    );
    let active_request = async {
        match thread_id {
            Some(thread_id) => bridge
                .request(
                    "thread/resume",
                    json!({
                        "threadId": thread_id,
                        "excludeTurns": true,
                        "initialTurnsPage": {
                            "limit": 30,
                            "sortDirection": "desc",
                            "itemsView": "full"
                        }
                    }),
                )
                .await
                .ok(),
            None => None,
        }
    };
    let approvals_request = bridge.outstanding_server_requests();
    let (threads, models, collaboration_modes, mcp_servers, active, approvals) = tokio::join!(
        threads_request,
        models_request,
        collaboration_modes_request,
        mcp_servers_request,
        active_request,
        approvals_request,
    );
    let threads = threads.unwrap_or_else(|_| json!({ "data": [] }));
    let models = models.unwrap_or_else(|_| json!({ "data": [] }));
    let collaboration_modes = collaboration_modes.unwrap_or_else(|_| json!({ "data": [] }));
    let mcp_servers = mcp_servers.unwrap_or_else(|_| json!({ "data": [] }));
    let hydrated_thread = active.as_ref().and_then(thread_with_initial_turns);
    let active_thread = hydrated_thread
        .as_ref()
        .or_else(|| active.as_ref().and_then(|response| response.get("thread")));
    let workspace_cwd = page_workspace_cwd(active_thread, selected_cwd, &state.cwd);
    let base_path = base_path(state);
    let accounts = account_profiles(state)?;
    document(
        cx,
        DocumentData {
            base_path: &base_path,
            instance_id: &state.instance_id,
            base_seq,
            threads: threads
                .get("data")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            active_thread,
            active_model: active
                .as_ref()
                .and_then(|response| response.get("model"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            active_effort: active
                .as_ref()
                .and_then(|response| response.get("reasoningEffort"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            active_permission_mode: permission_mode(active.as_ref()),
            models: models
                .get("data")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            collaboration_modes: collaboration_modes
                .get("data")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            approvals: &approvals,
            workspace_cwd,
            mcp_servers: mcp_servers
                .get("data")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            accounts: &accounts,
            active_account: &runtime.account_name,
            initial_next_cursor: active
                .as_ref()
                .and_then(|response| response.pointer("/initialTurnsPage/nextCursor"))
                .and_then(Value::as_str),
        },
    )
    .await
}

fn page_workspace_cwd<'a>(
    active_thread: Option<&'a Value>,
    selected_cwd: Option<&'a str>,
    daemon_cwd: &'a str,
) -> &'a str {
    active_thread
        .and_then(|thread| thread.get("cwd"))
        .and_then(Value::as_str)
        .or(selected_cwd)
        .unwrap_or(daemon_cwd)
}

fn account_profiles(state: &WebState) -> Result<Vec<AccountProfile>> {
    state
        .daemon_config
        .read()
        .map(|config| config.accounts.clone())
        .map_err(|_| Error::other("daemon config lock is poisoned").into())
}

pub(crate) fn active_runtime(state: &WebState) -> Result<ActiveRuntime> {
    state
        .runtime
        .read()
        .map(|runtime| runtime.clone())
        .map_err(|_| Error::other("active runtime lock is poisoned").into())
}

fn permission_mode(response: Option<&Value>) -> &'static str {
    let profile_id = response
        .and_then(|response| response.pointer("/activePermissionProfile/id"))
        .and_then(Value::as_str);
    match profile_id {
        Some(":danger-full-access") => "full-access",
        Some(":read-only") => "read-only",
        Some(":workspace") => "workspace",
        _ => {
            let approval = response
                .and_then(|response| response.get("approvalPolicy"))
                .and_then(Value::as_str);
            let sandbox = response
                .and_then(|response| response.pointer("/sandbox/type"))
                .and_then(Value::as_str);
            match (approval, sandbox) {
                (Some("never"), Some("danger-full-access")) => "full-access",
                (_, Some("read-only")) => "read-only",
                _ => "workspace",
            }
        }
    }
}

pub(crate) fn authorized_state(cx: &Cx) -> Result<&WebState> {
    let state = instance_state(cx)?;
    if !is_authorized(cx, state) {
        return Err(not_found().into());
    }
    Ok(state)
}

fn instance_state(cx: &Cx) -> Result<&WebState> {
    let state = public_state(cx)?;
    if path_param::<InstanceId>(cx) != state.instance_id {
        return Err(not_found().into());
    }
    Ok(state)
}

fn base_path(state: &WebState) -> String {
    format!("/i/{}", state.instance_id)
}

fn valid_callback_id(callback_id: &str) -> bool {
    callback_id.len() == 12
        && callback_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn public_state(cx: &Cx) -> Result<&WebState> {
    let state: &Arc<WebState> = app_context(cx);
    let host = headers(cx)
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(forbidden)?;
    if !state.allowed_authorities.contains(host) {
        return Err(forbidden().into());
    }
    Ok(state)
}

fn is_authorized(cx: &Cx, state: &WebState) -> bool {
    state.auth.authorize_cookie_header(
        headers(cx)
            .get(COOKIE)
            .and_then(|value| value.to_str().ok()),
    )
}

pub(crate) fn authorize_mutation(cx: &Cx, state: &WebState) -> Result<()> {
    let host = headers(cx)
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(forbidden)?;
    let origin = headers(cx)
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(forbidden)?;
    if !state.allowed_authorities.contains(host) || origin != format!("http://{host}") {
        return Err(forbidden().into());
    }
    Ok(())
}

fn static_asset(cx: &Cx, content_type: &'static str, body: &'static str) -> Result<Response> {
    let mut response = body.into_response(cx)?;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub(crate) fn json_response(cx: &Cx, payload: Value) -> Result<Response> {
    let mut response = serde_json::to_vec(&payload)?.into_response(cx)?;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;

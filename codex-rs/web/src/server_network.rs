use std::io::Error;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use http::HeaderValue;
use http::header::CACHE_CONTROL;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use topcoat::Result;
use topcoat::context::Cx;
use topcoat::router::Body;
use topcoat::router::Response;
use topcoat::router::route;
use topcoat::router::to_bytes;

use crate::daemon_config::SshTunnelConfig;
use crate::server::ActiveRuntime;
use crate::server::WebState;
use crate::server::active_runtime;
use crate::server::authorize_mutation;
use crate::server::authorized_state;
use crate::server::has_active_turn;
use crate::server::json_response;
use crate::server::start_profile_bridge;
use crate::ssh_tunnel::ManagedSshTunnel;

const MAX_SETTINGS_BODY_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SshTunnelRequest {
    Status,
    Test { config: SshTunnelInput },
    Apply { config: Option<SshTunnelInput> },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshTunnelInput {
    destination: String,
    identity_file: Option<PathBuf>,
    ssh_port: Option<u16>,
    local_port: Option<u16>,
}

impl SshTunnelInput {
    fn validate(self) -> std::io::Result<SshTunnelConfig> {
        SshTunnelConfig::validated(
            self.destination,
            self.identity_file,
            self.ssh_port,
            self.local_port,
        )
    }
}

fn ssh_tunnel_json(config: Option<&SshTunnelConfig>) -> Value {
    config.map_or(Value::Null, |config| {
        json!({
            "destination": config.destination,
            "identityFile": config.identity_file.as_ref().map(|path| path.display().to_string()),
            "sshPort": config.ssh_port,
            "localPort": config.local_port,
        })
    })
}

#[route(POST "/i/{instance_id}/api/private-link")]
async fn private_link(cx: &Cx) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let mut response = json_response(
        cx,
        json!({ "path": format!("/#bootstrap={}", state.auth.bootstrap_token()) }),
    )?;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[route(POST "/i/{instance_id}/api/ssh-tunnel")]
async fn manage_ssh_tunnel(cx: &Cx, body: Body) -> Result<Response> {
    let state = authorized_state(cx)?;
    authorize_mutation(cx, state)?;
    let bytes = to_bytes(body, MAX_SETTINGS_BODY_BYTES)
        .await
        .map_err(Error::other)?;
    let request: SshTunnelRequest = serde_json::from_slice(&bytes)?;
    match request {
        SshTunnelRequest::Status => tunnel_status(cx, state).await,
        SshTunnelRequest::Test { config } => test_tunnel(cx, config).await,
        SshTunnelRequest::Apply { config } => apply_ssh_tunnel(cx, state, config).await,
    }
}

async fn tunnel_status(cx: &Cx, state: &WebState) -> Result<Response> {
    let config = state
        .daemon_config
        .read()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .ssh_tunnel
        .clone();
    let mut tunnel = state.ssh_tunnel.lock().await;
    let connected = match tunnel.as_mut() {
        Some(tunnel) => tunnel.is_running().unwrap_or(false),
        None => false,
    };
    let proxy_url = tunnel.as_ref().map(ManagedSshTunnel::proxy_url);
    json_response(
        cx,
        json!({
            "state": if config.is_none() { "disabled" } else if connected { "connected" } else { "failed" },
            "config": ssh_tunnel_json(config.as_ref()),
            "proxyUrl": proxy_url,
        }),
    )
}

async fn test_tunnel(cx: &Cx, input: SshTunnelInput) -> Result<Response> {
    let config = match input.validate() {
        Ok(config) => config,
        Err(error) => return json_response(cx, json!({ "error": error.to_string() })),
    };
    match ManagedSshTunnel::start(&config).await {
        Ok(mut tunnel) => {
            let proxy_url = tunnel.proxy_url().to_string();
            tunnel.shutdown().await;
            json_response(cx, json!({ "state": "connected", "proxyUrl": proxy_url }))
        }
        Err(error) => json_response(cx, json!({ "error": error.to_string() })),
    }
}

async fn apply_ssh_tunnel(
    cx: &Cx,
    state: &WebState,
    input: Option<SshTunnelInput>,
) -> Result<Response> {
    let config = match input.map(SshTunnelInput::validate).transpose() {
        Ok(config) => config,
        Err(error) => return json_response(cx, json!({ "error": error.to_string() })),
    };
    let current = active_runtime(state)?;
    let profile = state
        .daemon_config
        .read()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .accounts
        .iter()
        .find(|profile| profile.name == current.account_name)
        .cloned()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "active account profile not found"))?;
    if profile.use_ssh_tunnel && has_active_turn(&current.bridge).await {
        return json_response(
            cx,
            json!({ "error": "stop the active turn before changing its SSH tunnel" }),
        );
    }
    let mut candidate_tunnel = match config.as_ref() {
        Some(config) => match ManagedSshTunnel::start(config).await {
            Ok(tunnel) => Some(tunnel),
            Err(error) => return json_response(cx, json!({ "error": error.to_string() })),
        },
        None => None,
    };
    let candidate_proxy = candidate_tunnel
        .as_ref()
        .map(ManagedSshTunnel::proxy_url)
        .map(ToOwned::to_owned);
    let mut candidate_launch = state.runtime_launch.clone();
    candidate_launch.ssh_proxy = Arc::new(RwLock::new(candidate_proxy.clone()));
    let replacement = if profile.use_ssh_tunnel {
        match start_profile_bridge(&profile, &candidate_launch).await {
            Ok(runtime) => Some(runtime),
            Err(error) => {
                if let Some(tunnel) = candidate_tunnel.as_mut() {
                    tunnel.shutdown().await;
                }
                return json_response(cx, json!({ "error": error.to_string() }));
            }
        }
    } else {
        None
    };
    let save_result = state
        .daemon_config
        .write()
        .map_err(|_| Error::other("daemon config lock is poisoned"))?
        .save_ssh_tunnel(config.clone());
    if let Err(error) = save_result {
        if let Some((bridge, _)) = replacement {
            bridge.shutdown().await;
        }
        if let Some(tunnel) = candidate_tunnel.as_mut() {
            tunnel.shutdown().await;
        }
        return json_response(cx, json!({ "error": error.to_string() }));
    }
    *state
        .runtime_launch
        .ssh_proxy
        .write()
        .map_err(|_| Error::other("SSH proxy lock is poisoned"))? = candidate_proxy.clone();
    let old_tunnel = {
        let mut tunnel = state.ssh_tunnel.lock().await;
        std::mem::replace(&mut *tunnel, candidate_tunnel.take())
    };
    if let Some((bridge, proxy_active)) = replacement {
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
                account_label: profile.label.clone(),
                proxy_active,
            };
        }
        state.events.runtime_changed(&profile.name).await;
        current.bridge.shutdown().await;
    }
    if let Some(mut tunnel) = old_tunnel {
        tunnel.shutdown().await;
    }
    json_response(
        cx,
        json!({
            "state": if config.is_some() { "connected" } else { "disabled" },
            "config": ssh_tunnel_json(config.as_ref()),
            "proxyUrl": candidate_proxy,
        }),
    )
}

#[cfg(test)]
#[path = "server_network_tests.rs"]
mod tests;

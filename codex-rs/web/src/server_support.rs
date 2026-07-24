use std::io::Error;
use std::net::SocketAddr;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use serde_json::Value;

use crate::network::authority;

pub(crate) fn canonicalize_cwd(cwd: PathBuf) -> std::io::Result<PathBuf> {
    cwd.canonicalize().map_err(|error| {
        Error::new(
            error.kind(),
            format!(
                "failed to resolve working directory {}: {error}",
                cwd.display()
            ),
        )
    })
}

pub(crate) fn generate_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn mcp_callback_url(addresses: &[SocketAddr]) -> Option<String> {
    let address = addresses
        .iter()
        .find(|address| !address.ip().is_loopback() && address.is_ipv4())
        .or_else(|| addresses.iter().find(|address| !address.ip().is_loopback()))?;
    Some(format!("http://{}/oauth/callback", authority(*address)))
}

pub(crate) fn thread_with_initial_turns(response: &Value) -> Option<Value> {
    let mut thread_value = response.get("thread")?.clone();
    let turns = chronological_turns(response.pointer("/initialTurnsPage/data"));
    thread_value
        .as_object_mut()?
        .insert("turns".to_string(), turns);
    Some(thread_value)
}

pub(crate) fn chronological_turns(turns: Option<&Value>) -> Value {
    let mut turns = turns.and_then(Value::as_array).cloned().unwrap_or_default();
    turns.reverse();
    Value::Array(turns)
}

pub(crate) fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

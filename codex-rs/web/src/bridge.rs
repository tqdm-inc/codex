use std::collections::HashMap;
use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::AppServerCommand;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const EVENT_CAPACITY: usize = 1024;

pub(crate) struct AppServerBridge {
    outbound: mpsc::Sender<Vec<u8>>,
    child: Mutex<Option<Child>>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    server_requests: Mutex<HashMap<String, Value>>,
    events: broadcast::Sender<Value>,
}

impl AppServerBridge {
    pub(crate) async fn start(
        app_server_command: &AppServerCommand,
        cwd: &Path,
        codex_home: &Path,
        proxy: Option<&str>,
        api_key: Option<&str>,
        config_overrides: &[String],
        strict_config: bool,
    ) -> std::io::Result<Arc<Self>> {
        let (executable, mut command) = match app_server_command {
            AppServerCommand::Standalone { executable } => {
                let mut command = Command::new(executable);
                command.arg("__app-server");
                (executable, command)
            }
            AppServerCommand::CodexCli { executable } => {
                let command = Command::new(executable);
                (executable, command)
            }
        };
        if matches!(app_server_command, AppServerCommand::CodexCli { .. }) {
            for config_override in config_overrides {
                command.args(["-c", config_override]);
            }
            command.arg("app-server");
        } else {
            for config_override in config_overrides {
                command.args(["-c", config_override]);
            }
        }
        if strict_config {
            command.arg("--strict-config");
        }
        if api_key.is_some() && matches!(app_server_command, AppServerCommand::Standalone { .. }) {
            command.arg("--enable-openai-api-key-env");
        }
        command.env("CODEX_HOME", codex_home);
        if let Some(api_key) = api_key {
            command.env("OPENAI_API_KEY", api_key);
        }
        if let Some(proxy) = proxy {
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                command.env(key, proxy);
            }
            for key in ["NO_PROXY", "no_proxy"] {
                command.env(key, "127.0.0.1,localhost,::1");
            }
        }
        let mut child = command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                Error::new(
                    error.kind(),
                    format!(
                        "failed to start Codex app-server with `{}`: {error}",
                        executable.display()
                    ),
                )
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::other("app-server stdin was unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::other("app-server stdout was unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::other("app-server stderr was unavailable"))?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (outbound, mut outbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let bridge = Arc::new(Self {
            outbound,
            child: Mutex::new(Some(child)),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            server_requests: Mutex::new(HashMap::new()),
            events,
        });

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(encoded) = outbound_rx.recv().await {
                if let Err(error) = stdin.write_all(&encoded).await {
                    tracing::warn!(%error, "failed writing to app-server");
                    break;
                }
                if let Err(error) = stdin.flush().await {
                    tracing::warn!(%error, "failed flushing app-server input");
                    break;
                }
            }
        });

        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("AuthRequired") {
                    tracing::debug!(line, "MCP server needs authentication");
                } else {
                    tracing::warn!(line, "app-server diagnostic");
                }
            }
        });

        let reader_bridge = Arc::clone(&bridge);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => reader_bridge.handle_line(&line).await,
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "failed reading app-server output");
                        break;
                    }
                }
            }
        });

        bridge
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "codex_web",
                        "title": "Codex Web",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await?;
        bridge
            .send(json!({ "jsonrpc": "2.0", "method": "initialized" }))
            .await?;
        Ok(bridge)
    }

    pub(crate) async fn request(&self, method: &str, params: Value) -> std::io::Result<Value> {
        let response = self.request_envelope(method, params).await?;
        if let Some(error) = response.get("error") {
            return Err(Error::other(
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("app-server request failed"),
            ));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub(crate) async fn request_envelope(
        &self,
        method: &str,
        params: Value,
    ) -> std::io::Result<Value> {
        let id = format!("web-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let (response_tx, response_rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), response_tx);
        if let Err(error) = self
            .send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match timeout(REQUEST_TIMEOUT, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(Error::new(
                ErrorKind::BrokenPipe,
                "app-server response channel closed",
            )),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(Error::new(
                    ErrorKind::TimedOut,
                    format!("{method} timed out"),
                ))
            }
        }
    }

    pub(crate) async fn respond(&self, response: Value) -> std::io::Result<()> {
        let resolved_id = response.get("id").cloned();
        if let Some(id) = &resolved_id {
            self.server_requests.lock().await.remove(&id_key(id));
        }
        self.send(response).await?;
        if let Some(id) = resolved_id {
            let _ = self.events.send(json!({
                "method": "web/requestResolved",
                "params": { "id": id }
            }));
        }
        Ok(())
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub(crate) async fn outstanding_server_requests(&self) -> Vec<Value> {
        self.server_requests
            .lock()
            .await
            .values()
            .cloned()
            .collect()
    }

    pub(crate) async fn shutdown(&self) {
        let Some(mut child) = self.child.lock().await.take() else {
            return;
        };
        let _ = child.start_kill();
        let _ = timeout(SHUTDOWN_TIMEOUT, child.wait()).await;
    }

    async fn send(&self, message: Value) -> std::io::Result<()> {
        let mut encoded = serde_json::to_vec(&message).map_err(Error::other)?;
        encoded.push(b'\n');
        self.outbound
            .send(encoded)
            .await
            .map_err(|_| Error::new(ErrorKind::BrokenPipe, "app-server input channel closed"))
    }

    async fn handle_line(&self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            tracing::debug!(line, "ignoring non-JSON app-server output");
            return;
        };
        if message.get("method").is_none()
            && let Some(id) = message.get("id")
            && let Some(response_tx) = self.pending.lock().await.remove(&id_key(id))
        {
            let _ = response_tx.send(message);
            return;
        }
        if message.get("method").is_some()
            && let Some(id) = message.get("id")
        {
            self.server_requests
                .lock()
                .await
                .insert(id_key(id), message.clone());
        }
        let _ = self.events.send(message);
    }
}

fn id_key(id: &Value) -> String {
    id.as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| id.to_string())
}

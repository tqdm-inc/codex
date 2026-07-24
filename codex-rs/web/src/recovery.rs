use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use tempfile::NamedTempFile;

use crate::bridge::AppServerBridge;

const RECOVERY_STATE_VERSION: u32 = 1;
const RECOVERY_INSTRUCTION: &str = "The web daemon restarted while this session was running. Continue the previous task in the same mode. First inspect the persisted conversation and workspace state, verify any partially completed tool actions before repeating them, then carry on from where the interrupted turn stopped.";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoverableSession {
    pub(crate) thread_id: String,
    pub(crate) account_name: String,
    #[serde(default, flatten)]
    settings: SavedTurnSettings,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedTurnSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collaboration_mode: Option<Value>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryFile {
    version: u32,
    sessions: Vec<RecoverableSession>,
}

#[derive(Default)]
struct RecoveryState {
    sessions: HashMap<String, RecoverableSession>,
    restoring: HashSet<String>,
    settings: HashMap<String, SavedTurnSettings>,
}

pub(crate) struct RecoveryManager {
    path: PathBuf,
    state: Mutex<RecoveryState>,
}

impl RecoveryManager {
    pub(crate) fn load(path: PathBuf) -> Arc<Self> {
        let sessions = match read_recovery_file(&path) {
            Ok(sessions) => sessions,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "ignoring unreadable recovery state");
                Vec::new()
            }
        };
        Arc::new(Self {
            path,
            state: Mutex::new(RecoveryState {
                sessions: sessions
                    .into_iter()
                    .map(|session| (session.thread_id.clone(), session))
                    .collect(),
                ..Default::default()
            }),
        })
    }

    pub(crate) fn preferred_account(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .values()
            .min_by(|left, right| left.thread_id.cmp(&right.thread_id))
            .map(|session| session.account_name.clone())
    }

    pub(crate) fn begin_restore(&self) -> Vec<RecoverableSession> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut sessions = state.sessions.values().cloned().collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
        state
            .restoring
            .extend(sessions.iter().map(|session| session.thread_id.clone()));
        sessions
    }

    pub(crate) fn finish_restore(&self, thread_id: &str) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .restoring
            .remove(thread_id);
    }

    pub(crate) fn mark_active(&self, thread_id: &str, account_name: &str) -> std::io::Result<()> {
        self.update(|state| {
            let settings = state
                .settings
                .get(thread_id)
                .cloned()
                .or_else(|| {
                    state
                        .sessions
                        .get(thread_id)
                        .map(|session| session.settings.clone())
                })
                .unwrap_or_default();
            state.sessions.insert(
                thread_id.to_string(),
                RecoverableSession {
                    thread_id: thread_id.to_string(),
                    account_name: account_name.to_string(),
                    settings,
                },
            );
        })
    }

    pub(crate) fn mark_turn_start(
        &self,
        thread_id: &str,
        account_name: &str,
        params: &Value,
    ) -> std::io::Result<()> {
        self.remember_settings(thread_id, params)?;
        self.mark_active(thread_id, account_name)
    }

    pub(crate) fn mark_idle(&self, thread_id: &str) -> std::io::Result<()> {
        self.update(|state| {
            if !state.restoring.contains(thread_id) {
                state.sessions.remove(thread_id);
            }
        })
    }

    pub(crate) fn discard(&self, thread_id: &str) -> std::io::Result<()> {
        self.update(|state| {
            state.restoring.remove(thread_id);
            state.sessions.remove(thread_id);
        })
    }

    pub(crate) fn attach(self: &Arc<Self>, bridge: Arc<AppServerBridge>, account_name: String) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut events = bridge.subscribe();
            loop {
                match events.recv().await {
                    Ok(message) => {
                        manager
                            .handle_notification(&bridge, &account_name, &message)
                            .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "recovery tracker lagged; reconciling loaded sessions"
                        );
                        manager.reconcile_loaded(&bridge, &account_name).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn handle_notification(
        &self,
        bridge: &Arc<AppServerBridge>,
        account_name: &str,
        message: &Value,
    ) {
        let method = message.get("method").and_then(Value::as_str);
        let params = message.get("params").unwrap_or(&Value::Null);
        let thread_id = params.get("threadId").and_then(Value::as_str);
        let result = match method {
            Some("thread/started") => {
                let thread = params.get("thread").unwrap_or(&Value::Null);
                let thread_id = thread.get("id").and_then(Value::as_str);
                if is_root_active_thread(thread) {
                    thread_id.map(|thread_id| self.mark_active(thread_id, account_name))
                } else {
                    None
                }
            }
            Some("thread/status/changed")
                if params.pointer("/status/type").and_then(Value::as_str) == Some("active") =>
            {
                match thread_id {
                    Some(thread_id) => {
                        self.confirm_root_active(bridge, thread_id, account_name)
                            .await
                    }
                    None => None,
                }
            }
            Some("thread/status/changed") => thread_id.map(|thread_id| self.mark_idle(thread_id)),
            Some("thread/settings/updated") => match thread_id {
                Some(thread_id) => params
                    .get("threadSettings")
                    .map(|settings| self.remember_settings(thread_id, settings)),
                None => None,
            },
            Some("thread/closed")
            | Some("thread/deleted")
            | Some("thread/archived")
            | Some("thread/unarchived") => thread_id.map(|thread_id| self.discard(thread_id)),
            _ => None,
        };
        if let Some(Err(error)) = result {
            tracing::warn!(%error, ?method, "failed to update recovery state");
        }
    }

    async fn confirm_root_active(
        &self,
        bridge: &Arc<AppServerBridge>,
        thread_id: &str,
        account_name: &str,
    ) -> Option<std::io::Result<()>> {
        let thread = match bridge
            .request(
                "thread/read",
                json!({ "threadId": thread_id, "includeTurns": false }),
            )
            .await
        {
            Ok(response) => response.get("thread").cloned().unwrap_or(Value::Null),
            Err(error) => {
                tracing::warn!(%error, %thread_id, "could not inspect active thread for recovery");
                return None;
            }
        };
        is_root_active_thread(&thread).then(|| self.mark_active(thread_id, account_name))
    }

    async fn reconcile_loaded(&self, bridge: &Arc<AppServerBridge>, account_name: &str) {
        let mut cursor = Value::Null;
        loop {
            let loaded = match bridge
                .request(
                    "thread/loaded/list",
                    json!({ "limit": 100, "cursor": cursor }),
                )
                .await
            {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::warn!(%error, "failed to reconcile loaded sessions for recovery");
                    return;
                }
            };
            let Some(thread_ids) = loaded.get("data").and_then(Value::as_array) else {
                return;
            };
            for thread_id in thread_ids.iter().filter_map(Value::as_str) {
                let _ = self
                    .confirm_root_active(bridge, thread_id, account_name)
                    .await;
            }
            cursor = loaded.get("nextCursor").cloned().unwrap_or(Value::Null);
            if cursor.is_null() {
                return;
            }
        }
    }

    fn update(&self, update: impl FnOnce(&mut RecoveryState)) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = state.sessions.clone();
        update(&mut state);
        if state.sessions == previous {
            return Ok(());
        }
        if let Err(error) = write_recovery_file(&self.path, state.sessions.values()) {
            state.sessions = previous;
            return Err(error);
        }
        Ok(())
    }

    fn remember_settings(&self, thread_id: &str, params: &Value) -> std::io::Result<()> {
        let settings = SavedTurnSettings::from_params(params);
        if settings == SavedTurnSettings::default() {
            return Ok(());
        }
        self.update(|state| {
            state
                .settings
                .insert(thread_id.to_string(), settings.clone());
            if let Some(session) = state.sessions.get_mut(thread_id) {
                session.settings = settings;
            }
        })
    }
}

pub(crate) async fn restore_sessions(
    manager: &Arc<RecoveryManager>,
    bridge: &Arc<AppServerBridge>,
    active_account: &str,
) {
    for session in manager.begin_restore() {
        if session.account_name != active_account {
            tracing::warn!(
                thread_id = %session.thread_id,
                previous_account = %session.account_name,
                active_account,
                "recovering interrupted session with a fallback account"
            );
        }
        match restore_session(bridge, &session).await {
            Ok(RestoreOutcome::Continued) => {
                manager.finish_restore(&session.thread_id);
                tracing::info!(thread_id = %session.thread_id, "continued interrupted session");
            }
            Ok(RestoreOutcome::GoalContinued) => {
                manager.finish_restore(&session.thread_id);
                tracing::info!(thread_id = %session.thread_id, "resumed active session goal");
            }
            Ok(RestoreOutcome::Discarded) => {
                if let Err(error) = manager.discard(&session.thread_id) {
                    tracing::warn!(%error, thread_id = %session.thread_id, "failed to discard stale recovery entry");
                }
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    thread_id = %session.thread_id,
                    "could not recover interrupted session; it will be retried after the next restart"
                );
            }
        }
    }
}

enum RestoreOutcome {
    Continued,
    GoalContinued,
    Discarded,
}

async fn restore_session(
    bridge: &Arc<AppServerBridge>,
    session: &RecoverableSession,
) -> std::io::Result<RestoreOutcome> {
    let thread_id = &session.thread_id;
    let resumed = bridge
        .request(
            "thread/resume",
            json!({
                "threadId": thread_id,
                "excludeTurns": true,
                "initialTurnsPage": {
                    "limit": 1,
                    "sortDirection": "desc",
                    "itemsView": "full"
                }
            }),
        )
        .await?;
    if resumed
        .pointer("/thread/parentThreadId")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(RestoreOutcome::Discarded);
    }
    let goal = bridge
        .request("thread/goal/get", json!({ "threadId": thread_id }))
        .await?;
    if goal.pointer("/goal/status").and_then(Value::as_str) == Some("active") {
        return Ok(RestoreOutcome::GoalContinued);
    }
    if latest_turn_status(&resumed).is_some_and(|status| matches!(status, "completed" | "failed")) {
        return Ok(RestoreOutcome::Discarded);
    }
    bridge
        .request(
            "turn/start",
            recovery_turn_params(thread_id, RECOVERY_INSTRUCTION, &session.settings),
        )
        .await?;
    Ok(RestoreOutcome::Continued)
}

fn latest_turn_status(resumed: &Value) -> Option<&str> {
    resumed
        .pointer("/initialTurnsPage/data/0/status")
        .and_then(Value::as_str)
}

fn recovery_turn_params(thread_id: &str, instruction: &str, settings: &SavedTurnSettings) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "input": [{
            "type": "text",
            "text": instruction,
            "text_elements": []
        }]
    });
    let Some(params) = params.as_object_mut() else {
        return Value::Null;
    };
    if let Some(model) = &settings.model {
        params.insert("model".to_string(), Value::String(model.clone()));
    }
    if let Some(effort) = &settings.effort {
        params.insert("effort".to_string(), effort.clone());
    }
    if let Some(collaboration_mode) = &settings.collaboration_mode {
        params.insert("collaborationMode".to_string(), collaboration_mode.clone());
    }
    Value::Object(params.clone())
}

fn is_root_active_thread(thread: &Value) -> bool {
    thread.get("parentThreadId").is_none_or(Value::is_null)
        && thread.pointer("/status/type").and_then(Value::as_str) == Some("active")
}

impl SavedTurnSettings {
    fn from_params(params: &Value) -> Self {
        Self {
            model: params
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            effort: params.get("effort").cloned(),
            collaboration_mode: params.get("collaborationMode").cloned(),
        }
    }
}

fn read_recovery_file(path: &Path) -> std::io::Result<Vec<RecoverableSession>> {
    let encoded = match fs::read(path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let recovery: RecoveryFile = serde_json::from_slice(&encoded)
        .map_err(|error| Error::new(ErrorKind::InvalidData, error))?;
    if recovery.version != RECOVERY_STATE_VERSION {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("unsupported recovery state version {}", recovery.version),
        ));
    }
    Ok(recovery.sessions)
}

fn write_recovery_file<'a>(
    path: &Path,
    sessions: impl Iterator<Item = &'a RecoverableSession>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "recovery path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut sessions = sessions.cloned().collect::<Vec<_>>();
    sessions.sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
    let encoded = serde_json::to_vec_pretty(&RecoveryFile {
        version: RECOVERY_STATE_VERSION,
        sessions,
    })
    .map_err(Error::other)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&encoded)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

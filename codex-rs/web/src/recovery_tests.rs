use std::fs;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::RECOVERY_INSTRUCTION;
use super::RecoverableSession;
use super::RecoveryManager;
use super::SavedTurnSettings;
use super::is_root_active_thread;
use super::latest_turn_status;
use super::recovery_turn_params;

#[test]
fn persists_only_sessions_that_remain_active() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("recovery.json");
    let manager = RecoveryManager::load(path.clone());

    manager
        .mark_active("thread-a", "work")
        .expect("mark active");
    manager
        .mark_active("thread-b", "work")
        .expect("mark active");
    manager.mark_idle("thread-a").expect("mark idle");

    let restored = RecoveryManager::load(path);
    assert_eq!(
        restored.begin_restore(),
        vec![RecoverableSession {
            thread_id: "thread-b".to_string(),
            account_name: "work".to_string(),
            settings: SavedTurnSettings::default(),
        }]
    );
}

#[test]
fn restoring_session_is_not_removed_by_transient_idle_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("recovery.json");
    let manager = RecoveryManager::load(path.clone());
    manager
        .mark_active("thread-a", "work")
        .expect("mark active");

    assert_eq!(manager.begin_restore().len(), 1);
    manager.mark_idle("thread-a").expect("mark idle");

    assert_eq!(RecoveryManager::load(path).begin_restore().len(), 1);
}

#[test]
fn invalid_recovery_file_does_not_prevent_loading() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("recovery.json");
    fs::write(&path, b"not json").expect("write invalid state");

    assert_eq!(RecoveryManager::load(path).begin_restore(), Vec::new());
}

#[test]
fn recovery_turn_keeps_thread_settings_implicit() {
    assert_eq!(
        recovery_turn_params(
            "thread-a",
            RECOVERY_INSTRUCTION,
            &SavedTurnSettings::default()
        ),
        json!({
            "threadId": "thread-a",
            "input": [{
                "type": "text",
                "text": RECOVERY_INSTRUCTION,
                "text_elements": []
            }]
        })
    );
}

#[test]
fn recovery_turn_restores_collaboration_mode_model_and_effort() {
    let settings = SavedTurnSettings::from_params(&json!({
        "model": "gpt-5.6",
        "effort": "high",
        "collaborationMode": {
            "mode": "plan",
            "settings": {
                "model": "gpt-5.6",
                "reasoning_effort": "high",
                "developer_instructions": null
            }
        }
    }));

    let params = recovery_turn_params("thread-a", RECOVERY_INSTRUCTION, &settings);

    assert_eq!(params.get("model"), Some(&json!("gpt-5.6")));
    assert_eq!(params.get("effort"), Some(&json!("high")));
    assert_eq!(
        params.pointer("/collaborationMode/mode"),
        Some(&json!("plan"))
    );
}

#[test]
fn completed_turn_is_not_recovered_again() {
    let resumed = json!({
        "initialTurnsPage": {
            "data": [{ "id": "turn-a", "status": "completed" }]
        }
    });

    let status = latest_turn_status(&resumed);
    assert_eq!(status, Some("completed"));
    assert_eq!(
        status.is_some_and(|status| matches!(status, "completed" | "failed")),
        true
    );
}

#[test]
fn only_active_root_threads_are_tracked() {
    assert_eq!(
        is_root_active_thread(&json!({
            "id": "root",
            "parentThreadId": null,
            "status": { "type": "active", "activeFlags": [] }
        })),
        true
    );
    assert_eq!(
        is_root_active_thread(&json!({
            "id": "child",
            "parentThreadId": "root",
            "status": { "type": "active", "activeFlags": [] }
        })),
        false
    );
}

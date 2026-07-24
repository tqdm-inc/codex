use std::sync::atomic::AtomicU64;

use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use super::EventRenderer;
use super::LiveEventHub;
use super::ReplayState;
use super::truncated_append;

#[tokio::test]
async fn item_ids_are_scoped_by_thread_and_turn() {
    let mut renderer = EventRenderer::default();
    let first = renderer
        .render(json!({
            "method": "item/agentMessage/delta",
            "params": { "threadId": "thread-a", "turnId": "turn-a", "itemId": "same", "delta": "A" }
        }))
        .await
        .expect("first event");
    let second = renderer
        .render(json!({
            "method": "item/agentMessage/delta",
            "params": { "threadId": "thread-b", "turnId": "turn-b", "itemId": "same", "delta": "B" }
        }))
        .await
        .expect("second event");

    assert_eq!(first.get("threadId"), Some(&json!("thread-a")));
    assert_eq!(second.get("threadId"), Some(&json!("thread-b")));
    assert!(
        first
            .get("html")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|html| html.contains("A"))
    );
    assert!(
        second
            .get("html")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|html| html.contains("B"))
    );
}

#[tokio::test]
async fn plan_actions_wait_for_item_and_turn_completion() {
    let mut renderer = EventRenderer::default();
    let started = renderer
        .render(json!({
            "method": "item/started",
            "params": {
                "threadId": "thread-a",
                "turnId": "turn-a",
                "item": { "id": "plan-a", "type": "plan", "text": "# Draft" }
            }
        }))
        .await
        .expect("started plan");
    let completed = renderer
        .render(json!({
            "method": "item/completed",
            "params": {
                "threadId": "thread-a",
                "turnId": "turn-a",
                "item": { "id": "plan-a", "type": "plan", "text": "# Final" }
            }
        }))
        .await
        .expect("completed plan");
    let started_html = started.get("html").and_then(serde_json::Value::as_str);
    let completed_html = completed.get("html").and_then(serde_json::Value::as_str);

    assert!(started_html.is_some_and(|html| {
        html.contains("data-plan-complete=\"false\"")
            && html.contains("data-plan-actionable=\"false\"")
    }));
    assert!(completed_html.is_some_and(|html| {
        html.contains("data-plan-complete=\"true\"")
            && html.contains("data-plan-actionable=\"false\"")
    }));
    assert_eq!(completed.get("turnId"), Some(&json!("turn-a")));
}

#[tokio::test]
async fn activity_items_target_the_turn_work_digest() {
    let mut renderer = EventRenderer::default();
    let event = renderer
        .render(json!({
            "method": "item/started",
            "params": {
                "threadId": "thread-a",
                "turnId": "turn-a",
                "item": {
                    "id": "command-a",
                    "type": "commandExecution",
                    "command": "cargo check",
                    "status": "inProgress"
                }
            }
        }))
        .await
        .expect("activity event");

    assert_eq!(event.get("op"), Some(&json!("activityUpsert")));
    assert_eq!(event.get("digestId"), Some(&json!("work-turn-a")));
    assert!(
        event
            .get("digestHtml")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|html| html.contains("data-work-digest"))
    );
}

#[tokio::test]
async fn empty_reasoning_removes_any_streaming_shell() {
    let mut renderer = EventRenderer::default();
    let event = renderer
        .render(json!({
            "method": "item/completed",
            "params": {
                "threadId": "thread-a",
                "turnId": "turn-a",
                "item": {
                    "id": "reasoning-a",
                    "type": "reasoning",
                    "summary": [],
                    "content": []
                }
            }
        }))
        .await
        .expect("remove event");

    assert_eq!(
        event,
        json!({
            "op": "remove",
            "threadId": "thread-a",
            "turnId": "turn-a",
            "domId": "item-reasoning-a"
        })
    );
}

#[tokio::test]
async fn skill_changes_invalidate_browser_discovery() {
    let mut renderer = EventRenderer::default();
    let event = renderer
        .render(json!({ "method": "skills/changed", "params": {} }))
        .await
        .expect("skills changed event");

    assert_eq!(event, json!({ "op": "skillsChanged" }));
}

#[tokio::test]
async fn turn_events_preserve_the_nested_turn_id() {
    let mut renderer = EventRenderer::default();
    let event = renderer
        .render(json!({
            "method": "turn/started",
            "params": {
                "threadId": "thread-a",
                "turn": { "id": "turn-a" }
            }
        }))
        .await
        .expect("turn event");

    assert_eq!(
        event,
        json!({
            "op": "turn",
            "threadId": "thread-a",
            "turnId": "turn-a",
            "status": "inProgress"
        })
    );
}

#[tokio::test]
async fn thread_settings_updates_project_permission_state() {
    let mut renderer = EventRenderer::default();
    let event = renderer
        .render(json!({
            "method": "thread/settings/updated",
            "params": {
                "threadId": "thread-a",
                "threadSettings": {
                    "activePermissionProfile": { "id": ":danger-full-access" },
                    "approvalPolicy": "never",
                    "sandboxPolicy": { "type": "danger-full-access" }
                }
            }
        }))
        .await
        .expect("settings event");

    assert_eq!(
        event,
        json!({
            "op": "threadSettings",
            "activePermissionProfile": { "id": ":danger-full-access" },
            "approvalPolicy": "never",
            "sandboxPolicy": { "type": "danger-full-access" },
            "threadId": "thread-a",
            "turnId": ""
        })
    );
}

#[tokio::test]
async fn goal_updates_are_scoped_and_preserve_the_full_goal() {
    let mut renderer = EventRenderer::default();
    let goal = json!({
        "threadId": "thread-a",
        "objective": "Ship it",
        "status": "active",
        "tokenBudget": 1000,
        "tokensUsed": 250,
        "timeUsedSeconds": 60,
        "createdAt": 1,
        "updatedAt": 2
    });
    let event = renderer
        .render(json!({
            "method": "thread/goal/updated",
            "params": { "threadId": "thread-a", "goal": goal }
        }))
        .await
        .expect("goal event");

    assert_eq!(
        event,
        json!({
            "op": "goal",
            "goal": goal,
            "threadId": "thread-a",
            "turnId": ""
        })
    );
}

#[test]
fn streamed_output_has_a_hard_cap() {
    let existing = "a".repeat(256 * 1024);
    let combined = truncated_append(&existing, "tail");

    assert!(combined.len() < existing.len() + 64);
    assert!(combined.ends_with("tail"));
    assert!(combined.starts_with("… output truncated …"));
}

#[tokio::test]
async fn replay_returns_only_events_after_the_client_watermark() {
    let (events, _) = broadcast::channel(8);
    let hub = LiveEventHub {
        next_seq: AtomicU64::new(1),
        replay: Mutex::new(ReplayState::default()),
        events,
    };
    hub.publish(json!({ "op": "first" })).await;
    hub.publish(json!({ "op": "second" })).await;

    let replay = hub.replay_after(Some(1)).await;

    assert!(!replay.gap);
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![2]
    );
}

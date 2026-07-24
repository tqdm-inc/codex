use serde_json::json;
use topcoat::context::Cx;

use super::is_visible_activity;
use super::work_digest;

#[tokio::test]
async fn renders_active_work_digest_with_compact_rows() {
    let cx = Cx::default();
    let items = json!([
        {
            "id": "command-1",
            "type": "commandExecution",
            "command": "cargo check -p typeduck-codex-web",
            "aggregatedOutput": "Checking typeduck-codex-web",
            "status": "completed"
        },
        {
            "id": "search-1",
            "type": "webSearch",
            "status": "inProgress"
        }
    ]);
    let view = work_digest(
        &cx,
        "turn-1",
        "inProgress",
        items.as_array().expect("items"),
    )
    .await
    .expect("render work digest");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn completed_work_digest_collapses_unless_an_action_failed() {
    let cx = Cx::default();
    let items = json!([
        {
            "id": "command-1",
            "type": "commandExecution",
            "command": "cargo test",
            "status": "completed"
        },
        {
            "id": "command-2",
            "type": "commandExecution",
            "command": "cargo publish",
            "status": "failed",
            "aggregatedOutput": "network unavailable"
        }
    ]);
    let view = work_digest(&cx, "turn-1", "completed", items.as_array().expect("items"))
        .await
        .expect("render work digest");

    insta::assert_snapshot!(view.render(&cx));
}

#[test]
fn empty_reasoning_is_not_visible_activity() {
    assert!(!is_visible_activity(&json!({
        "id": "reasoning-empty",
        "type": "reasoning",
        "summary": [],
        "content": [""]
    })));
    assert!(is_visible_activity(&json!({
        "id": "reasoning-visible",
        "type": "reasoning",
        "summary": ["Inspecting the current implementation"],
        "content": []
    })));
}

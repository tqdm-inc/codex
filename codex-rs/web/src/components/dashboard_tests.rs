use pretty_assertions::assert_eq;
use serde_json::json;
use topcoat::context::Cx;

use super::DashboardDocumentData;
use super::dashboard_document;
use super::session_groups_at;
use crate::dashboard::Project;

#[tokio::test]
async fn renders_dashboard_with_projects_sessions_and_onboarding() {
    let cx = Cx::default();
    let threads = json!([
        {
            "id": "thread-1",
            "cwd": "/srv/work/codex",
            "preview": "Improve the web dashboard",
            "recencyAt": 1720000000,
            "status": { "type": "active" }
        },
        {
            "id": "thread-2",
            "cwd": "/srv/work/typeduck",
            "preview": "Package the standalone binary",
            "recencyAt": 1710000000,
            "status": { "type": "idle" }
        }
    ]);
    let projects = vec![Project {
        cwd: "/srv/work/codex".to_string(),
        name: "codex".to_string(),
        latest_thread: threads[0].clone(),
        session_count: 3,
    }];
    let view = dashboard_document(
        &cx,
        DashboardDocumentData {
            base_path: "/i/instance",
            projects: &projects,
            threads: threads.as_array().expect("threads"),
            account: None,
            truncated: false,
            profile_label: "Primary",
            proxy_active: true,
            mcp_servers: &[],
            accounts: &[],
            active_account: "primary",
        },
    )
    .await
    .expect("render dashboard");

    insta::assert_snapshot!(view.render(&cx));
}

#[test]
fn groups_sessions_by_recent_activity() {
    let now = 2_000_000_000;
    let threads = json!([
        { "id": "today", "recencyAt": now - 30 },
        { "id": "yesterday", "recencyAt": now - 100_000 },
        { "id": "week", "recencyAt": now - 300_000 },
        { "id": "month", "recencyAt": now - 1_000_000 },
        { "id": "older", "recencyAt": now - 4_000_000 }
    ]);

    let groups = session_groups_at(threads.as_array().expect("threads"), now);
    let actual = groups
        .iter()
        .map(|group| (group.label, group.threads[0]["id"].as_str().expect("id")))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            ("Today", "today"),
            ("Yesterday", "yesterday"),
            ("Previous 7 days", "week"),
            ("Previous 30 days", "month"),
            ("Older", "older"),
        ]
    );
}

use pretty_assertions::assert_eq;
use serde_json::json;

use super::projects_from_threads;

#[test]
fn projects_follow_thread_recency_and_count_matching_cwds() {
    let threads = vec![
        json!({ "id": "new", "cwd": "/workspace/alpha", "preview": "latest" }),
        json!({ "id": "beta", "cwd": "/workspace/beta", "preview": "other" }),
        json!({ "id": "old", "cwd": "/workspace/alpha", "preview": "older" }),
    ];

    let projects = projects_from_threads(&threads);

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "alpha");
    assert_eq!(projects[0].session_count, 2);
    assert_eq!(projects[0].latest_thread, threads[0]);
    assert_eq!(projects[1].name, "beta");
}

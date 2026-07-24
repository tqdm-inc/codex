use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use serde_json::json;

use crate::bridge::AppServerBridge;

const THREAD_PAGE_SIZE: usize = 100;
const MAX_RECENT_THREADS: usize = 500;

#[derive(Clone, Debug)]
pub(crate) struct Project {
    pub(crate) cwd: String,
    pub(crate) name: String,
    pub(crate) latest_thread: Value,
    pub(crate) session_count: usize,
}

pub(crate) struct DashboardData {
    pub(crate) threads: Vec<Value>,
    pub(crate) projects: Vec<Project>,
    pub(crate) account: Option<Value>,
    pub(crate) truncated: bool,
    pub(crate) mcp_servers: Vec<Value>,
}

pub(crate) async fn load(bridge: &Arc<AppServerBridge>) -> DashboardData {
    let (threads, account, mcp_servers) = tokio::join!(
        load_threads(bridge),
        bridge.request("account/read", json!({})),
        bridge.request(
            "mcpServerStatus/list",
            json!({ "limit": 100, "detail": "toolsAndAuthOnly" }),
        )
    );
    let (threads, truncated) = threads.unwrap_or_default();
    DashboardData {
        projects: projects_from_threads(&threads),
        threads,
        account: account
            .ok()
            .and_then(|result| result.get("account").cloned())
            .filter(|account| !account.is_null()),
        truncated,
        mcp_servers: mcp_servers
            .ok()
            .and_then(|result| result.get("data").and_then(Value::as_array).cloned())
            .unwrap_or_default(),
    }
}

async fn load_threads(bridge: &Arc<AppServerBridge>) -> std::io::Result<(Vec<Value>, bool)> {
    let mut threads = Vec::new();
    let mut cursor = None;
    loop {
        let page = bridge
            .request(
                "thread/list",
                json!({
                    "cursor": cursor,
                    "limit": THREAD_PAGE_SIZE,
                    "sortKey": "recency_at",
                    "sortDirection": "desc",
                    "modelProviders": [],
                }),
            )
            .await?;
        let data = page
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        threads.extend(data);
        cursor = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if threads.len() >= MAX_RECENT_THREADS {
            threads.truncate(MAX_RECENT_THREADS);
            return Ok((threads, cursor.is_some()));
        }
        if cursor.is_none() {
            return Ok((threads, false));
        }
    }
}

fn projects_from_threads(threads: &[Value]) -> Vec<Project> {
    let mut projects: Vec<Project> = Vec::new();
    let mut indices = HashMap::<PathBuf, usize>::new();
    for thread in threads {
        let Some(cwd) = thread.get("cwd").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(cwd);
        let identity =
            codex_utils_path::normalize_for_path_comparison(&path).unwrap_or_else(|_| path.clone());
        if let Some(index) = indices.get(&identity).copied() {
            projects[index].session_count += 1;
            continue;
        }
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.to_string());
        indices.insert(identity, projects.len());
        projects.push(Project {
            cwd: cwd.to_string(),
            name,
            latest_thread: thread.clone(),
            session_count: 1,
        });
    }
    projects
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;

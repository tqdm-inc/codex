use std::net::SocketAddr;

use pretty_assertions::assert_eq;

use super::AccountAuthRequest;
use super::AccountProfilesRequest;
use super::mcp_callback_url;
use super::page_workspace_cwd;
use super::thread_is_active;
use super::valid_callback_id;

#[test]
fn oauth_callback_id_accepts_only_bound_base64url_identifier() {
    assert!(valid_callback_id("Abc_123-xYz9"));
    assert!(!valid_callback_id("short"));
    assert!(!valid_callback_id("abcdefghijkl%0d%0a"));
    assert!(!valid_callback_id("abcdefghijk\n"));
}

#[test]
fn mcp_callback_prefers_tailnet_ipv4() {
    let addresses = [
        "127.0.0.1:34261"
            .parse::<SocketAddr>()
            .expect("IPv4 loopback"),
        "[fd7a:115c:a1e0::1]:34261"
            .parse::<SocketAddr>()
            .expect("tailnet IPv6"),
        "100.71.13.123:34261"
            .parse::<SocketAddr>()
            .expect("tailnet IPv4"),
    ];

    assert_eq!(
        mcp_callback_url(&addresses),
        Some("http://100.71.13.123:34261/oauth/callback".to_string())
    );
}

#[test]
fn mcp_callback_stays_local_without_tailnet() {
    let addresses = [
        "127.0.0.1:34261"
            .parse::<SocketAddr>()
            .expect("IPv4 loopback"),
        "[::1]:34261".parse::<SocketAddr>().expect("IPv6 loopback"),
    ];

    assert_eq!(mcp_callback_url(&addresses), None);
}

#[test]
fn page_workspace_prefers_the_active_thread_directory() {
    let thread = serde_json::json!({ "cwd": "/projects/thread" });

    assert_eq!(
        page_workspace_cwd(Some(&thread), Some("/projects/selected"), "/daemon"),
        "/projects/thread"
    );
    assert_eq!(
        page_workspace_cwd(None, Some("/projects/selected"), "/daemon"),
        "/projects/selected"
    );
    assert_eq!(page_workspace_cwd(None, None, "/daemon"), "/daemon");
}

#[test]
fn account_switching_recognizes_active_thread_read_responses() {
    assert!(thread_is_active(&serde_json::json!({
        "thread": { "status": { "type": "active", "activeFlags": [] } }
    })));
    assert!(!thread_is_active(&serde_json::json!({
        "thread": { "status": { "type": "idle" } }
    })));
}

#[test]
fn account_profile_requests_use_browser_facing_camel_case_fields() {
    let request: AccountProfilesRequest = serde_json::from_value(serde_json::json!({
        "action": "add",
        "name": "personal",
        "label": "Personal",
        "proxy": null,
        "useSshTunnel": false
    }))
    .expect("deserialize profile request");

    assert!(matches!(
        request,
        AccountProfilesRequest::Add {
            name,
            use_ssh_tunnel: false,
            ..
        } if name == "personal"
    ));

    let request: AccountAuthRequest = serde_json::from_value(serde_json::json!({
        "action": "apiKey",
        "name": "personal",
        "apiKey": "sk-private"
    }))
    .expect("deserialize account auth request");
    assert!(matches!(
        request,
        AccountAuthRequest::ApiKey { name, api_key }
            if name == "personal" && api_key == "sk-private"
    ));
}

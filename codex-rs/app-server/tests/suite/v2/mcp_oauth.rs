use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_sequence_unchecked;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::McpServerOauthLogoutResponse;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;

#[tokio::test]
async fn mcp_oauth_logout_reports_when_no_credentials_are_stored() -> Result<()> {
    let responses = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &responses.uri(),
        &BTreeMap::new(),
        /*auto_compact_limit*/ 1024,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "compact",
    )?;
    let config_path = codex_home.path().join("config.toml");
    let mut config = std::fs::read_to_string(&config_path)?;
    config.push_str(
        r#"
mcp_oauth_credentials_store = "file"

[mcp_servers.docs]
url = "https://example.test/mcp"
"#,
    );
    std::fs::write(config_path, config)?;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app_server.initialize()).await??;

    let request_id = app_server
        .send_raw_request(
            "mcpServer/oauth/logout",
            Some(json!({ "name": "docs", "threadId": null })),
        )
        .await?;
    let response = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let response: McpServerOauthLogoutResponse = to_response(response)?;

    assert_eq!(response, McpServerOauthLogoutResponse { removed: false });
    Ok(())
}

#[tokio::test]
async fn mcp_oauth_logout_rejects_stdio_servers() -> Result<()> {
    let responses = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &responses.uri(),
        &BTreeMap::new(),
        /*auto_compact_limit*/ 1024,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "compact",
    )?;
    let config_path = codex_home.path().join("config.toml");
    let mut config = std::fs::read_to_string(&config_path)?;
    config.push_str(
        r#"
[mcp_servers.local]
command = "example-mcp"
"#,
    );
    std::fs::write(config_path, config)?;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app_server.initialize()).await??;

    let request_id = app_server
        .send_raw_request(
            "mcpServer/oauth/logout",
            Some(json!({ "name": "local", "threadId": null })),
        )
        .await?;
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert_eq!(
        error.error.message,
        "OAuth logout is only supported for streamable HTTP servers."
    );
    Ok(())
}

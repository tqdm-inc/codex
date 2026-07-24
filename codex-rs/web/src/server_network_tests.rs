use super::SshTunnelRequest;

#[test]
fn ssh_tunnel_requests_use_browser_facing_camel_case_fields() {
    let request: SshTunnelRequest = serde_json::from_value(serde_json::json!({
        "action": "apply",
        "config": {
            "destination": "codex@example.com",
            "identityFile": "/keys/id_ed25519",
            "sshPort": 2222,
            "localPort": 1080
        }
    }))
    .expect("deserialize SSH tunnel request");

    assert!(matches!(
        request,
        SshTunnelRequest::Apply { config: Some(_) }
    ));
}

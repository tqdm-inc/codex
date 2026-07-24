use std::fs;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::AccountProfile;
use super::AccountRole;
use super::DaemonConfig;
use super::SshTunnelConfig;

#[test]
fn loads_ordered_accounts_and_tunnel() {
    let temp = TempDir::new().expect("temp dir");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let identity = temp.path().join("id_ed25519");
    let config = temp.path().join("daemon.toml");
    fs::write(
        &config,
        format!(
            r#"
port = 36915
mcp_oauth_callback_url = "http://127.0.0.1:36915/oauth/callback"

[[accounts]]
name = "primary"
label = "Primary"
codex_home = {first:?}
proxy = "http://proxy.example:8080"

[[accounts]]
name = "backup"
codex_home = {second:?}
use_ssh_tunnel = true

[ssh_tunnel]
destination = "codex@example.com"
identity_file = {identity:?}
ssh_port = 2222
local_port = 1080
"#
        ),
    )
    .expect("write config");

    let loaded = DaemonConfig::load(Some(&config)).expect("load config");

    assert_eq!(loaded.port, Some(36915));
    assert_eq!(
        loaded.mcp_oauth_callback_url.as_deref(),
        Some("http://127.0.0.1:36915/oauth/callback")
    );
    assert_eq!(loaded.accounts.len(), 2);
    assert_eq!(loaded.accounts[0].name, "primary");
    assert_eq!(loaded.accounts[1].label, "backup");
    assert!(loaded.accounts[1].use_ssh_tunnel);
    assert_eq!(loaded.ssh_tunnel.expect("tunnel").local_port, Some(1080));
}

#[test]
fn rejects_invalid_persistent_callback_url() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("daemon.toml");
    fs::write(
        &config,
        r#"mcp_oauth_callback_url = "https://example.com/not-the-callback""#,
    )
    .expect("write config");

    let error = DaemonConfig::load(Some(&config)).expect_err("invalid callback URL");

    assert!(error.to_string().contains("must end at /oauth/callback"));
}

#[test]
fn rejects_duplicate_accounts_and_unsupported_proxies() {
    let temp = TempDir::new().expect("temp dir");
    let home = temp.path().join("home");
    let config = temp.path().join("daemon.toml");
    fs::write(
        &config,
        format!(
            r#"
[[accounts]]
name = "same"
codex_home = {home:?}
proxy = "ftp://proxy.example"

[[accounts]]
name = "same"
codex_home = {home:?}
"#
        ),
    )
    .expect("write config");

    let error = DaemonConfig::load(Some(&config)).expect_err("invalid config");

    assert!(error.to_string().contains("proxy URL scheme"));
}

#[test]
fn saves_account_profile_changes_without_losing_daemon_settings() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("daemon.toml");
    fs::write(
        &config,
        "port = 36915\nmcp_oauth_callback_url = \"http://127.0.0.1:36915/oauth/callback\"\n",
    )
    .expect("write config");
    let mut loaded = DaemonConfig::load(Some(&config)).expect("load config");
    let credential_home = loaded.codex_home.clone();

    loaded
        .save_accounts(vec![AccountProfile {
            name: "personal".to_string(),
            label: "Personal".to_string(),
            role: AccountRole::Primary,
            credential_home,
            proxy: Some("socks5h://127.0.0.1:1080".to_string()),
            use_ssh_tunnel: false,
        }])
        .expect("save accounts");
    let reloaded = DaemonConfig::load(Some(&config)).expect("reload config");

    assert_eq!(reloaded.port, Some(36915));
    assert_eq!(reloaded.accounts, loaded.accounts);
    assert_eq!(
        reloaded.mcp_oauth_callback_url.as_deref(),
        Some("http://127.0.0.1:36915/oauth/callback")
    );
}

#[test]
fn saves_tunnel_changes_and_protects_assigned_accounts() {
    let temp = TempDir::new().expect("temp dir");
    let config = temp.path().join("daemon.toml");
    fs::write(&config, "").expect("write config");
    let mut loaded = DaemonConfig::load(Some(&config)).expect("load config");
    let tunnel = SshTunnelConfig::validated(
        "codex@example.com".to_string(),
        Some(temp.path().join("id_ed25519")),
        Some(2222),
        Some(1080),
    )
    .expect("valid tunnel");

    loaded
        .save_ssh_tunnel(Some(tunnel.clone()))
        .expect("save tunnel");
    let mut accounts = loaded.accounts.clone();
    accounts[0].use_ssh_tunnel = true;
    loaded.save_accounts(accounts).expect("assign tunnel");

    assert_eq!(
        DaemonConfig::load(Some(&config))
            .expect("reload config")
            .ssh_tunnel,
        Some(tunnel)
    );
    assert!(
        loaded
            .save_ssh_tunnel(None)
            .expect_err("assigned tunnel cannot be removed")
            .to_string()
            .contains("enables the SSH tunnel")
    );
}

#[test]
fn imports_legacy_fallback_credentials_without_moving_source_data() {
    let temp = TempDir::new().expect("temp dir");
    let primary = temp.path().join("primary");
    let fallback = temp.path().join("fallback");
    fs::create_dir_all(&fallback).expect("fallback home");
    fs::write(
        fallback.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test"}"#,
    )
    .expect("fallback auth");
    fs::write(fallback.join("session.jsonl"), "preserve me").expect("legacy session");
    let config = temp.path().join("daemon.toml");
    fs::write(
        &config,
        format!(
            r#"
[[accounts]]
name = "primary"
codex_home = {primary:?}

[[accounts]]
name = "backup"
codex_home = {fallback:?}
"#
        ),
    )
    .expect("write config");

    let loaded = DaemonConfig::load(Some(&config)).expect("load config");

    assert_eq!(loaded.accounts[0].role, AccountRole::Primary);
    assert_eq!(loaded.accounts[1].role, AccountRole::Fallback);
    assert_eq!(
        fs::read_to_string(loaded.accounts[1].credential_home.join("auth.json"))
            .expect("imported auth"),
        r#"{"OPENAI_API_KEY":"sk-test"}"#
    );
    assert_eq!(
        fs::read_to_string(fallback.join("session.jsonl")).expect("legacy session"),
        "preserve me"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(&loaded.accounts[1].credential_home)
                .expect("credential metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

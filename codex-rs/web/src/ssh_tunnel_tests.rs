use std::ffi::OsString;
use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::ssh_args;
use crate::daemon_config::SshTunnelConfig;

#[test]
fn builds_constrained_dynamic_forward_command() {
    let args = ssh_args(
        &SshTunnelConfig {
            destination: "codex@example.com".to_string(),
            identity_file: Some(PathBuf::from("/keys/id_ed25519")),
            ssh_port: Some(2222),
            local_port: None,
        },
        1080,
    );

    assert_eq!(
        args,
        [
            "-N",
            "-D",
            "127.0.0.1:1080",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=30",
            "-p",
            "2222",
            "-i",
            "/keys/id_ed25519",
            "codex@example.com",
        ]
        .map(OsString::from)
    );
}

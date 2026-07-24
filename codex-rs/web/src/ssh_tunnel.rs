use std::ffi::OsString;
use std::io::Error;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::Instant;
use tokio::time::sleep;

use crate::daemon_config::SshTunnelConfig;

const START_TIMEOUT: Duration = Duration::from_secs(12);

pub(crate) struct ManagedSshTunnel {
    child: Child,
    proxy_url: String,
}

impl ManagedSshTunnel {
    pub(crate) async fn start(config: &SshTunnelConfig) -> std::io::Result<Self> {
        let local_port = match config.local_port {
            Some(port) => port,
            None => {
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
                listener.local_addr()?.port()
            }
        };
        let args = ssh_args(config, local_port);
        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                Error::new(
                    error.kind(),
                    format!("failed to start the managed SSH tunnel: {error}"),
                )
            })?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Some(status) = child.try_wait()? {
                return Err(Error::other(format!(
                    "managed SSH tunnel exited before it was ready: {status}"
                )));
            }
            if TcpStream::connect((Ipv4Addr::LOCALHOST, local_port))
                .await
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(Error::new(
                    ErrorKind::TimedOut,
                    "managed SSH tunnel did not become ready within 12 seconds",
                ));
            }
            sleep(Duration::from_millis(100)).await;
        }
        Ok(Self {
            child,
            proxy_url: format!("socks5h://127.0.0.1:{local_port}"),
        })
    }

    pub(crate) fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    pub(crate) fn is_running(&mut self) -> std::io::Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }

    pub(crate) async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

fn ssh_args(config: &SshTunnelConfig, local_port: u16) -> Vec<OsString> {
    let mut args = vec![
        "-N".into(),
        "-D".into(),
        format!("127.0.0.1:{local_port}").into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ServerAliveInterval=30".into(),
    ];
    if let Some(port) = config.ssh_port {
        args.extend(["-p".into(), port.to_string().into()]);
    }
    if let Some(identity_file) = &config.identity_file {
        args.extend(["-i".into(), identity_file.as_os_str().to_owned()]);
    }
    args.push(config.destination.clone().into());
    args
}

#[cfg(test)]
#[path = "ssh_tunnel_tests.rs"]
mod tests;

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::time::timeout;

pub(crate) async fn bind_listeners(
    requested_port: u16,
) -> std::io::Result<(Vec<TcpListener>, u16)> {
    let primary = TcpListener::bind((Ipv4Addr::LOCALHOST, requested_port)).await?;
    let port = primary.local_addr()?.port();
    let mut listeners = vec![primary];

    for address in detect_tailnet_ips()
        .await
        .into_iter()
        .chain([IpAddr::V6(Ipv6Addr::LOCALHOST)])
    {
        if let Ok(listener) = TcpListener::bind(SocketAddr::new(address, port)).await {
            listeners.push(listener);
        }
    }
    Ok((listeners, port))
}

async fn detect_tailnet_ips() -> Vec<IpAddr> {
    let mut addresses = Vec::new();
    for family in ["-4", "-6"] {
        let mut command = Command::new("tailscale");
        command.args(["ip", family]).kill_on_drop(true);
        let Ok(Ok(output)) = timeout(Duration::from_millis(750), command.output()).await else {
            continue;
        };
        if output.status.success() {
            addresses.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| line.trim().parse::<IpAddr>().ok()),
            );
        }
    }
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

pub(crate) fn authority(address: SocketAddr) -> String {
    match address.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", address.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", address.port()),
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;

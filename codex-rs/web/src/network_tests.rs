use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;

use pretty_assertions::assert_eq;

use super::authority;

#[test]
fn formats_socket_authorities() {
    assert_eq!(
        authority(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 34261)),
        "127.0.0.1:34261"
    );
    assert_eq!(
        authority(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 34261)),
        "[::1]:34261"
    );
}

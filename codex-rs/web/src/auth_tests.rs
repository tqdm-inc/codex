use super::*;

#[test]
fn session_cookie_round_trips_without_exposing_bootstrap_token() {
    let key = [7_u8; 32];
    let auth = WebAuth {
        key,
        bootstrap_token: URL_SAFE_NO_PAD.encode(key),
    };

    let cookie = auth.session_cookie("/i/instance").expect("session cookie");
    let request_cookie = cookie.split(';').next().expect("cookie pair");

    assert!(auth.authorize_cookie_header(Some(request_cookie)));
    assert!(!cookie.contains(auth.bootstrap_token()));
    assert!(cookie.contains("Path=/i/instance;"));
}

#[test]
fn changed_key_invalidates_existing_session_cookie() {
    let auth = WebAuth {
        key: [7_u8; 32],
        bootstrap_token: URL_SAFE_NO_PAD.encode([7_u8; 32]),
    };
    let changed = WebAuth {
        key: [8_u8; 32],
        bootstrap_token: URL_SAFE_NO_PAD.encode([8_u8; 32]),
    };
    let cookie = auth.session_cookie("/i/instance").expect("session cookie");
    let request_cookie = cookie.split(';').next().expect("cookie pair");

    assert!(!changed.authorize_cookie_header(Some(request_cookie)));
}

#[test]
fn bootstrap_token_comparison_rejects_invalid_values() {
    let key = [9_u8; 32];
    let auth = WebAuth {
        key,
        bootstrap_token: URL_SAFE_NO_PAD.encode(key),
    };

    assert!(auth.verify_bootstrap(auth.bootstrap_token()));
    assert!(!auth.verify_bootstrap("not-a-token"));
}

use std::path::PathBuf;

use serde_json::json;
use topcoat::context::Cx;
use topcoat::view::view;

use super::AccountProfile;
use super::AccountRole;
use super::goal_modal;
use super::settings_modal;

#[tokio::test]
async fn renders_goal_editor_with_every_lifecycle_status() {
    let cx = &Cx::default();
    let view = view! { cx => goal_modal() }.expect("render goal modal");

    insta::assert_snapshot!(view.render(cx));
}

#[tokio::test]
async fn renders_account_and_mcp_management() {
    let cx = &Cx::default();
    let accounts = [AccountProfile {
        name: "primary".to_string(),
        label: "Work".to_string(),
        role: AccountRole::Primary,
        credential_home: PathBuf::from("/home/codex/.codex"),
        proxy: None,
        use_ssh_tunnel: false,
    }];
    let servers = json!([{
        "name": "jira",
        "authStatus": "notAuthenticated"
    }]);
    let view = view! { cx =>
        settings_modal(
            mcp_servers: servers.as_array().expect("servers"),
            accounts: &accounts,
            active_account: "primary",
        )
    }
    .expect("render settings modal");

    insta::assert_snapshot!(view.render(cx));
}

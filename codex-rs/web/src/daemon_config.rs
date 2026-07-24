use std::collections::HashSet;
use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use url::Url;

const CONFIG_FILE_NAME: &str = "daemon.toml";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AccountProfile {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) role: AccountRole,
    #[serde(skip)]
    pub(crate) credential_home: PathBuf,
    pub(crate) proxy: Option<String>,
    pub(crate) use_ssh_tunnel: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountRole {
    #[default]
    Fallback,
    Primary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SshTunnelConfig {
    pub(crate) destination: String,
    pub(crate) identity_file: Option<PathBuf>,
    pub(crate) ssh_port: Option<u16>,
    pub(crate) local_port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DaemonConfig {
    #[serde(skip)]
    pub(crate) source_path: PathBuf,
    #[serde(skip)]
    pub(crate) codex_home: PathBuf,
    pub(crate) port: Option<u16>,
    pub(crate) mcp_oauth_callback_url: Option<String>,
    pub(crate) accounts: Vec<AccountProfile>,
    pub(crate) ssh_tunnel: Option<SshTunnelConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    port: Option<u16>,
    mcp_oauth_callback_url: Option<String>,
    #[serde(default)]
    accounts: Vec<AccountFile>,
    ssh_tunnel: Option<SshTunnelFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountFile {
    name: String,
    label: Option<String>,
    role: Option<AccountRole>,
    codex_home: Option<PathBuf>,
    proxy: Option<String>,
    #[serde(default)]
    use_ssh_tunnel: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SshTunnelFile {
    destination: String,
    identity_file: Option<PathBuf>,
    ssh_port: Option<u16>,
    local_port: Option<u16>,
}

impl DaemonConfig {
    pub(crate) fn load(explicit_path: Option<&Path>) -> std::io::Result<Self> {
        let default_home = codex_utils_home_dir::find_codex_home()?.to_path_buf();
        let default_path = default_home
            .join("typeduck-codex-web")
            .join(CONFIG_FILE_NAME);
        let path = explicit_path.unwrap_or(&default_path);
        if !path.exists() && explicit_path.is_none() {
            return Ok(Self::single(default_home, default_path));
        }
        let encoded = std::fs::read_to_string(path).map_err(|error| {
            Error::new(
                error.kind(),
                format!("failed to read daemon config {}: {error}", path.display()),
            )
        })?;
        let parsed: ConfigFile = toml::from_str(&encoded).map_err(|error| {
            Error::new(
                ErrorKind::InvalidData,
                format!("invalid daemon config {}: {error}", path.display()),
            )
        })?;
        parsed.validate(default_home, path.to_path_buf())
    }

    fn single(codex_home: PathBuf, source_path: PathBuf) -> Self {
        Self {
            source_path,
            codex_home: codex_home.clone(),
            port: None,
            mcp_oauth_callback_url: None,
            accounts: vec![AccountProfile {
                name: "default".to_string(),
                label: "Default".to_string(),
                role: AccountRole::Primary,
                credential_home: codex_home,
                proxy: None,
                use_ssh_tunnel: false,
            }],
            ssh_tunnel: None,
        }
    }

    pub(crate) fn save_accounts(&mut self, accounts: Vec<AccountProfile>) -> std::io::Result<()> {
        validate_accounts(&accounts, self.ssh_tunnel.as_ref())?;
        let previous = std::mem::replace(&mut self.accounts, accounts);
        let encoded = toml::to_string_pretty(self).map_err(|error| {
            Error::new(
                ErrorKind::InvalidData,
                format!("could not encode daemon config: {error}"),
            )
        })?;
        if let Some(parent) = self.source_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Err(error) = std::fs::write(&self.source_path, encoded) {
            self.accounts = previous;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn save_ssh_tunnel(
        &mut self,
        ssh_tunnel: Option<SshTunnelConfig>,
    ) -> std::io::Result<()> {
        validate_accounts(&self.accounts, ssh_tunnel.as_ref())?;
        let previous = std::mem::replace(&mut self.ssh_tunnel, ssh_tunnel);
        if let Err(error) = self.write() {
            self.ssh_tunnel = previous;
            return Err(error);
        }
        Ok(())
    }

    fn write(&self) -> std::io::Result<()> {
        let encoded = toml::to_string_pretty(self).map_err(|error| {
            Error::new(
                ErrorKind::InvalidData,
                format!("could not encode daemon config: {error}"),
            )
        })?;
        if let Some(parent) = self.source_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.source_path, encoded)
    }

    pub(crate) fn credential_home(&self, name: &str) -> PathBuf {
        account_vault(&self.source_path, name)
    }

    pub(crate) fn recovery_state_path(&self) -> PathBuf {
        self.source_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join("recovery.json")
    }
}

impl SshTunnelConfig {
    pub(crate) fn validated(
        destination: String,
        identity_file: Option<PathBuf>,
        ssh_port: Option<u16>,
        local_port: Option<u16>,
    ) -> std::io::Result<Self> {
        SshTunnelFile {
            destination,
            identity_file,
            ssh_port,
            local_port,
        }
        .validate()
    }
}

impl ConfigFile {
    fn validate(
        self,
        default_home: PathBuf,
        source_path: PathBuf,
    ) -> std::io::Result<DaemonConfig> {
        if self.port == Some(0) {
            return Err(invalid("daemon port must be greater than zero"));
        }
        if let Some(callback_url) = self.mcp_oauth_callback_url.as_deref() {
            validate_callback_url(callback_url)?;
        }
        if self.accounts.is_empty() {
            let mut config = DaemonConfig::single(default_home, source_path);
            config.port = self.port;
            config.mcp_oauth_callback_url = self.mcp_oauth_callback_url;
            return Ok(config);
        }
        let mut accounts = Vec::with_capacity(self.accounts.len());
        for (index, account) in self.accounts.into_iter().enumerate() {
            let role = account.role.unwrap_or(if index == 0 {
                AccountRole::Primary
            } else {
                AccountRole::Fallback
            });
            let credential_home = if role == AccountRole::Primary {
                default_home.clone()
            } else {
                account_vault(&source_path, &account.name)
            };
            if role == AccountRole::Fallback
                && let Some(legacy_home) = account.codex_home.as_deref()
            {
                import_legacy_credentials(legacy_home, &credential_home)?;
            }
            accounts.push(AccountProfile {
                label: account.label.unwrap_or_else(|| account.name.clone()),
                name: account.name,
                role,
                credential_home,
                proxy: account.proxy,
                use_ssh_tunnel: account.use_ssh_tunnel,
            });
        }
        let ssh_tunnel = self.ssh_tunnel.map(SshTunnelFile::validate).transpose()?;
        validate_accounts(&accounts, ssh_tunnel.as_ref())?;
        Ok(DaemonConfig {
            source_path,
            codex_home: default_home,
            port: self.port,
            mcp_oauth_callback_url: self.mcp_oauth_callback_url,
            accounts,
            ssh_tunnel,
        })
    }
}

fn validate_accounts(
    accounts: &[AccountProfile],
    ssh_tunnel: Option<&SshTunnelConfig>,
) -> std::io::Result<()> {
    if accounts.is_empty() {
        return Err(invalid("at least one account is required"));
    }
    let mut names = HashSet::new();
    let mut primary_count = 0;
    for account in accounts {
        if !valid_name(&account.name) {
            return Err(invalid(
                "account names may contain only letters, numbers, '-' and '_'",
            ));
        }
        if !names.insert(account.name.clone()) {
            return Err(invalid(format!(
                "duplicate account name {:?}",
                account.name
            )));
        }
        if !account.credential_home.is_absolute() {
            return Err(invalid(format!(
                "credential home for account {:?} must be absolute",
                account.name
            )));
        }
        if account.role == AccountRole::Primary {
            primary_count += 1;
        }
        if let Some(proxy) = account.proxy.as_deref() {
            validate_proxy(proxy)?;
        }
        if account.proxy.is_some() && account.use_ssh_tunnel {
            return Err(invalid(format!(
                "account {:?} cannot set both proxy and use_ssh_tunnel",
                account.name
            )));
        }
        if account.use_ssh_tunnel && ssh_tunnel.is_none() {
            return Err(invalid(format!(
                "account {:?} enables the SSH tunnel but [ssh_tunnel] is missing",
                account.name
            )));
        }
    }
    if primary_count != 1
        || accounts.first().map(|account| account.role) != Some(AccountRole::Primary)
    {
        return Err(invalid(
            "accounts must contain exactly one primary profile in the first position",
        ));
    }
    Ok(())
}

fn account_vault(source_path: &Path, name: &str) -> PathBuf {
    source_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("accounts")
        .join(name)
}

fn import_legacy_credentials(legacy_home: &Path, credential_home: &Path) -> std::io::Result<()> {
    let source = legacy_home.join("auth.json");
    let destination = credential_home.join("auth.json");
    if !source.is_file() || destination.exists() {
        return Ok(());
    }
    prepare_credential_home(credential_home)?;
    std::fs::copy(source, &destination)?;
    restrict_credentials(credential_home, &destination)
}

pub(crate) fn prepare_credential_home(directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    restrict_credential_directory(directory)
}

#[cfg(unix)]
fn restrict_credential_directory(directory: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_credential_directory(_directory: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_credentials(directory: &Path, file: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    restrict_credential_directory(directory)?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_credentials(_directory: &Path, _file: &Path) -> std::io::Result<()> {
    Ok(())
}

impl SshTunnelFile {
    fn validate(self) -> std::io::Result<SshTunnelConfig> {
        if self.destination.is_empty()
            || self.destination.starts_with('-')
            || self.destination.chars().any(char::is_whitespace)
        {
            return Err(invalid(
                "ssh_tunnel.destination must be one SSH destination argument",
            ));
        }
        if self.ssh_port == Some(0) || self.local_port == Some(0) {
            return Err(invalid("SSH ports must be greater than zero"));
        }
        if let Some(path) = &self.identity_file
            && !path.is_absolute()
        {
            return Err(invalid("ssh_tunnel.identity_file must be absolute"));
        }
        Ok(SshTunnelConfig {
            destination: self.destination,
            identity_file: self.identity_file,
            ssh_port: self.ssh_port,
            local_port: self.local_port,
        })
    }
}

fn validate_proxy(proxy: &str) -> std::io::Result<()> {
    let parsed =
        Url::parse(proxy).map_err(|error| invalid(format!("invalid proxy URL: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err(invalid(
            "proxy URL scheme must be http, https, socks5, or socks5h",
        ));
    }
    if parsed.host().is_none() {
        return Err(invalid("proxy URL must include a host"));
    }
    Ok(())
}

fn validate_callback_url(callback_url: &str) -> std::io::Result<()> {
    let parsed = Url::parse(callback_url)
        .map_err(|error| invalid(format!("invalid MCP OAuth callback URL: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(invalid("MCP OAuth callback URL must use http or https"));
    }
    if parsed.host().is_none() || parsed.port_or_known_default().is_none() {
        return Err(invalid(
            "MCP OAuth callback URL must include a host and port",
        ));
    }
    if parsed.path() != "/oauth/callback" || parsed.query().is_some() || parsed.fragment().is_some()
    {
        return Err(invalid(
            "MCP OAuth callback URL must end at /oauth/callback without a query or fragment",
        ));
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
#[path = "daemon_config_tests.rs"]
mod tests;

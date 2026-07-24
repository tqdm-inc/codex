use std::fs;
use std::fs::OpenOptions;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use constant_time_eq::constant_time_eq_32;
use hmac::Hmac;
use hmac::Mac;
use rand::RngCore;
use sha2::Sha256;

const TOKEN_FILE_NAME: &str = "token";
const SESSION_COOKIE_NAME: &str = "typeduck_codex_web";
const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

type HmacSha256 = Hmac<Sha256>;

pub(crate) struct WebAuth {
    key: [u8; 32],
    bootstrap_token: String,
}

impl WebAuth {
    pub(crate) fn load(reset: bool) -> std::io::Result<Self> {
        let codex_home = codex_utils_home_dir::find_codex_home()?;
        let auth_dir = codex_home.join("typeduck-codex-web");
        fs::create_dir_all(&auth_dir)?;
        let token_path = auth_dir.join(TOKEN_FILE_NAME);
        let token = if reset || !token_path.exists() {
            replace_token(&token_path)?
        } else {
            read_token(&token_path)?
        };
        let key = decode_token(&token)?;
        Ok(Self {
            key,
            bootstrap_token: token,
        })
    }

    pub(crate) fn bootstrap_token(&self) -> &str {
        &self.bootstrap_token
    }

    pub(crate) fn verify_bootstrap(&self, candidate: &str) -> bool {
        decode_token(candidate).is_ok_and(|candidate| constant_time_eq_32(&self.key, &candidate))
    }

    pub(crate) fn session_cookie(&self, path: &str) -> std::io::Result<String> {
        let expires_at = unix_time()?.saturating_add(SESSION_LIFETIME.as_secs());
        let nonce = random_token();
        let payload = format!("{expires_at}.{nonce}");
        let signature = self.sign(&payload)?;
        Ok(format!(
            "{SESSION_COOKIE_NAME}={payload}.{signature}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={}",
            SESSION_LIFETIME.as_secs()
        ))
    }

    pub(crate) fn authorize_cookie_header(&self, cookie_header: Option<&str>) -> bool {
        let Some(cookie) = cookie_header.and_then(session_cookie_value) else {
            return false;
        };
        let Some((payload, signature)) = cookie.rsplit_once('.') else {
            return false;
        };
        let Some((expires_at, _nonce)) = payload.split_once('.') else {
            return false;
        };
        let Ok(expires_at) = expires_at.parse::<u64>() else {
            return false;
        };
        match unix_time() {
            Ok(now) if now <= expires_at => {}
            Ok(_) | Err(_) => return false,
        }
        let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
            return false;
        };
        let Ok(mut mac) = HmacSha256::new_from_slice(&self.key) else {
            return false;
        };
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).is_ok()
    }

    fn sign(&self, payload: &str) -> std::io::Result<String> {
        let mut mac = HmacSha256::new_from_slice(&self.key).map_err(Error::other)?;
        mac.update(payload.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }
}

fn read_token(path: &Path) -> std::io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "browser token path is not a regular file: {}",
                path.display()
            ),
        ));
    }
    harden_permissions(path)?;
    let token = fs::read_to_string(path)?;
    let token = token.trim().to_string();
    decode_token(&token)?;
    Ok(token)
}

fn replace_token(path: &Path) -> std::io::Result<String> {
    let token = random_token();
    let parent = path
        .parent()
        .ok_or_else(|| Error::other("browser token path has no parent"))?;
    let temp_path = parent.join(format!(".token-{}", random_token()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp_path)?;
    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp_path, path)?;
    harden_permissions(path)?;
    Ok(token)
}

#[cfg(unix)]
fn harden_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn harden_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn decode_token(token: &str) -> std::io::Result<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(token)
        .map_err(Error::other)?
        .try_into()
        .map_err(|_| {
            Error::new(
                ErrorKind::InvalidData,
                "browser token must contain 32 bytes",
            )
        })
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unix_time() -> std::io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(Error::other)
}

fn session_cookie_value(header: &str) -> Option<&str> {
    header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE_NAME).then_some(value)
    })
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;

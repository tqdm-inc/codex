use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

const MAX_PATH_BYTES: usize = 32 * 1024;
const PAGE_SIZE: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryListRequest {
    pub(crate) path: Option<String>,
    pub(crate) cursor: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryListing {
    pub(crate) path: String,
    pub(crate) parent: Option<String>,
    pub(crate) roots: Vec<String>,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) next_cursor: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryEntry {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) symlink: bool,
}

pub(crate) fn list(request: DirectoryListRequest) -> std::io::Result<DirectoryListing> {
    let requested = match request.path {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        Some(_) | None => dirs::home_dir()
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "home directory is unavailable"))?,
    };
    if requested.as_os_str().len() > MAX_PATH_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "directory path is too long",
        ));
    }
    let path = requested.canonicalize().map_err(|error| {
        Error::new(
            error.kind(),
            format!("could not open {}: {error}", requested.display()),
        )
    })?;
    if !path.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("{} is not a directory", path.display()),
        ));
    }
    let mut entries = std::fs::read_dir(&path)?
        .filter_map(std::result::Result::ok)
        .filter_map(directory_entry)
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    let cursor = request.cursor.unwrap_or_default().min(entries.len());
    let next_cursor = (cursor + PAGE_SIZE < entries.len()).then_some(cursor + PAGE_SIZE);
    let entries = entries.into_iter().skip(cursor).take(PAGE_SIZE).collect();
    Ok(DirectoryListing {
        path: path_string(&path)?,
        parent: path.parent().map(path_string).transpose()?,
        roots: filesystem_roots(),
        entries,
        next_cursor,
    })
}

pub(crate) fn canonical_directory(path: &str) -> std::io::Result<PathBuf> {
    if path.len() > MAX_PATH_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "directory path is too long",
        ));
    }
    let canonical = Path::new(path).canonicalize()?;
    if !canonical.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "selected path is not a directory",
        ));
    }
    std::fs::read_dir(&canonical)?;
    path_string(&canonical)?;
    Ok(canonical)
}

pub(crate) fn path_string(path: &Path) -> std::io::Result<String> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidData,
            format!("path is not valid Unicode: {}", path.display()),
        )
    })
}

fn directory_entry(entry: std::fs::DirEntry) -> Option<DirectoryEntry> {
    let file_type = entry.file_type().ok()?;
    let symlink = file_type.is_symlink();
    if !(file_type.is_dir() || symlink && entry.path().is_dir()) {
        return None;
    }
    let name = entry.file_name().into_string().ok()?;
    let path = entry.path().into_os_string().into_string().ok()?;
    Some(DirectoryEntry {
        name,
        path,
        symlink,
    })
}

#[cfg(unix)]
fn filesystem_roots() -> Vec<String> {
    vec!["/".to_string()]
}

#[cfg(windows)]
fn filesystem_roots() -> Vec<String> {
    (b'A'..=b'Z')
        .map(|drive| format!("{}:\\", char::from(drive)))
        .filter(|root| Path::new(root).exists())
        .collect()
}

#[cfg(test)]
#[path = "directory_tests.rs"]
mod tests;

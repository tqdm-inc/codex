use std::fs;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn listing_contains_only_directories_in_stable_order() {
    let temp = tempfile::tempdir().expect("temp directory");
    fs::create_dir(temp.path().join("zeta")).expect("zeta");
    fs::create_dir(temp.path().join("Alpha")).expect("Alpha");
    fs::write(temp.path().join("ignored.txt"), "file").expect("file");

    let listing = list(DirectoryListRequest {
        path: Some(path_string(temp.path()).expect("Unicode temporary path")),
        cursor: None,
    })
    .expect("listing");

    assert_eq!(
        listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "zeta"]
    );
}

#[cfg(unix)]
#[test]
fn listing_omits_non_unicode_directory_names() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().expect("temp directory");
    fs::create_dir(temp.path().join("visible")).expect("visible directory");
    fs::create_dir(temp.path().join(OsString::from_vec(vec![b'h', b'i', 0xff])))
        .expect("non-Unicode directory");

    let listing = list(DirectoryListRequest {
        path: Some(path_string(temp.path()).expect("Unicode temporary path")),
        cursor: None,
    })
    .expect("listing");

    assert_eq!(
        listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["visible"]
    );
}

mod common;

use std::fs;

use common::{folder, nell, status_number};

#[test]
fn update_stores_only_the_changed_and_new_files() {
    let folder = folder("update");
    nell(&folder, &["init"]);
    let (_, before) = nell(&folder, &["status"]);
    fs::write(folder.join("src/auth.rs"), "fn validate_token() {}\nfn other() {}\nfn more() {}\n").unwrap();
    fs::write(folder.join("src/fresh.rs"), "fn fresh() {}\n").unwrap();
    let (code, out) = nell(&folder, &["update"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("2 files, 2 chunks, 0 vectors, "), "{out}");
    assert!(out.ends_with(" seconds\n"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    let (_, after) = nell(&folder, &["status"]);
    assert_eq!(status_number(&after, "files"), 3, "{after}");
    assert_eq!(status_number(&after, "chunks"), 3, "{after}");
    // notes.md was not touched, so its record stayed and only auth.rs became dead.
    assert!(status_number(&after, "dead bytes") > status_number(&before, "dead bytes"), "{after}");
}

#[test]
fn update_with_nothing_changed_stores_nothing() {
    let folder = folder("update-same");
    nell(&folder, &["init"]);
    let (_, before) = nell(&folder, &["status"]);
    let (code, out) = nell(&folder, &["update"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("0 files, 0 chunks, 0 vectors, "), "{out}");
    let (_, after) = nell(&folder, &["status"]);
    assert_eq!(before, after);
}

#[test]
fn update_on_an_empty_store_stores_everything() {
    let folder = folder("update-empty");
    let (code, out) = nell(&folder, &["update"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("2 files, 2 chunks, 0 vectors, "), "{out}");
    let (_, status) = nell(&folder, &["status"]);
    assert_eq!(status_number(&status, "files"), 2, "{status}");
}

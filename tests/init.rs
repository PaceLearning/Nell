mod common;

use std::fs;
use std::path::Path;

use common::{folder, nell, status_number};

/// Adds two 64 line files to a test folder. In `long.rs` line 23 is blank, inside the last third of the first
/// 32 lines, so its chunks are 1 to 23, 24 to 55 and 56 to 64. In `plain.rs` line 10 is blank, too early to
/// end a chunk, so its chunks are 1 to 32 and 33 to 64.
fn add_long_files(folder: &Path) {
    let mut long: Vec<String> = (1..=64).map(|line| format!("long {line}")).collect();
    long[22] = String::new();
    fs::write(folder.join("src/long.rs"), long.join("\n") + "\n").unwrap();
    let mut plain: Vec<String> = (1..=64).map(|line| format!("plain {line}")).collect();
    plain[9] = String::new();
    fs::write(folder.join("src/plain.rs"), plain.join("\n") + "\n").unwrap();
}

#[test]
fn init_prints_the_files_and_chunks_it_stored() {
    let folder = folder("init");
    add_long_files(&folder);
    let (code, out) = nell(&folder, &["init"]);
    assert_eq!(code, 0);
    // auth.rs and notes.md are one chunk each. The ignored log and the hidden .gitignore are skipped.
    assert!(out.starts_with("4 files, 7 chunks, "), "{out}");
    assert!(out.ends_with(" seconds\n"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[test]
fn status_agrees_with_init() {
    let folder = folder("init-status");
    add_long_files(&folder);
    nell(&folder, &["init"]);
    let (code, status) = nell(&folder, &["status"]);
    assert_eq!(code, 0);
    assert_eq!(status_number(&status, "files"), 4, "{status}");
    assert_eq!(status_number(&status, "chunks"), 7, "{status}");
}

#[test]
fn init_again_starts_the_store_over() {
    let folder = folder("init-again");
    add_long_files(&folder);
    nell(&folder, &["init"]);
    fs::remove_file(folder.join("src/plain.rs")).unwrap();
    let (code, out) = nell(&folder, &["init"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("3 files, 5 chunks, "), "{out}");
    let (_, status) = nell(&folder, &["status"]);
    assert_eq!(status_number(&status, "files"), 3, "{status}");
    assert_eq!(status_number(&status, "chunks"), 5, "{status}");
}

#[test]
fn a_search_after_init_finds_its_lines_from_the_store() {
    let folder = folder("init-search");
    nell(&folder, &["init"]);
    let (_, before) = nell(&folder, &["status"]);
    let (code, out) = nell(&folder, &["validate_token", "src"]);
    let (_, after) = nell(&folder, &["status"]);
    assert_eq!(code, 0);
    assert_eq!(out, "src/auth.rs:1:fn validate_token() {}\n".replace('/', std::path::MAIN_SEPARATOR_STR));
    assert_eq!(before, after);
}

#[test]
fn a_search_stores_the_chunks_of_what_it_reads() {
    let folder = folder("search-chunks");
    add_long_files(&folder);
    nell(&folder, &["long", "src"]);
    let (_, status) = nell(&folder, &["status"]);
    assert_eq!(status_number(&status, "files"), 4, "{status}");
    assert_eq!(status_number(&status, "chunks"), 7, "{status}");
}

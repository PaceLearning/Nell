mod common;

use std::fs;

use common::{folder, home, nell, start, status_number};

#[test]
fn status_prints_the_store_file_and_its_counts() {
    let folder = folder("status");
    let (code, out) = nell(&folder, &["status"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 6, "{out}");
    assert!(lines[0].starts_with(&format!("store: {}", home(&folder).display())), "{out}");
    assert_eq!(lines[1], "files: 0");
    assert_eq!(lines[2], "chunks: 0");
    assert_eq!(lines[3], "vectors: 0");
    assert_eq!(lines[4], "bytes: 32");
    assert_eq!(lines[5], "dead bytes: 0");
}

#[test]
fn a_second_search_reads_from_the_store() {
    let folder = folder("second");
    let first = nell(&folder, &["validate_token", "src"]);
    let (_, after_first) = nell(&folder, &["status"]);
    let second = nell(&folder, &["validate_token", "src"]);
    let (_, after_second) = nell(&folder, &["status"]);
    assert_eq!(first, second);
    assert_eq!(status_number(&after_first, "files"), 2);
    assert_eq!(after_first, after_second);
    assert_eq!(status_number(&after_second, "dead bytes"), 0);
}

#[test]
fn a_changed_file_is_read_and_stored_again() {
    let folder = folder("changed");
    nell(&folder, &["validate_token", "src"]);
    let (_, before) = nell(&folder, &["status"]);
    fs::write(folder.join("src/auth.rs"), "fn check_token() {}\nfn validate_token_twice() {}\n").unwrap();
    let (code, out) = nell(&folder, &["validate_token", "src"]);
    let (_, after) = nell(&folder, &["status"]);
    assert_eq!(code, 0);
    assert_eq!(out, "src/auth.rs:2:fn validate_token_twice() {}\n".replace('/', std::path::MAIN_SEPARATOR_STR));
    assert_eq!(status_number(&after, "files"), 2);
    assert!(status_number(&after, "bytes") > status_number(&before, "bytes"));
    assert!(status_number(&after, "dead bytes") > 0, "{after}");
}

#[test]
fn many_processes_at_once_leave_a_store_that_reads_cleanly() {
    let folder = folder("many");
    let mut running = Vec::new();
    for _ in 0..10 {
        running.push(start(&folder, &["-i", "validate", "src"]));
    }
    for child in running {
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 2);
    }
    let (code, status) = nell(&folder, &["status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status_number(&status, "files"), 2);
    assert_eq!(status_number(&status, "dead bytes"), 0, "{status}");
    let (code, out) = nell(&folder, &["validate_token", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 1);
}

#[test]
fn dead_records_are_dropped_once_they_are_more_than_half_the_file() {
    let folder = folder("compact");
    for round in 0..8 {
        let padding = "x".repeat(1000 + round);
        fs::write(folder.join("src/auth.rs"), format!("fn validate_token_{round}() {{}}\n// {padding}\n")).unwrap();
        let (code, out) = nell(&folder, &["validate_token", "src"]);
        assert_eq!(code, 0);
        assert!(out.contains(&format!("validate_token_{round}")), "{out}");
    }
    let (_, status) = nell(&folder, &["status"]);
    assert_eq!(status_number(&status, "files"), 2);
    // Eight versions of about a kilobyte each were written. Without compaction the file would hold them all.
    assert!(status_number(&status, "bytes") < 5000, "{status}");
}

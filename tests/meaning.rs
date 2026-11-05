mod common;

use std::fs;
use std::process::Command;

use common::{folder, home, nell};

/// Turns `/` into the platform's separator, the way the binary prints paths.
fn native(text: &str) -> String {
    text.replace('/', std::path::MAIN_SEPARATOR_STR)
}

#[test]
fn a_question_whose_words_are_in_one_file_ranks_that_file_first() {
    let folder = folder("meaning-rank");
    fs::write(folder.join("src/server.rs"), "fn handle_incoming_request() {\n    route_request();\n}\n").unwrap();
    fs::write(folder.join("src/cache.rs"), "fn store_value() {}\n").unwrap();
    nell(&folder, &["init"]);
    let (code, out) = nell(&folder, &["--meaning", "where is an incoming request handled", "src"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    // Line 1 holds three of the question's words, line 2 only one, so line 1 is the chunk's best line.
    assert_eq!(lines[0], native("src/server.rs:1:fn handle_incoming_request() {"), "{out}");
    assert_eq!(lines.len(), 1, "{out}");
}

#[test]
fn identifiers_are_split_on_case_and_underscores() {
    let folder = folder("meaning-split");
    fs::write(folder.join("src/Check.cs"), "class Check {\n    public bool ValidateToken(string value) => true;\n}\n")
        .unwrap();
    nell(&folder, &["init"]);
    let (code, out) = nell(&folder, &["--meaning", "validate", "src"]);
    assert_eq!(code, 0);
    assert!(out.contains(&native("src/Check.cs:2:    public bool ValidateToken(string value) => true;\n")), "{out}");
    assert!(out.contains(&native("src/auth.rs:1:fn validate_token() {}\n")), "{out}");
    assert_eq!(out.lines().count(), 3, "{out}");
}

#[test]
fn limit_sets_how_many_places_print() {
    let folder = folder("meaning-limit");
    nell(&folder, &["init"]);
    let (code, out) = nell(&folder, &["--meaning", "token", "--limit", "1", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[test]
fn exits_1_when_no_chunk_shares_a_word() {
    let folder = folder("meaning-none");
    nell(&folder, &["init"]);
    let (code, out) = nell(&folder, &["--meaning", "zebra crossing", "src"]);
    assert_eq!(code, 1);
    assert_eq!(out, "");
}

#[test]
fn exits_2_and_says_to_run_init_when_nothing_is_indexed() {
    let folder = folder("meaning-empty");
    let output = Command::new(env!("CARGO_BIN_EXE_nell"))
        .current_dir(&folder)
        .env("NELL_HOME", home(&folder))
        .args(["--meaning", "token", "src"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "nell: nothing indexed yet, run nell init\n");
}

#[test]
fn exits_2_without_a_question() {
    let folder = folder("meaning-bad");
    assert_eq!(nell(&folder, &["--meaning"]).0, 2);
}

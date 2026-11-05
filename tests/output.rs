mod common;

use std::fs;
use std::path::Path;

use common::{folder, nell};

/// Adds two source files that call `validate_token` 30 times each, so a search for it finds 61 lines in 3 files.
fn add_many_calls(folder: &Path) {
    let calls = "    validate_token();\n".repeat(30);
    fs::write(folder.join("src/first.rs"), &calls).unwrap();
    fs::write(folder.join("src/second.rs"), &calls).unwrap();
}

/// The file part of each output line, in order, with `/` as the separator and no leading `./`.
fn paths(out: &str) -> Vec<String> {
    let paths = out.lines().map(|line| line.split(':').next().unwrap().replace('\\', "/"));
    paths.map(|path| path.trim_start_matches("./").to_string()).collect()
}

#[test]
fn definitions_come_first_then_source_then_tests() {
    let folder = folder("order");
    fs::create_dir_all(folder.join("tests")).unwrap();
    fs::write(folder.join("src/caller.rs"), "fn main() {\n    let ok = validate_token();\n}\n").unwrap();
    fs::write(folder.join("src/auth.test.ts"), "validate_token();\n").unwrap();
    fs::write(folder.join("tests/flow.rs"), "fn validate_token_works() {\n    validate_token();\n}\n").unwrap();
    fs::write(folder.join("src/Auth.cs"), "public static bool validate_token(string token) => true;\n").unwrap();
    let (code, out) = nell(&folder, &["validate_token"]);
    assert_eq!(code, 0);
    let paths = paths(&out);
    assert_eq!(paths.len(), 6, "{out}");
    let mut definitions = paths[0..2].to_vec();
    definitions.sort();
    assert_eq!(definitions, ["src/Auth.cs", "src/auth.rs"], "{out}");
    assert_eq!(paths[2], "src/caller.rs", "{out}");
    let mut tests = paths[3..].to_vec();
    tests.sort();
    // flow.rs defines a test function, but a test file stays with the tests.
    assert_eq!(tests, ["src/auth.test.ts", "tests/flow.rs", "tests/flow.rs"], "{out}");
}

#[test]
fn prints_50_lines_and_says_what_was_left_out() {
    let folder = folder("bound");
    add_many_calls(&folder);
    let (code, out) = nell(&folder, &["validate_token", "src"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 51, "{out}");
    assert!(lines[0].ends_with(":1:fn validate_token() {}"), "{out}");
    assert_eq!(lines[50], "11 more matches in 1 files, --all prints them");
}

#[test]
fn limit_sets_the_bound() {
    let folder = folder("limit");
    add_many_calls(&folder);
    let (code, out) = nell(&folder, &["--limit", "5", "validate_token", "src"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 6, "{out}");
    assert_eq!(lines[5], "56 more matches in 2 files, --all prints them");
}

#[test]
fn all_prints_every_line() {
    let folder = folder("all");
    add_many_calls(&folder);
    let (code, out) = nell(&folder, &["--all", "validate_token", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 61, "{out}");
    assert!(!out.contains("more matches"), "{out}");
}

#[test]
fn exits_2_when_limit_is_not_a_number() {
    let folder = folder("limit-bad");
    assert_eq!(nell(&folder, &["--limit", "many", "validate_token", "src"]).0, 2);
    assert_eq!(nell(&folder, &["validate_token", "src", "--limit"]).0, 2);
}

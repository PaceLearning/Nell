mod common;

use std::fs;
use std::path::Path;

use common::{folder, native, nell, status_number};

/// Adds a caller before the definition in walk order, with a comment, a use, and a longer name that holds the
/// symbol and must not match.
fn add_caller(folder: &Path) {
    let caller = "// validate_token is called here\nfn main() {\n    validate_token();\n    validate_token_twice();\n}\n";
    fs::write(folder.join("src/a_caller.rs"), caller).unwrap();
}

#[test]
fn definitions_print_before_uses_and_comments_last() {
    let folder = folder("symbol-order");
    add_caller(&folder);
    let (code, out) = nell(&folder, &["--symbol", "validate_token", "src"]);
    assert_eq!(code, 0);
    let auth = native("src/auth.rs");
    let caller = native("src/a_caller.rs");
    let expected = format!(
        "{auth}:1:fn validate_token() {{}}\n{caller}:3:    validate_token();\n{caller}:1:// validate_token is called here\n"
    );
    assert_eq!(out, expected);
}

#[test]
fn i_ignores_case() {
    let folder = folder("symbol-case");
    add_caller(&folder);
    let (code, out) = nell(&folder, &["-i", "--symbol", "VALIDATE_TOKEN", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 3, "{out}");
    assert!(out.starts_with(&native("src/auth.rs:1:")), "{out}");
    assert_eq!(nell(&folder, &["--symbol", "VALIDATE_TOKEN", "src"]).0, 1);
}

#[test]
fn the_bound_counts_every_kind_and_each_file_once() {
    let folder = folder("symbol-bound");
    add_caller(&folder);
    let (code, out) = nell(&folder, &["--limit", "1", "--symbol", "validate_token", "src"]);
    assert_eq!(code, 0);
    let expected = "\
src/auth.rs:1:fn validate_token() {}
2 more matches in 1 files, --all prints them
";
    assert_eq!(out, native(expected));
}

#[test]
fn l_and_c_count_each_file_once() {
    let folder = folder("symbol-lc");
    add_caller(&folder);
    let (code, out) = nell(&folder, &["-c", "--symbol", "validate_token", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out, native("src/auth.rs:1\nsrc/a_caller.rs:2\n"));
    let (code, out) = nell(&folder, &["-l", "--symbol", "validate_token", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out, native("src/auth.rs\nsrc/a_caller.rs\n"));
}

#[test]
fn the_name_is_plain_text_and_a_second_search_reads_from_the_store() {
    let folder = folder("symbol-store");
    fs::write(folder.join("src/regex.rs"), "let a.b = 1;\naxb();\n").unwrap();
    let (code, out) = nell(&folder, &["--symbol", "a.b", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out, native("src/regex.rs:1:let a.b = 1;\n"));
    let (_, before) = nell(&folder, &["status"]);
    assert_eq!(status_number(&before, "files"), 3, "{before}");
    nell(&folder, &["--symbol", "a.b", "src"]);
    let (_, after) = nell(&folder, &["status"]);
    assert_eq!(before, after);
}

#[test]
fn exits_2_without_a_name_or_with_meaning() {
    let folder = folder("symbol-bad");
    assert_eq!(nell(&folder, &["--symbol"]).0, 2);
    assert_eq!(nell(&folder, &["--symbol", "token", "--meaning", "token"]).0, 2);
}

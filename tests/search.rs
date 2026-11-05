use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nell-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(folder.join("src")).unwrap();
    fs::write(folder.join("src/auth.rs"), "fn validate_token() {}\nfn other() {}\n").unwrap();
    fs::write(folder.join("src/notes.md"), "Validate the token first.\n").unwrap();
    fs::write(folder.join("target.log"), "validate_token called\n").unwrap();
    fs::write(folder.join(".gitignore"), "*.log\n").unwrap();
    folder
}

fn nell(folder: &PathBuf, arguments: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_nell"))
        .current_dir(folder)
        .args(arguments)
        .output()
        .unwrap();
    (output.status.code().unwrap(), String::from_utf8(output.stdout).unwrap())
}

#[test]
fn prints_path_line_and_text_for_each_match() {
    let folder = folder("match");
    let (code, out) = nell(&folder, &["validate_token", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out, "src/auth.rs:1:fn validate_token() {}\n".replace('/', std::path::MAIN_SEPARATOR_STR));
}

#[test]
fn ignores_case_with_i() {
    let folder = folder("case");
    let (code, out) = nell(&folder, &["-i", "validate", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 2);
}

#[test]
fn skips_what_gitignore_lists() {
    let folder = folder("ignore");
    let (_, out) = nell(&folder, &["validate_token"]);
    assert!(!out.contains("target.log"), "{out}");
}

#[test]
fn exits_1_when_nothing_matches() {
    let folder = folder("none");
    let (code, out) = nell(&folder, &["nowhere", "src"]);
    assert_eq!(code, 1);
    assert_eq!(out, "");
}

#[test]
fn exits_2_on_a_bad_pattern_or_no_pattern() {
    let folder = folder("bad");
    assert_eq!(nell(&folder, &["(", "src"]).0, 2);
    assert_eq!(nell(&folder, &[]).0, 2);
}

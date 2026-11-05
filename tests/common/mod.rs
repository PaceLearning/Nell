#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Makes a fresh folder with a small tree to search, and an empty store home beside it.
pub fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("nell-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let _ = fs::remove_dir_all(home(&folder));
    fs::create_dir_all(folder.join("src")).unwrap();
    fs::write(folder.join("src/auth.rs"), "fn validate_token() {}\nfn other() {}\n").unwrap();
    fs::write(folder.join("src/notes.md"), "Validate the token first.\n").unwrap();
    fs::write(folder.join("target.log"), "validate_token called\n").unwrap();
    fs::write(folder.join(".gitignore"), "*.log\n").unwrap();
    folder
}

/// The store home that goes with a test folder, so no test touches the user's real store.
pub fn home(folder: &Path) -> PathBuf {
    folder.with_file_name(format!("{}-home", folder.file_name().unwrap().to_string_lossy()))
}

/// Starts the binary in `folder` with its store in `home(folder)`, and returns it still running.
pub fn start(folder: &Path, arguments: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_nell"))
        .current_dir(folder)
        .env("NELL_HOME", home(folder))
        .args(arguments)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

/// Runs the binary in `folder` with its store in `home(folder)`, and returns the exit code and stdout.
pub fn nell(folder: &Path, arguments: &[&str]) -> (i32, String) {
    let output = start(folder, arguments).wait_with_output().unwrap();
    (output.status.code().unwrap(), String::from_utf8(output.stdout).unwrap())
}

/// Reads one number from `nell status` output, by the label before the colon.
pub fn status_number(status: &str, label: &str) -> u64 {
    let line = status.lines().find(|line| line.starts_with(&format!("{label}: "))).unwrap();
    line[label.len() + 2..].parse().unwrap()
}

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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

/// Starts the binary in `folder` with its store in `home(folder)`, and returns it still running. The model
/// download is pointed at a closed port, so no test fetches the model by accident; `nell_with_model` is for the
/// tests that mean to.
pub fn start(folder: &Path, arguments: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_nell"))
        .current_dir(folder)
        .env("NELL_HOME", home(folder))
        .env("HF_ENDPOINT", "http://127.0.0.1:1")
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Runs the binary in `folder` with its store in `home(folder)`, and returns the exit code and stdout.
pub fn nell(folder: &Path, arguments: &[&str]) -> (i32, String) {
    let output = start(folder, arguments).wait_with_output().unwrap();
    (output.status.code().unwrap(), String::from_utf8(output.stdout).unwrap())
}

/// Runs the binary the way `nell` does, and returns the exit code, stdout and stderr.
pub fn nell_with_stderr(folder: &Path, arguments: &[&str]) -> (i32, String, String) {
    let output = start(folder, arguments).wait_with_output().unwrap();
    let out = String::from_utf8(output.stdout).unwrap();
    let err = String::from_utf8(output.stderr).unwrap();
    (output.status.code().unwrap(), out, err)
}

/// Whether the tests that need the model should run: only when NELL_TEST_MODEL is set, because the first run
/// downloads the model. Prints why when they do not.
pub fn wants_model() -> bool {
    if std::env::var_os("NELL_TEST_MODEL").is_some() {
        return true;
    }
    println!("skipped: set NELL_TEST_MODEL to run the tests that load the model");
    false
}

/// One store home shared by every test that loads the model, kept between runs so the model is downloaded once.
pub fn model_home() -> PathBuf {
    std::env::temp_dir().join("nell-test-model-home")
}

/// Runs the binary in `folder` with its store in `model_home()` and the model download allowed, and returns the
/// exit code, stdout and stderr.
pub fn nell_with_model(folder: &Path, arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_nell"))
        .current_dir(folder)
        .env("NELL_HOME", model_home())
        .args(arguments)
        .output()
        .unwrap();
    let out = String::from_utf8(output.stdout).unwrap();
    let err = String::from_utf8(output.stderr).unwrap();
    (output.status.code().unwrap(), out, err)
}

/// Reads one number from `nell status` output, by the label before the colon.
pub fn status_number(status: &str, label: &str) -> u64 {
    let line = status.lines().find(|line| line.starts_with(&format!("{label}: "))).unwrap();
    line[label.len() + 2..].parse().unwrap()
}

//! The tests that load the model. They run only with NELL_TEST_MODEL set, because the first run downloads the
//! model, about 90 MB, into one store home shared by all of them.

mod common;

use std::fs;

use common::{folder, nell_with_model, status_number, wants_model};

#[test]
fn init_gives_every_chunk_a_vector() {
    if !wants_model() {
        return;
    }
    let folder = folder("vectors-init");
    let (code, out, err) = nell_with_model(&folder, &["init"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("2 files, 2 chunks, 2 vectors, "), "{out}");
    assert_eq!(err, "");
    let (_, status, _) = nell_with_model(&folder, &["status"]);
    assert_eq!(status_number(&status, "chunks"), 2, "{status}");
    assert_eq!(status_number(&status, "vectors"), 2, "{status}");
}

#[test]
fn update_embeds_only_the_changed_files() {
    if !wants_model() {
        return;
    }
    let folder = folder("vectors-update");
    nell_with_model(&folder, &["init"]);
    fs::write(folder.join("src/auth.rs"), "fn validate_token() {}\nfn other() {}\nfn more() {}\n").unwrap();
    let (code, out, err) = nell_with_model(&folder, &["update"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("1 files, 1 chunks, 1 vectors, "), "{out}");
    let (_, status, _) = nell_with_model(&folder, &["status"]);
    assert_eq!(status_number(&status, "vectors"), 2, "{status}");
}

#[test]
fn a_text_search_leaves_a_changed_file_without_vectors_and_a_meaning_search_fills_them() {
    if !wants_model() {
        return;
    }
    let folder = folder("vectors-search");
    nell_with_model(&folder, &["init"]);
    fs::write(folder.join("src/auth.rs"), "fn validate_token() {}\nfn other() {}\nfn more() {}\n").unwrap();
    let (code, _, _) = nell_with_model(&folder, &["validate_token", "src"]);
    assert_eq!(code, 0);
    let (_, status, _) = nell_with_model(&folder, &["status"]);
    assert_eq!(status_number(&status, "chunks"), 2, "{status}");
    assert_eq!(status_number(&status, "vectors"), 1, "{status}");
    let (code, out, err) = nell_with_model(&folder, &["--meaning", "checking a token", "src"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err, "");
    assert_eq!(out.lines().count(), 2, "{out}");
    let (_, status, _) = nell_with_model(&folder, &["status"]);
    assert_eq!(status_number(&status, "vectors"), 2, "{status}");
}

#[test]
fn a_meaning_search_ranks_by_meaning_not_by_shared_words() {
    if !wants_model() {
        return;
    }
    let folder = folder("vectors-rank");
    let login = "fn sign_in(name: &str, password: &str) -> Session {\n    check the password hash and open a session\n}\n";
    let draw = "fn paint_circle(radius: f32) {\n    put pixels on the canvas around the center\n}\n";
    fs::write(folder.join("src/login.rs"), login).unwrap();
    fs::write(folder.join("src/draw.rs"), draw).unwrap();
    nell_with_model(&folder, &["init"]);
    let (code, out, err) = nell_with_model(&folder, &["--meaning", "where does someone authenticate", "src"]);
    assert_eq!(code, 0, "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{out}");
    // No line shares a word with the question, so only the vectors can put the drawing code last, after the
    // three places about checking a token or a password.
    assert!(lines[3].contains("draw.rs"), "{out}");
    assert!(lines[0..3].iter().all(|line| !line.contains("draw.rs")), "{out}");
}

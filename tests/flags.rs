mod common;

use std::fs;

use common::{folder, native, nell};

#[test]
fn n_changes_nothing() {
    let folder = folder("flag-n");
    let plain = nell(&folder, &["validate_token", "src"]);
    let numbered = nell(&folder, &["-n", "validate_token", "src"]);
    assert_eq!(plain, numbered);
}

#[test]
fn l_prints_each_matching_path_once_in_group_order() {
    let folder = folder("flag-l");
    fs::write(folder.join("src/caller.rs"), "validate_token();\nvalidate_token();\n").unwrap();
    let (code, out) = nell(&folder, &["-l", "validate_token", "src"]);
    assert_eq!(code, 0);
    // auth.rs defines it, so it comes before caller.rs.
    assert_eq!(out, native("src/auth.rs\nsrc/caller.rs\n"));
}

#[test]
fn c_prints_path_and_count_for_each_matching_file() {
    let folder = folder("flag-c");
    fs::write(folder.join("src/caller.rs"), "validate_token();\nother();\nvalidate_token();\n").unwrap();
    let (code, out) = nell(&folder, &["validate_token", "src", "-c"]);
    assert_eq!(code, 0);
    assert_eq!(out, native("src/auth.rs:1\nsrc/caller.rs:2\n"));
}

#[test]
fn w_matches_whole_words_only() {
    let folder = folder("flag-w");
    fs::write(folder.join("src/words.txt"), "token\ntokens\nmy_token\nthe token here\n").unwrap();
    let (code, out) = nell(&folder, &["-w", "token", "src/words.txt"]);
    assert_eq!(code, 0);
    // A path given as a file prints as it was typed.
    assert_eq!(out, "src/words.txt:1:token\nsrc/words.txt:4:the token here\n");
}

#[test]
fn c_upper_prints_context_with_dashes_between_groups() {
    let folder = folder("flag-context");
    let text = "one\ntwo\nhit\nfour\nfive\nsix\nseven\nhit\nnine\n";
    fs::write(folder.join("src/lines.txt"), text).unwrap();
    let (code, out) = nell(&folder, &["-C", "1", "hit", "src/lines.txt"]);
    assert_eq!(code, 0);
    let expected = "\
src/lines.txt-2-two
src/lines.txt:3:hit
src/lines.txt-4-four
--
src/lines.txt-7-seven
src/lines.txt:8:hit
src/lines.txt-9-nine
";
    assert_eq!(out, expected);
}

#[test]
fn c_upper_joins_groups_that_touch_and_takes_an_attached_number() {
    let folder = folder("flag-context-join");
    fs::write(folder.join("src/lines.txt"), "one\nhit\nthree\nfour\nhit\nsix\n").unwrap();
    let (code, out) = nell(&folder, &["hit", "-C1", "src/lines.txt"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 6, "{out}");
    assert!(!out.contains("--"), "{out}");
}

#[test]
fn context_lines_do_not_count_toward_the_bound() {
    let folder = folder("flag-context-bound");
    let text = "hit\nquiet\nquiet\nhit\nquiet\nquiet\nhit\nquiet\n";
    fs::write(folder.join("src/lines.txt"), text).unwrap();
    let (code, out) = nell(&folder, &["--limit", "2", "-C", "1", "hit", "src/lines.txt"]);
    assert_eq!(code, 0);
    let expected = "\
src/lines.txt:1:hit
src/lines.txt-2-quiet
src/lines.txt-3-quiet
src/lines.txt:4:hit
src/lines.txt-5-quiet
1 more matches in 1 files, --all prints them
";
    assert_eq!(out, expected);
}

#[test]
fn combined_short_flags_work() {
    let folder = folder("flag-combined");
    let (code, out) = nell(&folder, &["-inw", "VALIDATE", "src"]);
    assert_eq!(code, 0);
    assert_eq!(out, native("src/notes.md:1:Validate the token first.\n"));
}

#[test]
fn exits_2_on_an_unknown_flag_or_a_bad_context() {
    let folder = folder("flag-bad");
    assert_eq!(nell(&folder, &["-z", "validate", "src"]).0, 2);
    assert_eq!(nell(&folder, &["validate", "src", "-C"]).0, 2);
    assert_eq!(nell(&folder, &["-C", "some", "validate", "src"]).0, 2);
}

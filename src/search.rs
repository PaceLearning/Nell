use std::fmt;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

use crate::files;
use crate::store::{Store, StoreError};

/// The keywords that make a line a definition when they come before the matched name. This is the one list.
pub const DEFINING: &[&str] = &[
    "fn", "struct", "enum", "trait", "impl", "type", "const", "static", "mod", "class", "record", "interface", "def",
    "function", "let", "var", "func", "public", "private", "protected", "internal", "abstract", "sealed", "readonly",
    "pub", "async",
];

/// Folder names that make every file under them a test file.
pub const TEST_FOLDERS: &[&str] = &["test", "tests", "spec", "specs"];

/// Endings of a file name, before the extension, that make it a test file.
pub const TEST_ENDINGS: &[&str] = &["_test", "_tests", ".test", ".spec", "Test", "Tests"];

/// What a line starts with, after whitespace, when it is only a comment. A search by symbol prints such lines
/// after the uses.
pub const COMMENT_STARTS: &[&str] = &["//", "#", "/*", "*", "---"];

/// One line that matched: the line number counted from 1 and the line's text.
#[derive(Clone)]
pub struct Match {
    /// The line number, counted from 1.
    pub line: usize,
    /// The line's text.
    pub text: String,
}

/// The lines that matched in one file, in line order, and the lines around them that do not match.
pub struct Group {
    /// The file, as the walk gave it.
    pub path: PathBuf,
    /// The lines that matched, in line order.
    pub matches: Vec<Match>,
    /// Every line that does not match and is at most the search's context lines away from a match, in line order.
    pub context: Vec<Match>,
}

/// Why a search could not be created.
pub enum SearchError {
    /// The pattern is not a valid regular expression.
    BadPattern(regex::Error),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::BadPattern(error) => write!(f, "bad pattern: {error}"),
        }
    }
}

/// A text search over the files under some paths, the way grep does it.
pub struct Search {
    pattern: Regex,
    paths: Vec<String>,
    context: usize,
    symbol: bool,
}

impl Search {
    /// Creates a search for `pattern` under `paths`. With `whole_word` a match must not have a word character
    /// just before or just after it, the way grep's `-w` works. `context` is how many lines around each match
    /// are returned as context. With `symbol` the pattern is a name, matched as plain text and as a whole word,
    /// and the lines come back ordered by symbol, as `run` says. Fails when the pattern is not a valid regular
    /// expression.
    pub fn create(
        pattern: &str,
        paths: Vec<String>,
        ignore_case: bool,
        whole_word: bool,
        context: usize,
        symbol: bool,
    ) -> Result<Search, SearchError> {
        let mut inner = pattern.to_string();
        if symbol {
            inner = regex::escape(pattern);
        }
        let mut source = inner.clone();
        if whole_word || symbol {
            source = format!(r"\b{{start-half}}(?:{inner})\b{{end-half}}");
        }
        let pattern = RegexBuilder::new(&source)
            .case_insensitive(ignore_case)
            .build()
            .map_err(SearchError::BadPattern)?;
        Ok(Search { pattern, paths, context, symbol })
    }

    /// Walks the paths, skipping what `.gitignore` lists and hidden files, and returns the lines that match,
    /// grouped by file, each group with its context lines. Source files with a definition line come first, then other source files, then test
    /// files. Within each of those, files are in walk order.
    ///
    /// A line is a definition when the words before the matched name, on the same line, start with a keyword
    /// from `DEFINING` followed only by other words, such as modifiers and type names. A file is a test file when
    /// its path, from the repository root, has a folder from `TEST_FOLDERS` or its name ends with one of
    /// `TEST_ENDINGS` before the extension.
    ///
    /// A search by symbol orders lines instead of files. It returns the definition lines first, then the lines
    /// that use the name, then the lines that are only comments, each kind in walk order and grouped by file, so
    /// a file can have a group in each kind. A line is only a comment when it starts with one of
    /// `COMMENT_STARTS` after whitespace, and that comes before the definition rule.
    ///
    /// A file whose size and modified time the store already holds is read from the store. Any other file is
    /// read from disk and stored first, with its chunks. Files that cannot be read or are not text are skipped.
    /// Fails only when the store cannot be written.
    pub fn run(&self, store: &mut Store) -> Result<Vec<Group>, StoreError> {
        let mut found: Vec<(u8, Group)> = Vec::new();
        for entry in files::walk(Path::new(&self.paths[0]), &self.paths[1..]) {
            let Ok(entry) = entry else { continue };
            let Some(kept) = files::keep(store, &entry)? else { continue };
            let Some(record) = store.held(&kept.key, kept.size, kept.modified) else { continue };
            let lines: Vec<&str> = record.lines().collect();
            let mut matches = Vec::new();
            // The kind of each match for a search by symbol: 0 a definition, 1 a use, 2 a comment.
            let mut kinds: Vec<u8> = Vec::new();
            let mut definition = false;
            for (index, line) in lines.iter().enumerate() {
                let mut matched = false;
                let mut defines = false;
                for at in self.pattern.find_iter(line) {
                    matched = true;
                    // The last word up to the end of the match is the matched name. The words before it decide.
                    let words: Vec<&str> = line[..at.end()].split_whitespace().collect();
                    let before = &words[..words.len().saturating_sub(1)];
                    let Some(keyword) = before.iter().rposition(|word| DEFINING.contains(word)) else { continue };
                    let plain = before[keyword + 1..]
                        .iter()
                        .all(|word| word.chars().all(|c| c.is_alphanumeric() || "_<>[]?,.:&*'".contains(c)));
                    if plain {
                        defines = true;
                    }
                }
                if !matched {
                    continue;
                }
                if defines {
                    definition = true;
                }
                let trimmed = line.trim_start();
                let mut kind = 1;
                if defines {
                    kind = 0;
                }
                if COMMENT_STARTS.iter().any(|start| trimmed.starts_with(start)) {
                    kind = 2;
                }
                kinds.push(kind);
                matches.push(Match { line: index + 1, text: line.to_string() });
            }
            if matches.is_empty() {
                continue;
            }
            let mut near = vec![false; lines.len()];
            for item in &matches {
                let first = item.line.saturating_sub(1 + self.context);
                let last = (item.line + self.context).min(lines.len());
                near[first..last].fill(true);
            }
            for item in &matches {
                near[item.line - 1] = false;
            }
            let context: Vec<Match> = lines
                .iter()
                .enumerate()
                .filter(|(index, _)| near[*index])
                .map(|(index, line)| Match { line: index + 1, text: line.to_string() })
                .collect();
            let path = entry.path().to_path_buf();
            if self.symbol {
                for kind in 0..3 {
                    let picked: Vec<Match> = matches
                        .iter()
                        .zip(&kinds)
                        .filter(|(_, of)| **of == kind)
                        .map(|(item, _)| item.clone())
                        .collect();
                    if picked.is_empty() {
                        continue;
                    }
                    found.push((kind, Group { path: path.clone(), matches: picked, context: context.clone() }));
                }
                continue;
            }
            let key = Path::new(&kept.key);
            let mut folders = key.parent().map(|parent| parent.components()).into_iter().flatten();
            let in_test_folder = folders.any(|part| {
                let name = part.as_os_str().to_string_lossy();
                TEST_FOLDERS.iter().any(|folder| name.eq_ignore_ascii_case(folder))
            });
            let stem = key.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let test_name = TEST_ENDINGS.iter().any(|ending| stem.ends_with(ending));
            let mut rank = 1;
            if definition {
                rank = 0;
            }
            if in_test_folder || test_name {
                rank = 2;
            }
            found.push((rank, Group { path, matches, context }));
        }
        found.sort_by_key(|(rank, _)| *rank);
        Ok(found.into_iter().map(|(_, group)| group).collect())
    }
}

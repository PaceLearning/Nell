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

/// One line that matched: the line number counted from 1 and the line's text.
pub struct Match {
    pub line: usize,
    pub text: String,
}

/// The lines that matched in one file, in line order.
pub struct Group {
    pub path: PathBuf,
    pub matches: Vec<Match>,
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
}

impl Search {
    /// Creates a search for `pattern` under `paths`. Fails when the pattern is not a valid regular expression.
    pub fn create(pattern: &str, paths: Vec<String>, ignore_case: bool) -> Result<Search, SearchError> {
        let pattern = RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(SearchError::BadPattern)?;
        Ok(Search { pattern, paths })
    }

    /// Walks the paths, skipping what `.gitignore` lists and hidden files, and returns the lines that match,
    /// grouped by file. Source files with a definition line come first, then other source files, then test
    /// files. Within each of those, files are in walk order.
    ///
    /// A line is a definition when the words before the matched name, on the same line, start with a keyword
    /// from `DEFINING` followed only by other words, such as modifiers and type names. A file is a test file when
    /// its path, from the repository root, has a folder from `TEST_FOLDERS` or its name ends with one of
    /// `TEST_ENDINGS` before the extension.
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
            let mut matches = Vec::new();
            let mut definition = false;
            for (index, line) in record.lines().enumerate() {
                let mut matched = false;
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
                        definition = true;
                    }
                }
                if !matched {
                    continue;
                }
                matches.push(Match { line: index + 1, text: line.to_string() });
            }
            if matches.is_empty() {
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
            found.push((rank, Group { path: entry.path().to_path_buf(), matches }));
        }
        found.sort_by_key(|(rank, _)| *rank);
        Ok(found.into_iter().map(|(_, group)| group).collect())
    }
}

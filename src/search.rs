use std::fmt;
use std::path::PathBuf;

use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};

/// One line that matched, as grep prints it: the file, the line number counted from 1, and the line's text.
pub struct Match {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
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

    /// Walks the paths, skipping what `.gitignore` lists and hidden files, and returns every line that matches,
    /// in walk order. Files that cannot be read or are not text are skipped.
    pub fn run(&self) -> Vec<Match> {
        let mut found = Vec::new();
        let mut walk = WalkBuilder::new(&self.paths[0]);
        walk.require_git(false);
        for path in &self.paths[1..] {
            walk.add(path);
        }
        for entry in walk.build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else { continue };
            let Ok(text) = String::from_utf8(bytes) else { continue };
            for (index, line) in text.lines().enumerate() {
                if !self.pattern.is_match(line) {
                    continue;
                }
                found.push(Match {
                    path: entry.path().to_path_buf(),
                    line: index + 1,
                    text: line.to_string(),
                });
            }
        }
        found
    }
}

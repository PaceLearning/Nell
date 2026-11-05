use std::fmt;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

use crate::files;
use crate::store::{Store, StoreError};

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
    /// in walk order. A file whose size and modified time the store already holds is read from the store. Any
    /// other file is read from disk and stored first, with its chunks. Files that cannot be read or are not text
    /// are skipped. Fails only when the store cannot be written.
    pub fn run(&self, store: &mut Store) -> Result<Vec<Match>, StoreError> {
        let mut found = Vec::new();
        for entry in files::walk(Path::new(&self.paths[0]), &self.paths[1..]) {
            let Ok(entry) = entry else { continue };
            let Some(kept) = files::keep(store, &entry)? else { continue };
            let Some(record) = store.held(&kept.key, kept.size, kept.modified) else { continue };
            for (index, line) in record.lines().enumerate() {
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
        Ok(found)
    }
}

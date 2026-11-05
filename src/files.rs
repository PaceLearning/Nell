//! What a search and `nell init` share: walking files the way grep does, and keeping each one in the store.

use std::path::Path;

use ignore::{DirEntry, Walk, WalkBuilder};

use crate::record::Modified;
use crate::store::{Store, StoreError};

/// A file the store holds, by the key and the size and modified time it was stored under.
pub struct Kept {
    pub key: String,
    pub size: u64,
    pub modified: Modified,
}

/// Walks `first` and every path in `rest`, skipping what `.gitignore` lists and hidden files.
pub fn walk(first: &Path, rest: &[String]) -> Walk {
    let mut walk = WalkBuilder::new(first);
    walk.require_git(false);
    for path in rest {
        walk.add(path);
    }
    walk.build()
}

/// The key the store keeps the file at `path` under: its path from the repository root, or its absolute path
/// when it is outside the root. Nothing when the path cannot be made absolute.
pub fn key(store: &Store, path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).ok()?;
    Some(absolute.strip_prefix(store.root()).unwrap_or(&absolute).to_string_lossy().into_owned())
}

/// Makes sure the store holds the file at `entry` as it is on disk. A file whose size and modified time the
/// store already holds is left alone. Any other file is read from disk and stored, with its chunks. Returns
/// nothing for an entry that is not a file, cannot be read, or is not text. Fails only when the store cannot be
/// written.
pub fn keep(store: &mut Store, entry: &DirEntry) -> Result<Option<Kept>, StoreError> {
    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
        return Ok(None);
    }
    let Ok(metadata) = entry.metadata() else { return Ok(None) };
    let Ok(time) = metadata.modified() else { return Ok(None) };
    let size = metadata.len();
    let modified = Modified::create(time);
    let Some(key) = key(store, entry.path()) else { return Ok(None) };
    if store.held(&key, size, modified).is_some() {
        return Ok(Some(Kept { key, size, modified }));
    }
    let Ok(bytes) = std::fs::read(entry.path()) else { return Ok(None) };
    let Ok(text) = String::from_utf8(bytes) else { return Ok(None) };
    store.put(&key, size, modified, &text)?;
    Ok(Some(Kept { key, size, modified }))
}

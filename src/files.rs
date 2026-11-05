//! What a search, `nell init` and `nell update` share: walking files the way grep does, keeping each one in the
//! store, and giving the chunks of the kept files their vectors.

use std::fmt;
use std::path::Path;

use ignore::{DirEntry, Walk, WalkBuilder};

use crate::model::{Model, ModelError};
use crate::record::Modified;
use crate::store::{Store, StoreError};
use crate::vector::{self, Quantized};

/// A file the store holds, by the key and the size and modified time it was stored under, and whether this
/// process read it from disk and stored it.
pub struct Kept {
    pub key: String,
    pub size: u64,
    pub modified: Modified,
    pub stored: bool,
}

/// Why chunks could not be given their vectors.
pub enum EmbedError {
    /// The model could not embed.
    Model(ModelError),
    /// The store could not be written.
    Store(StoreError),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmbedError::Model(error) => write!(f, "{error}"),
            EmbedError::Store(error) => write!(f, "{error}"),
        }
    }
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
/// store already holds is left alone. Any other file is read from disk and stored, with its chunks and without
/// vectors. Returns nothing for an entry that is not a file, cannot be read, or is not text. Fails only when
/// the store cannot be written.
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
        return Ok(Some(Kept { key, size, modified, stored: false }));
    }
    let Ok(bytes) = std::fs::read(entry.path()) else { return Ok(None) };
    let Ok(text) = String::from_utf8(bytes) else { return Ok(None) };
    store.put(&key, size, modified, &text)?;
    Ok(Some(Kept { key, size, modified, stored: true }))
}

/// Gives every chunk of `files` that has no vector one, through `model`, and keeps the vectors in the store. A
/// file whose chunks all have vectors is left alone. Each chunk is embedded as its lines joined with newlines.
/// Returns how many vectors were made.
pub fn embed(store: &mut Store, model: &mut Model, files: &[Kept]) -> Result<usize, EmbedError> {
    let mut made = 0;
    for kept in files {
        let Some(record) = store.held(&kept.key, kept.size, kept.modified) else { continue };
        if record.vector_count() >= record.chunk_count() {
            continue;
        }
        let lines: Vec<&str> = record.lines().collect();
        let mut texts = Vec::with_capacity(record.chunk_count());
        for (first, last) in record.chunks() {
            let text = lines.get(first.saturating_sub(1)..last).map_or(String::new(), |lines| lines.join("\n"));
            texts.push(text);
        }
        let vectors = model.embed(&texts).map_err(EmbedError::Model)?;
        let quantized: Vec<Quantized> = vectors.iter().map(|vector| vector::quantize(vector)).collect();
        store.put_vectors(&kept.key, kept.size, kept.modified, &quantized).map_err(EmbedError::Store)?;
        made += quantized.len();
    }
    Ok(made)
}

//! The store: one memory mapped file per repository that every nell process shares.
//!
//! The file starts with a header: a magic, a format version and the committed length. Records follow one after
//! another. A writer takes an exclusive lock, appends, then advances the committed length. A reader maps the
//! file and trusts only the bytes inside the committed length, so it never sees a half written record.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::record::{self, Head, Modified, Record};
use crate::vector::Quantized;

const MAGIC: &[u8; 8] = b"nellsto1";
/// Written over the magic of a file that a compaction has replaced, so a writer that was waiting on its lock
/// knows to open the store again.
const DEAD: &[u8; 8] = b"nelldead";
const VERSION: u32 = 1;
/// The header: magic at 0, version at 8, committed length at 16, the rest reserved.
const HEADER: usize = 32;

/// Why the store could not be opened or written.
pub enum StoreError {
    /// No folder to keep stores in: none of NELL_HOME, LOCALAPPDATA, XDG_DATA_HOME or HOME is set.
    NoDataFolder,
    /// The current directory could not be read.
    NoCurrentDir(io::Error),
    /// A file could not be read or written.
    Io(PathBuf, io::Error),
    /// The file is not a store nell can read.
    BadFormat(PathBuf),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NoDataFolder => write!(f, "no folder for the store: set NELL_HOME"),
            StoreError::NoCurrentDir(error) => write!(f, "cannot read the current directory: {error}"),
            StoreError::Io(path, error) => write!(f, "{}: {error}", path.display()),
            StoreError::BadFormat(path) => write!(f, "{}: not a nell store", path.display()),
        }
    }
}

/// What `nell status` prints.
pub struct Status {
    /// The store file.
    pub path: PathBuf,
    /// How many files the store holds a version of.
    pub files: usize,
    /// How many chunks those files are split into.
    pub chunks: usize,
    /// How many of those chunks have a vector.
    pub vectors: usize,
    /// The committed length of the store file.
    pub bytes: u64,
    /// Bytes held by records that a newer version has replaced.
    pub dead_bytes: u64,
}

/// The store of one repository, mapped into memory.
pub struct Store {
    path: PathBuf,
    root: PathBuf,
    file: File,
    map: Mmap,
    committed: usize,
    /// The latest text record of each path.
    index: HashMap<String, Head>,
    /// The vectors record written for the latest text record of each path, for the paths that have one.
    vectors: HashMap<String, Head>,
}

/// Finds the repository root for a search path: the nearest ancestor with a `.git` entry, or the current
/// directory when there is none.
pub fn root_of(first: &Path) -> Result<PathBuf, StoreError> {
    let absolute = std::path::absolute(first).map_err(StoreError::NoCurrentDir)?;
    for ancestor in absolute.ancestors() {
        if ancestor.join(".git").exists() {
            return Ok(ancestor.to_path_buf());
        }
    }
    std::env::current_dir().map_err(StoreError::NoCurrentDir)
}

/// The folder nell keeps its stores and its model files in: the folder NELL_HOME names, or `nell` under the
/// user's local data folder. It is created when it does not exist.
pub fn data_folder() -> Result<PathBuf, StoreError> {
    let mut folder = std::env::var_os("NELL_HOME").map(PathBuf::from);
    if folder.is_none()
        && cfg!(windows)
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        folder = Some(PathBuf::from(local).join("nell"));
    }
    if folder.is_none() && let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        folder = Some(PathBuf::from(data).join("nell"));
    }
    if folder.is_none() && let Some(home) = std::env::var_os("HOME") {
        folder = Some(PathBuf::from(home).join(".local").join("share").join("nell"));
    }
    let Some(folder) = folder else {
        return Err(StoreError::NoDataFolder);
    };
    fs::create_dir_all(&folder).map_err(|error| StoreError::Io(folder.clone(), error))?;
    Ok(folder)
}

impl Store {
    /// Opens the store of the repository at `root`, creating it when there is none. The store file sits in the
    /// data folder and is named from a hash of `root`.
    pub fn open(root: &Path) -> Result<Store, StoreError> {
        let folder = data_folder()?;
        // FNV-1a over the root path, so the same repository always maps to the same file.
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in root.to_string_lossy().bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let path = folder.join(format!("{hash:016x}.nell"));
        // A compaction can replace the file between opening it and reading its header; open again when it did.
        for _ in 0..3 {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|error| StoreError::Io(path.clone(), error))?;
            let length = file.metadata().map_err(|error| StoreError::Io(path.clone(), error))?.len();
            if length < HEADER as u64 {
                file.lock().map_err(|error| StoreError::Io(path.clone(), error))?;
                let length = file.metadata().map_err(|error| StoreError::Io(path.clone(), error))?.len();
                if length < HEADER as u64 {
                    let mut header = [0u8; HEADER];
                    header[0..8].copy_from_slice(MAGIC);
                    header[8..12].copy_from_slice(&VERSION.to_le_bytes());
                    header[16..24].copy_from_slice(&(HEADER as u64).to_le_bytes());
                    (&file).seek(SeekFrom::Start(0)).map_err(|error| StoreError::Io(path.clone(), error))?;
                    (&file).write_all(&header).map_err(|error| StoreError::Io(path.clone(), error))?;
                    (&file).flush().map_err(|error| StoreError::Io(path.clone(), error))?;
                }
                file.unlock().map_err(|error| StoreError::Io(path.clone(), error))?;
            }
            // SAFETY: the map is read only, and the store protocol never truncates or rewrites mapped bytes:
            // writers only append past the committed length, and a replaced file keeps its old bytes until
            // every handle to it is closed.
            let map = unsafe { Mmap::map(&file) }.map_err(|error| StoreError::Io(path.clone(), error))?;
            if map.len() < HEADER {
                return Err(StoreError::BadFormat(path));
            }
            if &map[0..8] == DEAD {
                continue;
            }
            if &map[0..8] != MAGIC || record::u32_at(&map, 8) != Some(VERSION) {
                return Err(StoreError::BadFormat(path));
            }
            let committed = record::u64_at(&map, 16).unwrap_or(0).min(map.len() as u64) as usize;
            let committed = committed.max(HEADER);
            let (heads, end) = record::scan(&map[..committed], HEADER);
            let mut store = Store {
                path,
                root: root.to_path_buf(),
                file,
                map,
                committed: end,
                index: HashMap::new(),
                vectors: HashMap::new(),
            };
            store.absorb(heads);
            return Ok(store);
        }
        Err(StoreError::BadFormat(path))
    }

    /// The repository root this store belongs to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The data folder the store file sits in, where the model files go too.
    pub fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }

    /// Whether the store holds any version of `path`.
    pub fn knows(&self, path: &str) -> bool {
        self.index.contains_key(path)
    }

    /// The latest record of `path`, when it was stored with the same size and modified time, with its vectors
    /// when the store holds them for that version.
    pub fn held(&self, path: &str, size: u64, modified: Modified) -> Option<Record<'_>> {
        let head = self.index.get(path)?;
        let bytes = &self.map[..self.committed];
        let mut record = record::read(bytes, head.offset)?;
        if record.size != size || record.modified != modified {
            return None;
        }
        let vectors = self.vectors.get(path).and_then(|head| record::read_vectors(bytes, head.offset));
        if let Some(vectors) = vectors
            && vectors.size == size
            && vectors.modified == modified
        {
            record.vectors = Some(vectors);
        }
        Some(record)
    }

    /// Stores a version of `path`, under the lock. Does nothing when another process stored the same version
    /// first. Copies the live records into a fresh file first when dead bytes are more than half the file.
    pub fn put(&mut self, path: &str, size: u64, modified: Modified, text: &str) -> Result<(), StoreError> {
        self.lock()?;
        if self.held(path, size, modified).is_some() {
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            return Ok(());
        }
        let bytes = record::encode(path, size, modified, text);
        let offset = self.append(&bytes)?;
        let head = Head { kind: record::KIND_TEXT, path: path.to_string(), offset, total: bytes.len() };
        self.index.insert(path.to_string(), head);
        // The vectors of the version this one replaces are stale now.
        self.vectors.remove(path);
        self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        Ok(())
    }

    /// Stores the vectors of the version of `path` that had `size` and `modified` on disk, under the lock. Does
    /// nothing when that version is no longer the latest one, or when another process stored its vectors first.
    pub fn put_vectors(
        &mut self,
        path: &str,
        size: u64,
        modified: Modified,
        vectors: &[Quantized],
    ) -> Result<(), StoreError> {
        self.lock()?;
        let held = self.held(path, size, modified).map(|record| record.vectors.is_some());
        let Some(has_vectors) = held else {
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            return Ok(());
        };
        if has_vectors {
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            return Ok(());
        }
        let bytes = record::encode_vectors(path, size, modified, vectors);
        let offset = self.append(&bytes)?;
        let head = Head { kind: record::KIND_VECTORS, path: path.to_string(), offset, total: bytes.len() };
        self.vectors.insert(path.to_string(), head);
        self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        Ok(())
    }

    /// Starts the store over, under the lock, by appending a record that marks every record before it as dead.
    /// Processes that have the store open see it at their next write. The dead bytes go at the next compaction.
    pub fn clear(&mut self) -> Result<(), StoreError> {
        self.lock()?;
        let bytes = record::encode_clear();
        self.append(&bytes)?;
        self.index.clear();
        self.vectors.clear();
        self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        Ok(())
    }

    /// The store file, how many files, chunks and vectors it holds, its committed length and its dead bytes.
    pub fn status(&self) -> Status {
        let live: usize = self.index.values().chain(self.vectors.values()).map(|head| head.total).sum();
        let bytes = &self.map[..self.committed];
        let mut chunks = 0;
        let mut vectors = 0;
        for (path, head) in &self.index {
            let Some(record) = record::read(bytes, head.offset) else { continue };
            let Some(record) = self.held(path, record.size, record.modified) else { continue };
            chunks += record.chunk_count();
            vectors += record.vector_count();
        }
        Status {
            path: self.path.clone(),
            files: self.index.len(),
            chunks,
            vectors,
            bytes: self.committed as u64,
            dead_bytes: (self.committed - HEADER - live) as u64,
        }
    }

    /// Puts record heads into the index in file order. A clear record empties it. A text record replaces the
    /// path's text record and drops its vectors, which were made for the version before. A vectors record is
    /// kept for the path.
    fn absorb(&mut self, heads: Vec<Head>) {
        for head in heads {
            if head.kind == record::KIND_CLEAR {
                self.index.clear();
                self.vectors.clear();
                continue;
            }
            if head.kind == record::KIND_VECTORS {
                self.vectors.insert(head.path.clone(), head);
                continue;
            }
            self.vectors.remove(&head.path);
            self.index.insert(head.path.clone(), head);
        }
    }

    /// Takes the exclusive lock, opening the store again when a compaction replaced the file meanwhile, and
    /// reads in what other processes appended since this one last looked.
    fn lock(&mut self) -> Result<(), StoreError> {
        let mut header = [0u8; HEADER];
        loop {
            self.file.lock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            (&self.file).seek(SeekFrom::Start(0)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
            (&self.file).read_exact(&mut header).map_err(|error| StoreError::Io(self.path.clone(), error))?;
            if &header[0..8] != DEAD {
                break;
            }
            // A compaction replaced the file while this process waited: open the new one and lock that.
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            *self = Store::open(&self.root)?;
        }
        let length = self.file.metadata().map_err(|error| StoreError::Io(self.path.clone(), error))?.len();
        let committed = record::u64_at(&header, 16).unwrap_or(0).min(length) as usize;
        if committed <= self.committed {
            return Ok(());
        }
        // SAFETY: as in `open`; the file only grew.
        self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
        let committed = committed.min(self.map.len());
        let (heads, end) = record::scan(&self.map[..committed], self.committed);
        self.absorb(heads);
        self.committed = end;
        Ok(())
    }

    /// Appends one record under the lock the caller holds, and returns its offset. Copies the live records into
    /// a fresh file first when dead bytes are more than half the file.
    fn append(&mut self, bytes: &[u8]) -> Result<usize, StoreError> {
        let live: usize = self.index.values().chain(self.vectors.values()).map(|head| head.total).sum();
        let dead = self.committed - HEADER - live;
        if dead * 2 > self.committed {
            let new_path = self.path.with_extension("new");
            let new_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&new_path)
                .map_err(|error| StoreError::Io(new_path.clone(), error))?;
            // Locked before it takes the store's name, so a process that opens the new file waits for this one.
            new_file.lock().map_err(|error| StoreError::Io(new_path.clone(), error))?;
            let mut heads: Vec<&Head> = self.index.values().chain(self.vectors.values()).collect();
            heads.sort_by_key(|head| head.offset);
            let mut copied = Vec::with_capacity(HEADER + live);
            copied.extend_from_slice(MAGIC);
            copied.extend_from_slice(&VERSION.to_le_bytes());
            copied.resize(HEADER, 0);
            let mut moved = Vec::with_capacity(heads.len());
            for head in heads {
                let offset = copied.len();
                copied.extend_from_slice(&self.map[head.offset..head.offset + head.total]);
                moved.push(Head { offset, ..head.clone() });
            }
            let end = copied.len();
            copied[16..24].copy_from_slice(&(end as u64).to_le_bytes());
            (&new_file).write_all(&copied).map_err(|error| StoreError::Io(new_path.clone(), error))?;
            (&new_file).flush().map_err(|error| StoreError::Io(new_path.clone(), error))?;
            // SAFETY: as in `open`.
            let new_map = unsafe { Mmap::map(&new_file) }.map_err(|error| StoreError::Io(new_path.clone(), error))?;
            // This process's own mapping of the old file is dropped first, so the file can be replaced on Windows.
            self.map = new_map;
            let renamed = fs::rename(&new_path, &self.path);
            if renamed.is_err() {
                // Another process still maps the old file. Keep appending to it; a later writer tries again.
                let _ = fs::remove_file(&new_path);
                // SAFETY: as in `open`.
                self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
            }
            if renamed.is_ok() {
                (&self.file).seek(SeekFrom::Start(0)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
                (&self.file).write_all(DEAD).map_err(|error| StoreError::Io(self.path.clone(), error))?;
                (&self.file).flush().map_err(|error| StoreError::Io(self.path.clone(), error))?;
                let old = std::mem::replace(&mut self.file, new_file);
                old.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
                self.index.clear();
                self.vectors.clear();
                self.absorb(moved);
                self.committed = end;
            }
        }
        let offset = self.committed;
        (&self.file).seek(SeekFrom::Start(offset as u64)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(bytes).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        let end = offset + bytes.len();
        (&self.file).seek(SeekFrom::Start(16)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(&(end as u64).to_le_bytes()).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).flush().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        // SAFETY: as in `open`.
        self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
        self.committed = end;
        Ok(offset)
    }
}

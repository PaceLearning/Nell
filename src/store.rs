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
    index: HashMap<String, Head>,
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

impl Store {
    /// Opens the store of the repository at `root`, creating it when there is none. The store file sits in the
    /// user's local data folder, or the folder NELL_HOME names, and is named from a hash of `root`.
    pub fn open(root: &Path) -> Result<Store, StoreError> {
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
            let mut index = HashMap::new();
            for head in heads {
                if head.kind == record::KIND_CLEAR {
                    index.clear();
                    continue;
                }
                index.insert(head.path.clone(), head);
            }
            return Ok(Store { path, root: root.to_path_buf(), file, map, committed: end, index });
        }
        Err(StoreError::BadFormat(path))
    }

    /// The repository root this store belongs to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The latest record of `path`, when it was stored with the same size and modified time.
    pub fn held(&self, path: &str, size: u64, modified: Modified) -> Option<Record<'_>> {
        let head = self.index.get(path)?;
        let record = record::read(&self.map[..self.committed], head.offset)?;
        if record.size != size || record.modified != modified {
            return None;
        }
        Some(record)
    }

    /// Stores a version of `path`, under the lock. Does nothing when another process stored the same version
    /// first. Copies the live records into a fresh file first when dead bytes are more than half the file.
    pub fn put(&mut self, path: &str, size: u64, modified: Modified, text: &str) -> Result<(), StoreError> {
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
        // Other processes may have appended since this one mapped the file.
        let length = self.file.metadata().map_err(|error| StoreError::Io(self.path.clone(), error))?.len();
        let committed = record::u64_at(&header, 16).unwrap_or(0).min(length) as usize;
        if committed > self.committed {
            // SAFETY: as in `open`; the file only grew.
            self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
            let committed = committed.min(self.map.len());
            let (heads, end) = record::scan(&self.map[..committed], self.committed);
            for head in heads {
                if head.kind == record::KIND_CLEAR {
                    self.index.clear();
                    continue;
                }
                self.index.insert(head.path.clone(), head);
            }
            self.committed = end;
        }
        if self.held(path, size, modified).is_some() {
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            return Ok(());
        }
        let live: usize = self.index.values().map(|head| head.total).sum();
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
            let mut heads: Vec<&Head> = self.index.values().collect();
            heads.sort_by_key(|head| head.offset);
            let mut bytes = Vec::with_capacity(HEADER + live);
            bytes.extend_from_slice(&header);
            let mut index = HashMap::new();
            for head in heads {
                let offset = bytes.len();
                bytes.extend_from_slice(&self.map[head.offset..head.offset + head.total]);
                index.insert(head.path.clone(), Head { offset, ..head.clone() });
            }
            let end = bytes.len();
            bytes[16..24].copy_from_slice(&(end as u64).to_le_bytes());
            (&new_file).write_all(&bytes).map_err(|error| StoreError::Io(new_path.clone(), error))?;
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
                self.index = index;
                self.committed = end;
            }
        }
        let bytes = record::encode(path, size, modified, text);
        let offset = self.committed;
        (&self.file).seek(SeekFrom::Start(offset as u64)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(&bytes).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        let end = offset + bytes.len();
        (&self.file).seek(SeekFrom::Start(16)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(&(end as u64).to_le_bytes()).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).flush().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        // SAFETY: as in `open`.
        self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
        let head = Head { kind: record::KIND_TEXT, path: path.to_string(), offset, total: bytes.len() };
        self.index.insert(path.to_string(), head);
        self.committed = end;
        self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        Ok(())
    }

    /// Starts the store over, under the lock, by appending a record that marks every record before it as dead.
    /// Processes that have the store open see it at their next write. The dead bytes go at the next compaction.
    pub fn clear(&mut self) -> Result<(), StoreError> {
        let mut header = [0u8; HEADER];
        loop {
            self.file.lock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            (&self.file).seek(SeekFrom::Start(0)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
            (&self.file).read_exact(&mut header).map_err(|error| StoreError::Io(self.path.clone(), error))?;
            if &header[0..8] != DEAD {
                break;
            }
            self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
            *self = Store::open(&self.root)?;
        }
        let length = self.file.metadata().map_err(|error| StoreError::Io(self.path.clone(), error))?.len();
        let offset = (record::u64_at(&header, 16).unwrap_or(0).min(length) as usize).max(HEADER);
        let bytes = record::encode_clear();
        (&self.file).seek(SeekFrom::Start(offset as u64)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(&bytes).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        let end = offset + bytes.len();
        (&self.file).seek(SeekFrom::Start(16)).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).write_all(&(end as u64).to_le_bytes()).map_err(|error| StoreError::Io(self.path.clone(), error))?;
        (&self.file).flush().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        // SAFETY: as in `open`.
        self.map = unsafe { Mmap::map(&self.file) }.map_err(|error| StoreError::Io(self.path.clone(), error))?;
        self.index.clear();
        self.committed = end;
        self.file.unlock().map_err(|error| StoreError::Io(self.path.clone(), error))?;
        Ok(())
    }

    /// The store file, how many files and chunks it holds, its committed length and its dead bytes.
    pub fn status(&self) -> Status {
        let live: usize = self.index.values().map(|head| head.total).sum();
        let bytes = &self.map[..self.committed];
        let records = self.index.values().filter_map(|head| record::read(bytes, head.offset));
        let chunks = records.map(|record| record.chunk_count()).sum();
        Status {
            path: self.path.clone(),
            files: self.index.len(),
            chunks,
            bytes: self.committed as u64,
            dead_bytes: (self.committed - HEADER - live) as u64,
        }
    }
}

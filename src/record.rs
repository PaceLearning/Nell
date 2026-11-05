//! The bytes of one store record.
//!
//! A record is a kind byte, a length, and a body. The length counts the body, so a reader that does not know a
//! kind can skip it. Kind 1 is a file's text: the path relative to the repository root, the size and modified
//! time the file had on disk, the text, and the start of every line. All numbers are little endian.

use std::time::{SystemTime, UNIX_EPOCH};

/// The kind byte of a record that holds a file's text.
pub const KIND_TEXT: u8 = 1;

/// The bytes before a record's body: the kind and the length.
pub const HEAD: usize = 5;

/// A file's modified time as the store keeps it: whole seconds since 1970 and the nanoseconds past them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modified {
    pub secs: u64,
    pub nanos: u32,
}

impl Modified {
    /// Converts a system time. A time before 1970 becomes zero.
    pub fn create(time: SystemTime) -> Modified {
        let Ok(since) = time.duration_since(UNIX_EPOCH) else {
            return Modified { secs: 0, nanos: 0 };
        };
        Modified { secs: since.as_secs(), nanos: since.subsec_nanos() }
    }
}

/// Where one record sits in the store: its path, its offset from the start of the file, and its whole length.
#[derive(Clone, Debug)]
pub struct Head {
    pub path: String,
    pub offset: usize,
    pub total: usize,
}

/// A text record read from the store, borrowing the mapped bytes. The path is the index key that found it.
pub struct Record<'a> {
    pub size: u64,
    pub modified: Modified,
    pub text: &'a str,
    starts: &'a [u8],
}

impl<'a> Record<'a> {
    /// The lines of the text, the way `str::lines` gives them, read from the stored line starts.
    pub fn lines(&self) -> Lines<'a> {
        Lines { text: self.text, starts: self.starts, index: 0 }
    }
}

/// Walks the lines of a stored text from its line starts.
pub struct Lines<'a> {
    text: &'a str,
    starts: &'a [u8],
    index: usize,
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let start = u32_at(self.starts, self.index * 4)? as usize;
        let mut end = self.text.len();
        if let Some(next) = u32_at(self.starts, (self.index + 1) * 4) {
            end = next as usize;
        }
        self.index += 1;
        let full = self.text.get(start..end)?;
        let Some(cut) = full.strip_suffix('\n') else {
            return Some(full);
        };
        Some(cut.strip_suffix('\r').unwrap_or(cut))
    }
}

/// Reads a little endian u32 at `offset`, or nothing when the bytes end before it.
pub fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

/// Reads a little endian u64 at `offset`, or nothing when the bytes end before it.
pub fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

/// Builds the bytes of a text record, line starts included.
pub fn encode(path: &str, size: u64, modified: Modified, text: &str) -> Vec<u8> {
    let mut starts: Vec<u32> = Vec::new();
    if !text.is_empty() {
        starts.push(0);
    }
    for (index, byte) in text.bytes().enumerate() {
        if byte != b'\n' || index + 1 >= text.len() {
            continue;
        }
        starts.push((index + 1) as u32);
    }
    let body = 4 + path.len() + 8 + 8 + 4 + 4 + text.len() + 4 + starts.len() * 4;
    let mut bytes = Vec::with_capacity(HEAD + body);
    bytes.push(KIND_TEXT);
    bytes.extend_from_slice(&(body as u32).to_le_bytes());
    bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&modified.secs.to_le_bytes());
    bytes.extend_from_slice(&modified.nanos.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(&(starts.len() as u32).to_le_bytes());
    for start in starts {
        bytes.extend_from_slice(&start.to_le_bytes());
    }
    bytes
}

/// Walks the record heads from `from` to the end of `bytes`, which must end at the committed length. Returns the
/// heads in file order, every kind included, and the offset where a whole record no longer fit, which is the
/// end of the bytes when nothing is cut short.
pub fn scan(bytes: &[u8], from: usize) -> (Vec<Head>, usize) {
    let mut heads = Vec::new();
    let mut offset = from;
    while let Some(length) = u32_at(bytes, offset + 1) {
        let total = HEAD + length as usize;
        if offset + total > bytes.len() {
            break;
        }
        let Some(path_len) = u32_at(bytes, offset + HEAD) else { break };
        let Some(path) = bytes.get(offset + HEAD + 4..offset + HEAD + 4 + path_len as usize) else { break };
        let Ok(path) = std::str::from_utf8(path) else { break };
        heads.push(Head { path: path.to_string(), offset, total });
        offset += total;
    }
    (heads, offset)
}

/// Reads the text record at `offset`, or nothing when the bytes there are not a whole text record.
pub fn read(bytes: &[u8], offset: usize) -> Option<Record<'_>> {
    if *bytes.get(offset)? != KIND_TEXT {
        return None;
    }
    let length = u32_at(bytes, offset + 1)? as usize;
    let body = bytes.get(offset + HEAD..offset + HEAD + length)?;
    let path_len = u32_at(body, 0)? as usize;
    std::str::from_utf8(body.get(4..4 + path_len)?).ok()?;
    let at = 4 + path_len;
    let size = u64_at(body, at)?;
    let secs = u64_at(body, at + 8)?;
    let nanos = u32_at(body, at + 16)?;
    let text_len = u32_at(body, at + 20)? as usize;
    let text = std::str::from_utf8(body.get(at + 24..at + 24 + text_len)?).ok()?;
    let at = at + 24 + text_len;
    let count = u32_at(body, at)? as usize;
    let starts = body.get(at + 4..at + 4 + count * 4)?;
    Some(Record { size, modified: Modified { secs, nanos }, text, starts })
}

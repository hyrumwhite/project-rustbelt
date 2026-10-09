//! Reader for Halo: Combat Evolved Xbox cache files (`maps/*.map`).
//!
//! An Xbox cache file is a 2 KiB header followed by a zlib stream. Once
//! inflated, the tag data is a memory image the game loads at
//! [`TAG_CACHE_BASE`]; each structure BSP is a separate image loaded at its own
//! base address when the game switches to it. Pointers inside tags are those
//! 32-bit Xbox addresses, so every read goes through a [`Ptr`] that knows
//! which image (a [`Space`]) it is in.
//!
//! Struct offsets come from [`layout`], which is generated from OpenCE's C
//! definitions by `tools/tag_layout`.

pub mod bitmap;
pub mod bsp;
pub mod combiner;
mod cache;
pub mod layout;
pub mod math;
pub mod model;
pub mod objects;
mod palette;
pub mod scenario;
pub mod shader;
pub mod sky;

pub use cache::{Block, CacheFile, Header, Ptr, Space, TagData, TagInstance, TagRef, TAG_CACHE_BASE};

use std::fmt;

/// A four-character code (`b"sbsp"`) as the game stores it: a big-endian
/// integer written little-endian.
pub const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// The printable form of a four-character code.
pub fn fourcc_str(code: u32) -> String {
    code.to_be_bytes()
        .iter()
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '?' })
        .collect()
}

pub mod groups {
    use super::fourcc;

    pub const SCENARIO: u32 = fourcc(b"scnr");
    pub const STRUCTURE_BSP: u32 = fourcc(b"sbsp");
    pub const BITMAP: u32 = fourcc(b"bitm");
    pub const SHADER: u32 = fourcc(b"shdr");
    pub const SHADER_ENVIRONMENT: u32 = fourcc(b"senv");
    pub const SHADER_MODEL: u32 = fourcc(b"soso");
    pub const SKY: u32 = fourcc(b"sky ");
    pub const MODEL: u32 = fourcc(b"mode");
    /// The parent of every object group (`scen`, `vehi`, `weap`, ...).
    pub const OBJECT: u32 = fourcc(b"obje");
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file is not an Xbox cache file this reader understands.
    Format(String),
    /// A pointer, block or data field leaves its image.
    OutOfBounds { what: &'static str, address: u32, len: usize },
    /// A tag index that names no tag, or a tag of an unexpected group.
    BadTag(String),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::Format(s) => write!(f, "not a valid cache file: {s}"),
            Error::OutOfBounds { what, address, len } => {
                write!(f, "{what} at {address:#010x} (+{len:#x}) is outside its tag data")
            }
            Error::BadTag(s) => write!(f, "bad tag: {s}"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

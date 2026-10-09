//! The cache file itself: header, decompression, the tag index and typed
//! reads through Xbox addresses.

use crate::{fourcc, layout, Error, Result};
use std::{io::Read, path::Path};

/// Where the Xbox game loads a cache file's tag data
/// (`physical_memory_map.c`, `TAG_CACHE_BASE_ADDRESS`).
pub const TAG_CACHE_BASE: u32 = 0x803A_6000;

const HEADER_SIZE: usize = 0x800;
const HEADER_SIGNATURE: u32 = fourcc(b"head");
const FOOTER_SIGNATURE: u32 = fourcc(b"foot");
const TAGS_SIGNATURE: u32 = fourcc(b"tags");
/// The Xbox cache file version (Custom Edition's is 609).
const XBOX_VERSION: i32 = 5;

/// `struct cache_file_header` (cache_files.c), the file's first 2 KiB.
#[derive(Debug, Clone)]
pub struct Header {
    pub version: i32,
    /// The inflated file's length, header included.
    pub file_length: usize,
    pub tag_data_offset: usize,
    pub tag_data_size: usize,
    pub name: String,
    pub build: String,
    pub checksum: u32,
}

/// One of a cache file's memory images: `size` bytes at file offset
/// `file_offset`, which the game loads at Xbox address `base`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub base: u32,
    pub file_offset: usize,
    pub size: usize,
}

impl Space {
    /// The file offset of `len` bytes at `address`, if they are all inside.
    fn resolve(&self, address: u32, len: usize) -> Option<usize> {
        let relative = address.checked_sub(self.base)? as usize;
        (relative.checked_add(len)? <= self.size).then_some(self.file_offset + relative)
    }
}

/// `struct cache_file_tag_instance` (cache_files.c).
#[derive(Debug, Clone)]
pub struct TagInstance {
    pub group: u32,
    pub parent_groups: [u32; 2],
    /// The tag's datum index (absolute index in the low 16 bits, salt above).
    pub datum: u32,
    pub name: String,
    /// Xbox address of the tag's root structure; 0 for structure BSPs, which
    /// are only placed when loaded (see [`crate::bsp`]).
    pub base_address: u32,
}

impl TagInstance {
    pub fn is_a(&self, group: u32) -> bool {
        self.group == group || self.parent_groups.contains(&group)
    }
}

/// `struct tag_reference`: a tag by group and datum index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagRef {
    pub group: u32,
    /// `None` when the reference is empty (NONE).
    pub datum: Option<u32>,
}

/// The inflated cache file and its tag index.
pub struct CacheFile {
    data: Vec<u8>,
    pub header: Header,
    pub tags: Vec<TagInstance>,
    pub scenario_datum: u32,
    /// The tag data image.
    pub tag_space: Space,
}

impl CacheFile {
    /// Reads and inflates an Xbox cache file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(raw: Vec<u8>) -> Result<Self> {
        if raw.len() < HEADER_SIZE {
            return Err(Error::Format("shorter than its header".into()));
        }
        let header = parse_header(&raw[..HEADER_SIZE])?;

        // Everything after the header is one zlib stream
        // (cache_files_decompress_windows.c).
        let mut data = Vec::with_capacity(header.file_length);
        data.extend_from_slice(&raw[..HEADER_SIZE]);
        flate2::read::ZlibDecoder::new(&raw[HEADER_SIZE..]).read_to_end(&mut data)?;
        drop(raw);
        if data.len() < header.file_length {
            return Err(Error::Format(format!(
                "inflated to {:#x} bytes; the header says {:#x}",
                data.len(),
                header.file_length
            )));
        }

        let tag_space = Space {
            base: TAG_CACHE_BASE,
            file_offset: header.tag_data_offset,
            size: header.tag_data_size,
        };
        if tag_space.file_offset.checked_add(tag_space.size).is_none_or(|end| end > data.len()) {
            return Err(Error::Format("tag data leaves the file".into()));
        }

        let mut cache = CacheFile { data, header, tags: Vec::new(), scenario_datum: 0, tag_space };
        cache.read_tag_index()?;
        Ok(cache)
    }

    /// `struct cache_file_tag_header`, at the start of the tag data.
    fn read_tag_index(&mut self) -> Result<()> {
        let root = self.root(self.tag_space, TAG_CACHE_BASE)?;
        let instances_address = root.u32(0x0)?;
        let scenario_datum = root.u32(0x4)?;
        let tag_count = root.i32(0xC)?;
        let signature = root.u32(0x20)?;
        if signature != TAGS_SIGNATURE {
            return Err(Error::Format(format!("tag header signature {signature:#x}")));
        }
        if !(0..=0xFFFF).contains(&tag_count) {
            return Err(Error::Format(format!("{tag_count} tags")));
        }

        const INSTANCE_SIZE: usize = 0x20;
        let instances = root.at(instances_address)?;
        let mut tags = Vec::with_capacity(tag_count as usize);
        for i in 0..tag_count as usize {
            let instance = instances.add(i * INSTANCE_SIZE);
            let name_address = instance.u32(0x10)?;
            tags.push(TagInstance {
                group: instance.u32(0x0)?,
                parent_groups: [instance.u32(0x4)?, instance.u32(0x8)?],
                datum: instance.u32(0xC)?,
                name: root.at(name_address)?.c_string(256)?,
                base_address: instance.u32(0x14)?,
            });
        }
        self.tags = tags;
        self.scenario_datum = scenario_datum;
        Ok(())
    }

    /// The whole inflated file.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// `len` bytes at a file offset (bitmap pixels and other data the tags
    /// locate by file offset rather than by address).
    pub fn file_bytes(&self, offset: usize, len: usize) -> Result<&[u8]> {
        offset
            .checked_add(len)
            .and_then(|end| self.data.get(offset..end))
            .ok_or(Error::OutOfBounds { what: "file data", address: offset as u32, len })
    }

    /// A pointer to `address` in `space`.
    pub fn root(&self, space: Space, address: u32) -> Result<Ptr<'_>> {
        let offset = space.resolve(address, 0).ok_or(Error::OutOfBounds { what: "pointer", address, len: 0 })?;
        Ok(Ptr { cache: self, space, offset })
    }

    /// The tag with this datum index.
    pub fn tag(&self, datum: u32) -> Result<&TagInstance> {
        let tag = self
            .tags
            .get((datum & 0xFFFF) as usize)
            .ok_or_else(|| Error::BadTag(format!("no tag {datum:#010x}")))?;
        if tag.datum != datum {
            return Err(Error::BadTag(format!("{datum:#010x} is stale (tag is {:#010x})", tag.datum)));
        }
        Ok(tag)
    }

    /// The root structure of a tag that lives in the tag data, checked to be
    /// of `group` (or a child group of it).
    pub fn tag_root(&self, datum: u32, group: u32) -> Result<Ptr<'_>> {
        let tag = self.tag(datum)?;
        if !tag.is_a(group) {
            return Err(Error::BadTag(format!(
                "{} is a {}, not a {}",
                tag.name,
                crate::fourcc_str(tag.group),
                crate::fourcc_str(group)
            )));
        }
        if tag.base_address == 0 {
            return Err(Error::BadTag(format!("{} has no data in the tag cache", tag.name)));
        }
        self.root(self.tag_space, tag.base_address)
    }

    pub fn scenario(&self) -> Result<Ptr<'_>> {
        self.tag_root(self.scenario_datum, crate::groups::SCENARIO)
    }

    pub fn find_tag(&self, group: u32, name: &str) -> Option<&TagInstance> {
        self.tags.iter().find(|t| t.group == group && t.name.eq_ignore_ascii_case(name))
    }
}

fn parse_header(raw: &[u8]) -> Result<Header> {
    let u32_at = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let string_at = |o: usize| {
        let field = &raw[o..o + 0x20];
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..end]).into_owned()
    };
    if u32_at(0x0) != HEADER_SIGNATURE || u32_at(0x7FC) != FOOTER_SIGNATURE {
        return Err(Error::Format("no head/foot signatures".into()));
    }
    let version = u32_at(0x4) as i32;
    if version != XBOX_VERSION {
        return Err(Error::Format(format!("version {version}; only Xbox (5) cache files are supported")));
    }
    Ok(Header {
        version,
        file_length: u32_at(0x8) as usize,
        tag_data_offset: u32_at(0x10) as usize,
        tag_data_size: u32_at(0x14) as usize,
        name: string_at(0x20),
        build: string_at(0x40),
        checksum: u32_at(0x64),
    })
}

/// A position in one of the cache file's images. Reads are relative to it
/// and bounds-checked against the image.
#[derive(Clone, Copy)]
pub struct Ptr<'a> {
    cache: &'a CacheFile,
    space: Space,
    /// File offset.
    offset: usize,
}

impl<'a> Ptr<'a> {
    pub fn cache(&self) -> &'a CacheFile {
        self.cache
    }

    pub fn space(&self) -> Space {
        self.space
    }

    /// This position's Xbox address.
    pub fn address(&self) -> u32 {
        self.space.base + (self.offset - self.space.file_offset) as u32
    }

    /// `rel` bytes further on (checked when read).
    pub fn add(&self, rel: usize) -> Ptr<'a> {
        Ptr { offset: self.offset + rel, ..*self }
    }

    /// The same image at another address.
    pub fn at(&self, address: u32) -> Result<Ptr<'a>> {
        self.cache.root(self.space, address)
    }

    pub fn bytes(&self, rel: usize, len: usize) -> Result<&'a [u8]> {
        let start = self.offset + rel;
        let end = self.space.file_offset + self.space.size;
        if start.checked_add(len).is_none_or(|e| e > end) {
            return Err(Error::OutOfBounds { what: "field", address: self.address().wrapping_add(rel as u32), len });
        }
        Ok(&self.cache.data[start..start + len])
    }

    fn array<const N: usize>(&self, rel: usize) -> Result<[u8; N]> {
        Ok(self.bytes(rel, N)?.try_into().unwrap())
    }

    pub fn u8(&self, rel: usize) -> Result<u8> {
        Ok(self.array::<1>(rel)?[0])
    }
    pub fn u16(&self, rel: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array(rel)?))
    }
    pub fn i16(&self, rel: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array(rel)?))
    }
    pub fn u32(&self, rel: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(rel)?))
    }
    pub fn i32(&self, rel: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array(rel)?))
    }
    pub fn f32(&self, rel: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array(rel)?))
    }
    pub fn vec2(&self, rel: usize) -> Result<[f32; 2]> {
        Ok([self.f32(rel)?, self.f32(rel + 4)?])
    }
    pub fn vec3(&self, rel: usize) -> Result<[f32; 3]> {
        Ok([self.f32(rel)?, self.f32(rel + 4)?, self.f32(rel + 8)?])
    }

    /// A NUL-terminated string of at most `max` bytes here.
    pub fn c_string(&self, max: usize) -> Result<String> {
        let end = self.space.file_offset + self.space.size;
        let available = end.saturating_sub(self.offset).min(max);
        let bytes = self.bytes(0, available)?;
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..len]).into_owned())
    }

    /// The `struct tag_block` at `rel`, whose elements are `element_size`
    /// bytes each.
    pub fn block(&self, rel: usize, element_size: usize) -> Result<Block<'a>> {
        let count = self.i32(rel + layout::tag_block::COUNT)?;
        let address = self.u32(rel + layout::tag_block::ADDRESS)?;
        if count <= 0 {
            return Ok(Block { first: None, count: 0, element_size });
        }
        let count = count as usize;
        let len = count.checked_mul(element_size).ok_or(Error::OutOfBounds { what: "tag block", address, len: usize::MAX })?;
        let first = self.at(address)?;
        first.bytes(0, len).map_err(|_| Error::OutOfBounds { what: "tag block", address, len })?;
        Ok(Block { first: Some(first), count, element_size })
    }

    /// The `struct tag_reference` at `rel`.
    pub fn reference(&self, rel: usize) -> Result<TagRef> {
        let group = self.u32(rel + layout::tag_reference::GROUP_TAG)?;
        let index = self.u32(rel + layout::tag_reference::INDEX)?;
        Ok(TagRef { group, datum: (index != u32::MAX).then_some(index) })
    }

    /// The `struct tag_data` at `rel`.
    pub fn data(&self, rel: usize) -> Result<TagData> {
        Ok(TagData {
            size: self.i32(rel + layout::tag_data::SIZE)?.max(0) as usize,
            file_offset: self.u32(rel + layout::tag_data::FILE_OFFSET)? as usize,
            address: self.u32(rel + layout::tag_data::ADDRESS)?,
        })
    }

    /// The bytes of the `struct tag_data` at `rel`, which live in this image.
    pub fn data_bytes(&self, rel: usize) -> Result<&'a [u8]> {
        let data = self.data(rel)?;
        if data.size == 0 {
            return Ok(&[]);
        }
        self.at(data.address)?
            .bytes(0, data.size)
            .map_err(|_| Error::OutOfBounds { what: "tag data", address: data.address, len: data.size })
    }
}

/// `struct tag_data`.
#[derive(Debug, Clone, Copy)]
pub struct TagData {
    pub size: usize,
    pub file_offset: usize,
    pub address: u32,
}

/// A `struct tag_block`'s elements.
#[derive(Clone, Copy)]
pub struct Block<'a> {
    first: Option<Ptr<'a>>,
    pub count: usize,
    element_size: usize,
}

impl<'a> Block<'a> {
    pub fn get(&self, index: usize) -> Option<Ptr<'a>> {
        (index < self.count).then(|| self.first.unwrap().add(index * self.element_size))
    }

    pub fn iter(&self) -> impl Iterator<Item = Ptr<'a>> + '_ {
        (0..self.count).map(|i| self.get(i).unwrap())
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

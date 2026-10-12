//! Halo's packet codec (memory/data_packets.c, data_encoding.c and
//! data_packet_groups.c): a packet's struct, described by a list of fields
//! (bytes, shorts, longs, 64-bit integers, strings, sized data, arrays of
//! nested fields, padding), encoded to the wire big-endian with its version
//! byte first and its type's header byte last, and decoded back.
//!
//! The game's messages (network_messages.c) and its key agreement go
//! through it. This is the C's walk exactly: its fields' sizes as
//! `_data_packet_verify` sets them (a field the definition's version leaves
//! out keeps the size before it), its encoding of fields the packet's
//! version leaves out (their count of zero bytes, an empty string, a zero
//! count), and decoding's byte swap of the received buffer in place.

/// data_packet_groups.h's `enum data_packet_field_type`.
pub mod field_type {
    pub const PAD: i16 = 0;
    pub const BYTES: i16 = 1;
    pub const SHORTS: i16 = 2;
    pub const LONGS: i16 = 3;
    pub const INT64S: i16 = 4;
    pub const STRING: i16 = 5;
    pub const DATA: i16 = 6;
    pub const ARRAY: i16 = 7;
    pub const RAW: i16 = 8;
    pub const END: i16 = 9;
    pub const COUNT: i16 = 10;
}

/// `struct data_packet_field` (its size is the codec's own, [`sizes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub kind: i16,
    pub count: i16,
    pub minimum_version: i16,
    pub maximum_version: i16,
}

/// `struct data_packet_definition`: its version and its fields, through the
/// top level's end (nested arrays' fields and ends among them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub size: i16,
    pub version: i16,
    pub fields: Vec<Field>,
}

/// `struct data_packet_entry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub packet_class: i16,
    pub definition: Option<Definition>,
}

/// `struct data_packet_group_definition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub packet_class_count: i16,
    pub maximum_decoded_packet_size: i32,
    pub maximum_encoded_packet_size: i32,
    pub packets: Vec<Entry>,
}

/// Why a group's encoding or decoding failed (its error string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// "couldn't append header to encoded packet"
    AppendHeader = 1,
    /// "couldn't encode packet"
    Encode = 2,
    /// "got packet with no header"
    NoHeader = 3,
    /// "got packet with bad type"
    BadType = 4,
    /// "got packet with mismatched class"
    MismatchedClass = 5,
    /// "got packet which wouldn't decode"
    WouldNotDecode = 6,
}

fn eligible(field: &Field, version: i16) -> bool {
    version >= field.minimum_version && (version <= field.maximum_version || field.maximum_version == 0)
}

/// `_data_packet_verify` from the field `first`: each field's size into
/// `sizes`, and the fields' total size and count (their end's included).
fn verify(definition: &Definition, sizes: &mut [i16], first: usize) -> (i16, i16) {
    let fields = &definition.fields;
    let mut index = first;
    let mut total_size: i16 = 0;
    // (the C's is indeterminate until an eligible field sets it)
    let mut field_size: i16 = 0;
    while fields[index].kind != field_type::END {
        let field = fields[index];
        if eligible(&field, definition.version) {
            field_size = match field.kind {
                field_type::PAD | field_type::BYTES | field_type::RAW => field.count,
                field_type::SHORTS => field.count.wrapping_mul(2),
                field_type::LONGS => field.count.wrapping_mul(4),
                field_type::INT64S => field.count.wrapping_mul(8),
                field_type::STRING => field.count.wrapping_add(1),
                field_type::DATA => field.count.wrapping_add(2),
                field_type::ARRAY => {
                    let (element_size, element_field_count) = verify(definition, sizes, index + 1);
                    index += element_field_count as usize;
                    2i16.wrapping_add(field.count.wrapping_mul(element_size))
                }
                _ => 0,
            };
        }
        sizes[index] = field_size;
        total_size = total_size.wrapping_add(field_size);
        index += 1;
    }
    (total_size, (index - first + 1) as i16)
}

/// The fields' sizes as the definition's verification leaves them.
pub fn sizes(definition: &Definition) -> Vec<i16> {
    let mut sizes = vec![0; definition.fields.len()];
    verify(definition, &mut sizes, 0);
    sizes
}

/// `struct data_encoding_state`.
struct State<'a> {
    buffer: &'a mut [u8],
    offset: i32,
    overflow: bool,
}

/// The size of `count` elements of a size code (1, or -2, -4, -8 swapped).
fn memory_size(count: i16, element_size: i32) -> i32 {
    match element_size {
        -2 => (count as i32) << 1,
        -4 => (count as i32) << 2,
        -8 => (count as i32) << 3,
        _ => count as i32,
    }
}

/// `byte_swap_memory`.
fn byte_swap(memory: &mut [u8], element_size: i32) {
    let width = (-element_size) as usize;
    for element in memory.chunks_exact_mut(width) {
        element.reverse();
    }
}

impl State<'_> {
    fn limit(&self) -> i32 {
        self.buffer.len() as i32
    }

    /// `data_encode_memory`: zeros for no source.
    fn encode_memory(&mut self, source: Option<&[u8]>, count: i16, element_size: i32) -> bool {
        let size = memory_size(count, element_size);
        if self.offset + size <= self.limit() && !self.overflow {
            let start = self.offset as usize;
            let destination = &mut self.buffer[start..start + size as usize];
            match source {
                Some(source) => destination.copy_from_slice(&source[..size as usize]),
                None => destination.fill(0),
            }
            if element_size != 1 {
                byte_swap(destination, element_size);
            }
            self.offset += size;
        } else {
            self.overflow = true;
        }
        !self.overflow
    }

    /// `data_encode_integer`.
    fn encode_integer(&mut self, value: i32, maximum_value: i32) {
        if maximum_value <= u8::MAX as i32 {
            if self.offset + 1 <= self.limit() && !self.overflow {
                self.buffer[self.offset as usize] = value as u8;
                self.offset += 1;
            } else {
                self.overflow = true;
            }
        } else if maximum_value <= u16::MAX as i32 {
            self.encode_memory(Some(&(value as i16).to_le_bytes()), 1, -2);
        } else {
            self.encode_memory(Some(&value.to_le_bytes()), 1, -4);
        }
    }

    /// `data_encode_string`: no more than `maximum_length` characters.
    fn encode_string(&mut self, string: &[u8], maximum_length: i16) {
        let limit = (maximum_length.max(0) as usize).min(string.len());
        let length = string[..limit].iter().position(|&c| c == 0).unwrap_or(limit) as i32;
        if self.offset + length + 1 <= self.limit() && !self.overflow {
            let start = self.offset as usize;
            self.buffer[start..start + length as usize].copy_from_slice(&string[..length as usize]);
            self.buffer[start + length as usize] = 0;
            self.offset += length + 1;
        } else {
            self.overflow = true;
        }
    }

    /// `data_decode_memory`: where the elements lie (swapped in place), or
    /// none past the buffer.
    fn decode_memory(&mut self, count: i16, element_size: i32) -> Option<usize> {
        let size = memory_size(count, element_size);
        if self.offset + size <= self.limit() && !self.overflow {
            let start = self.offset as usize;
            if element_size != 1 {
                byte_swap(&mut self.buffer[start..start + size as usize], element_size);
            }
            self.offset += size;
            Some(start)
        } else {
            self.overflow = true;
            None
        }
    }

    fn decode_byte(&mut self) -> u8 {
        self.decode_memory(1, 1).map_or(0, |at| self.buffer[at])
    }

    /// `data_decode_integer`.
    fn decode_integer(&mut self, maximum_value: i32) -> i32 {
        if maximum_value <= u8::MAX as i32 {
            return self.decode_byte() as i32;
        }
        if maximum_value <= u16::MAX as i32 {
            return self.decode_memory(1, -2).map_or(0, |at| i16::from_le_bytes([self.buffer[at], self.buffer[at + 1]]) as i32);
        }
        self.decode_memory(1, -4).map_or(0, |at| i32::from_le_bytes(self.buffer[at..at + 4].try_into().unwrap()))
    }

    /// `data_decode_string`: where the string lies and its length, or none
    /// without its terminator within `maximum_length` characters.
    fn decode_string(&mut self, maximum_length: u16) -> Option<(usize, usize)> {
        let start = self.offset as usize;
        let mut length: i16 = 0;
        while self.offset + (length as i32) < self.limit() && length <= maximum_length as i16 {
            if self.buffer[start + length as usize] == 0 {
                self.offset += length as i32 + 1;
                return Some((start, length as usize));
            }
            length += 1;
        }
        self.overflow = true;
        None
    }
}

fn read_short(memory: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([memory[at], memory[at + 1]])
}

/// `_data_packet_encode` from the field `first` of the decoded struct at
/// `decoded[start..]`: how far it reads, and its fields' count.
fn encode_fields(
    definition: &Definition,
    sizes: &mut [i16],
    state: &mut State<'_>,
    packet_version: i16,
    decoded: &[u8],
    start: usize,
    first: usize,
) -> (usize, i16) {
    let fields = &definition.fields;
    let mut index = first;
    let mut at = start;
    while fields[index].kind != field_type::END {
        let field = fields[index];
        let count = field.count;
        if eligible(&field, packet_version) {
            match field.kind {
                field_type::PAD => {}
                field_type::BYTES | field_type::RAW => {
                    state.encode_memory(Some(&decoded[at..]), count, 1);
                }
                field_type::SHORTS => {
                    state.encode_memory(Some(&decoded[at..]), count, -2);
                }
                field_type::LONGS => {
                    state.encode_memory(Some(&decoded[at..]), count, -4);
                }
                field_type::INT64S => {
                    state.encode_memory(Some(&decoded[at..]), count, -8);
                }
                field_type::STRING => state.encode_string(&decoded[at..], count),
                field_type::DATA => {
                    let mut data_size = read_short(decoded, at);
                    if data_size < 0 || data_size > count {
                        state.overflow = true;
                        data_size = 0;
                    }
                    state.encode_integer(data_size as i32, count as i32);
                    state.encode_memory(Some(&decoded[at + 2..]), data_size, 1);
                }
                field_type::ARRAY => {
                    let mut element_count = read_short(decoded, at);
                    let (_, element_field_count) = verify(definition, sizes, index + 1);
                    if element_count < 0 || element_count > count {
                        state.overflow = true;
                        element_count = 0;
                    }
                    state.encode_integer(element_count as i32, count as i32);
                    let mut element = at + 2;
                    while element_count > 0 {
                        element_count -= 1;
                        let (element_size, _) = encode_fields(definition, sizes, state, packet_version, decoded, element, index + 1);
                        element += element_size;
                    }
                    index += element_field_count as usize;
                }
                _ => {}
            }
        } else {
            match field.kind {
                field_type::PAD => {}
                field_type::STRING => {
                    state.encode_memory(Some(&[0]), 1, 1);
                }
                field_type::DATA | field_type::ARRAY => state.encode_integer(0, count as i32),
                _ => {
                    state.encode_memory(None, count, 1);
                }
            }
        }
        at = at.wrapping_add_signed(sizes[index] as isize);
        index += 1;
    }
    (at - start, (index - first + 1) as i16)
}

/// `_data_packet_decode` from the field `first` into `decoded[start..]`:
/// how far it writes.
fn decode_fields(
    definition: &Definition,
    sizes: &mut [i16],
    state: &mut State<'_>,
    packet_version: i16,
    decoded: &mut [u8],
    start: usize,
    first: usize,
) -> usize {
    let fields = &definition.fields;
    let mut index = first;
    let mut at = start;
    while fields[index].kind != field_type::END {
        let field = fields[index];
        let count = field.count;
        if eligible(&field, packet_version) {
            let copy = |state: &mut State<'_>, decoded: &mut [u8], element_size: i32| {
                if let Some(source) = state.decode_memory(count, element_size) {
                    let size = memory_size(count, element_size) as usize;
                    decoded[at..at + size].copy_from_slice(&state.buffer[source..source + size]);
                }
            };
            match field.kind {
                field_type::BYTES | field_type::RAW => copy(state, decoded, 1),
                field_type::SHORTS => copy(state, decoded, -2),
                field_type::LONGS => copy(state, decoded, -4),
                field_type::INT64S => copy(state, decoded, -8),
                field_type::STRING => {
                    if let Some((source, length)) = state.decode_string(count as u16) {
                        decoded[at..at + length + 1].copy_from_slice(&state.buffer[source..source + length + 1]);
                    }
                }
                field_type::DATA => {
                    let mut data_size = state.decode_integer(count as i32) as i16;
                    if data_size < 0 || data_size > count {
                        state.overflow = true;
                        data_size = 0;
                    }
                    decoded[at..at + 2].copy_from_slice(&data_size.to_le_bytes());
                    if let Some(source) = state.decode_memory(data_size, 1) {
                        let size = data_size as usize;
                        decoded[at + 2..at + 2 + size].copy_from_slice(&state.buffer[source..source + size]);
                    }
                }
                field_type::ARRAY => {
                    let mut element_count = state.decode_integer(count as i32) as i16;
                    let (_, element_field_count) = verify(definition, sizes, index + 1);
                    if element_count < 0 || element_count > count {
                        state.overflow = true;
                        element_count = 0;
                    }
                    decoded[at..at + 2].copy_from_slice(&element_count.to_le_bytes());
                    let mut element = at + 2;
                    while element_count > 0 {
                        element_count -= 1;
                        element += decode_fields(definition, sizes, state, packet_version, decoded, element, index + 1);
                    }
                    index += element_field_count as usize;
                }
                _ => {}
            }
        } else {
            let size = sizes[index].max(0) as usize;
            decoded[at..at + size].fill(0);
        }
        at = at.wrapping_add_signed(sizes[index] as isize);
        index += 1;
    }
    at - start
}

/// `data_packet_encode` into `buffer` (its length the most it takes): the
/// encoded size, and whether it fit.
pub fn encode(definition: &Definition, packet_version: i32, decoded: &[u8], buffer: &mut [u8]) -> (i16, bool) {
    let mut sizes = sizes(definition);
    let mut state = State { buffer, offset: 0, overflow: false };
    let mut version = packet_version as i16;
    if version == -1 {
        version = definition.version;
    }
    if definition.version > 0 {
        state.encode_memory(Some(&[version as u8]), 1, 1);
    }
    encode_fields(definition, &mut sizes, &mut state, version, decoded, 0, 0);
    (state.offset as i16, !state.overflow)
}

/// `data_packet_decode` of `encoded` (swapped in place as it's read) into
/// `decoded`: whether it decoded, and its version.
pub fn decode(definition: &Definition, encoded: &mut [u8], decoded: &mut [u8]) -> (bool, i16) {
    let mut sizes = sizes(definition);
    let mut state = State { buffer: encoded, offset: 0, overflow: false };
    let version = if definition.version == 0 { 0 } else { state.decode_byte() as i16 };
    let mut result = false;
    if version <= definition.version {
        decode_fields(definition, &mut sizes, &mut state, version, decoded, 0, 0);
        result = !state.overflow;
    }
    (result, version)
}

/// `data_packet_group_encode_packet` of the packet type into `buffer` (its
/// length the group's most): the encoded size (as the encoding left it),
/// and the error, if any.
pub fn group_encode(group: &Group, decoded: &[u8], buffer: &mut [u8], packet_type: i16, packet_version: i32) -> (i16, Option<Error>) {
    let Some(definition) = group.packets.get(packet_type as usize).and_then(|p| p.definition.as_ref()) else {
        return (0, Some(Error::Encode));
    };
    let maximum = (group.maximum_encoded_packet_size as i16).max(0) as usize;
    let limit = maximum.min(buffer.len());
    let (mut size, fitted) = encode(definition, packet_version, decoded, &mut buffer[..limit]);
    if !fitted {
        return (size, Some(Error::Encode));
    }
    // (data_packet_group_append_packet_header: its one byte, the type)
    if (size as i32) + 1 < group.maximum_encoded_packet_size {
        buffer[size as usize] = packet_type as u8;
        size += 1;
        (size, None)
    } else {
        (size, Some(Error::AppendHeader))
    }
}

/// What a group's decoding left: the encoded size, the packet's type and
/// version (as given where it didn't set them), and the error, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    pub encoded_size: i16,
    pub packet_type: i16,
    pub packet_version: i16,
    pub error: Option<Error>,
}

/// `data_packet_group_decode_packet` of the packet of `encoded_size` bytes
/// in `encoded` (swapped in place as it's read) into `decoded`.
pub fn group_decode(
    group: &Group,
    decoded: &mut [u8],
    encoded: &mut [u8],
    encoded_size: i16,
    packet_type: i16,
    packet_version: i16,
    expected_packet_class: i16,
) -> Decoded {
    let mut out = Decoded { encoded_size, packet_type, packet_version, error: None };
    if encoded_size < 1 {
        out.error = Some(Error::NoHeader);
        return out;
    }
    let header = encoded[encoded_size as usize - 1] as i8 as i16;
    let Some(packet) = (header >= 0).then(|| group.packets.get(header as usize)).flatten() else {
        out.error = Some(Error::BadType);
        return out;
    };
    if packet.packet_class != expected_packet_class {
        out.error = Some(Error::MismatchedClass);
        return out;
    }
    out.encoded_size -= 1;
    let decoded_ok = match &packet.definition {
        None => true,
        Some(definition) => {
            let (ok, version) = decode(definition, &mut encoded[..out.encoded_size as usize], decoded);
            out.packet_version = version;
            ok
        }
    };
    if decoded_ok {
        out.packet_type = header;
    } else {
        out.error = Some(Error::WouldNotDecode);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: i16, count: i16) -> Field {
        Field { kind, count, minimum_version: 0, maximum_version: 0 }
    }

    fn definition() -> Definition {
        // a short, two longs, a 7-character string, data of up to 4 bytes,
        // and an array of up to 2 elements each a byte and a short
        Definition {
            size: 2 + 8 + 8 + 6 + 2 + 2 * 3,
            version: 1,
            fields: vec![
                field(field_type::SHORTS, 1),
                field(field_type::LONGS, 2),
                field(field_type::STRING, 7),
                field(field_type::DATA, 4),
                field(field_type::ARRAY, 2),
                field(field_type::BYTES, 1),
                field(field_type::SHORTS, 1),
                field(field_type::END, 0),
                field(field_type::END, 0),
            ],
        }
    }

    #[test]
    fn sizes_add_up() {
        let d = definition();
        let (total, count) = verify(&d, &mut vec![0; d.fields.len()], 0);
        assert_eq!(total, d.size);
        assert_eq!(count as usize, d.fields.len());
    }

    #[test]
    fn round_trip() {
        let d = definition();
        let mut decoded = vec![0u8; d.size as usize];
        decoded[0..2].copy_from_slice(&0x1234i16.to_le_bytes());
        decoded[2..6].copy_from_slice(&0x0102_0304i32.to_le_bytes());
        decoded[6..10].copy_from_slice(&(-5i32).to_le_bytes());
        decoded[10..13].copy_from_slice(b"abc");
        decoded[18..20].copy_from_slice(&3i16.to_le_bytes());
        decoded[20..23].copy_from_slice(&[9, 8, 7]);
        decoded[24..26].copy_from_slice(&1i16.to_le_bytes());
        decoded[26] = 0x55;
        decoded[27..29].copy_from_slice(&(-2i16).to_le_bytes());
        let mut encoded = vec![0u8; 64];
        let (size, ok) = encode(&d, -1, &decoded, &mut encoded);
        assert!(ok);
        assert_eq!(
            &encoded[..size as usize],
            &[1, 0x12, 0x34, 1, 2, 3, 4, 0xff, 0xff, 0xff, 0xfb, b'a', b'b', b'c', 0, 3, 9, 8, 7, 1, 0x55, 0xff, 0xfe]
        );
        let mut back = vec![0u8; d.size as usize];
        let (ok, version) = decode(&d, &mut encoded[..size as usize], &mut back);
        assert!(ok);
        assert_eq!(version, 1);
        // (the string's characters past its terminator, and the data and
        // elements past their counts, are left as they were)
        assert_eq!(back, decoded);
    }

    #[test]
    fn group_header() {
        let group = Group {
            packet_class_count: 1,
            maximum_decoded_packet_size: 64,
            maximum_encoded_packet_size: 64,
            packets: vec![Entry { packet_class: 0, definition: None }, Entry { packet_class: 0, definition: Some(definition()) }],
        };
        let decoded = vec![0u8; 26 + 6];
        let mut encoded = vec![0u8; 64];
        let (size, error) = group_encode(&group, &decoded, &mut encoded, 1, 1);
        assert_eq!(error, None);
        assert_eq!(encoded[size as usize - 1], 1);
        let mut back = vec![0u8; 26 + 6];
        let out = group_decode(&group, &mut back, &mut encoded, size, 0, 1, 0);
        assert_eq!(out.error, None);
        assert_eq!(out.packet_type, 1);
        assert_eq!(out.encoded_size, size - 1);
    }
}

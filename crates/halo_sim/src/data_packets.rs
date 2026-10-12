//! The game's packet codec (memory/data_packet_groups.c's
//! `data_packet_group_encode_packet` and `_decode_packet`) in Rust
//! ([`halo_map::data_packets`]): the group's definitions are read where
//! the game holds them, and the packet encoded or decoded in its buffers.

use halo_map::data_packets::{Definition, Entry, Field, Group, field_type, group_decode, group_encode};

/// A packet's encoding or decoding (halo_sim.h's
/// `struct halo_sim_data_packet_call`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DataPacketCall {
    /// The group (`struct data_packet_group_definition`).
    pub group: u32,
    pub decoded: *mut u8,
    pub encoded: *mut u8,
    /// Decoding's in, both's out.
    pub encoded_size: i16,
    /// Encoding's in; decoding's in and out.
    pub packet_type: i16,
    /// Decoding's in and out.
    pub packet_version: i16,
    pub expected_packet_class: i16,
    /// Encoding's version (NONE for the definition's).
    pub version: i32,
    /// The error (data_packets' `Error`), 0 for none.
    pub error: i32,
    /// Decoding's: the caller's size, written (as the game does) before
    /// the struct is (a struct shorter than its packet's definition
    /// overruns into what lies after it). Null for none.
    pub encoded_size_reference: *mut i16,
}

/// Reads game memory.
///
/// # Safety
/// `address` must be readable for `T`.
unsafe fn read<T: Copy>(address: u32) -> T {
    // SAFETY: as the caller promises
    unsafe { (address as usize as *const T).read_unaligned() }
}

/// A definition where the game holds it (`struct data_packet_definition`):
/// its fields through the top level's end.
///
/// # Safety
/// `address` must be a definition.
unsafe fn definition(address: u32) -> Definition {
    // SAFETY: the definition's fields, as the caller promises
    unsafe {
        let fields_address: u32 = read(address + 0xc);
        let mut fields = Vec::new();
        let mut depth = 0i32;
        loop {
            let at = fields_address + 10 * fields.len() as u32;
            let field = Field { kind: read(at), count: read(at + 2), minimum_version: read(at + 4), maximum_version: read(at + 6) };
            fields.push(field);
            if field.kind == field_type::ARRAY {
                depth += 1;
            } else if field.kind == field_type::END {
                depth -= 1;
                if depth < 0 {
                    break;
                }
            } else if !(0..field_type::COUNT).contains(&field.kind) || fields.len() > 4096 {
                break;
            }
        }
        Definition { size: read(address + 8), version: read(address + 0xa), fields }
    }
}

/// A group where the game holds it (`struct data_packet_group_definition`).
///
/// # Safety
/// `address` must be a group.
unsafe fn group(address: u32) -> Group {
    // SAFETY: the group and its entries, as the caller promises
    unsafe {
        let packet_type_count: i16 = read(address + 4);
        let packets_address: u32 = read(address + 0x10);
        let packets = (0..packet_type_count.max(0) as u32)
            .map(|index| {
                let entry = packets_address + 8 * index;
                let definition_address: u32 = read(entry + 4);
                Entry {
                    packet_class: read(entry),
                    definition: (definition_address != 0).then(|| definition(definition_address)),
                }
            })
            .collect();
        Group {
            packet_class_count: read(address + 6),
            maximum_decoded_packet_size: read(address + 8),
            maximum_encoded_packet_size: read(address + 0xc),
            packets,
        }
    }
}

/// Encodes the packet into the game's buffer (the group's most bytes).
///
/// # Safety
/// `call` must be valid, its buffers the group's sizes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_data_packet_encode(call: *mut DataPacketCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    // SAFETY: the game's group
    let group = unsafe { group(call.group) };
    let Some(definition) = group.packets.get(call.packet_type as usize).and_then(|p| p.definition.as_ref()) else {
        return -1;
    };
    let maximum = (group.maximum_encoded_packet_size as i16).max(0) as usize;
    // SAFETY: the decoded struct (its definition's size) and the buffer,
    // as the caller promises
    let (decoded, encoded) = unsafe {
        (
            std::slice::from_raw_parts(call.decoded, definition.size.max(0) as usize),
            std::slice::from_raw_parts_mut(call.encoded, maximum),
        )
    };
    let (size, error) = group_encode(&group, decoded, encoded, call.packet_type, call.version);
    call.encoded_size = size;
    call.error = error.map_or(0, |e| e as i32);
    0
}

/// Decodes the packet of `encoded_size` bytes (swapped in place as it's
/// read) into the game's struct.
///
/// # Safety
/// `call` must be valid, the decoded struct large enough for the packet's
/// type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_data_packet_decode(call: *mut DataPacketCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    // SAFETY: the game's group
    let group = unsafe { group(call.group) };
    let encoded_size = call.encoded_size.max(0) as usize;
    // SAFETY: the encoded packet, as the caller promises
    let encoded = unsafe { std::slice::from_raw_parts_mut(call.encoded, encoded_size) };
    // (the struct the header's type decodes into, if any)
    let decoded_size = encoded
        .last()
        .map(|&header| header as i8)
        .filter(|&header| header >= 0)
        .and_then(|header| group.packets.get(header as usize))
        .and_then(|packet| packet.definition.as_ref())
        .map_or(0, |definition| definition.size.max(0) as usize);
    // (the size the header's taken from, where the game sets it: before
    // the struct)
    let header = encoded.last().map(|&header| header as i8 as i16);
    let matched = header
        .filter(|&header| header >= 0)
        .and_then(|header| group.packets.get(header as usize))
        .is_some_and(|packet| packet.packet_class == call.expected_packet_class);
    if matched && !call.encoded_size_reference.is_null() {
        // SAFETY: the caller's size
        unsafe { call.encoded_size_reference.write_unaligned(call.encoded_size - 1) };
    }
    // SAFETY: the decoded struct, as the caller promises
    let decoded = unsafe { std::slice::from_raw_parts_mut(call.decoded, decoded_size) };
    let out = group_decode(
        &group,
        decoded,
        encoded,
        call.encoded_size,
        call.packet_type,
        call.packet_version,
        call.expected_packet_class,
    );
    call.encoded_size = out.encoded_size;
    call.packet_type = out.packet_type;
    call.packet_version = out.packet_version;
    call.error = out.error.map_or(0, |e| e as i32);
    0
}

//! memory/data.c's data arrays (`struct data_array`) as the game keeps them:
//! fixed-size datums, each headed by its identifier (zero when free), handed
//! out from the first free absolute index. The script runtime and the AI make
//! and delete their datums (threads, globals, props) with these, so the
//! arrays stay exactly as the game's own `datum_new` and `datum_delete`
//! would leave them.

use crate::ai_actor::{AiMemory, NONE};

/// `struct data_array`'s fields.
pub const DATA_NAME: u32 = 0;
pub const DATA_MAXIMUM_COUNT: u32 = 0x20;
pub const DATA_SIZE: u32 = 0x22;
pub const DATA_VALID: u32 = 0x24;
pub const DATA_SIGNATURE: u32 = 0x28;
pub const DATA_FIRST_FREE_ABSOLUTE_INDEX: u32 = 0x2C;
pub const DATA_COUNT: u32 = 0x2E;
pub const DATA_ACTUAL_COUNT: u32 = 0x30;
pub const DATA_NEXT_IDENTIFIER: u32 = 0x32;
pub const DATA_DATA: u32 = 0x34;

/// `data_array`'s signature, "d@t@".
pub const DATA_SIGNATURE_VALUE: u32 = u32::from_be_bytes(*b"d@t@");

/// `datum_get`: the datum's address (None for one unused or changed).
pub fn datum_get(m: &dyn AiMemory, data: u32, index: i32) -> Option<u32> {
    let identifier = (index >> 16) as i16;
    let absolute_index = index as i16;
    let count = m.r16(data + DATA_COUNT) as i16;
    if absolute_index >= 0 && absolute_index < count {
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let header = m.r32(data + DATA_DATA) + size * absolute_index as u32;
        let header_identifier = m.r16(header) as i16;
        if header_identifier != 0 && (identifier == 0 || identifier == header_identifier) {
            return Some(header);
        }
    }
    None
}

/// `data_next_index`.
pub fn data_next_index(m: &dyn AiMemory, data: u32, index: i32) -> i32 {
    let mut absolute_index = index.wrapping_add(1) as i16;
    let count = m.r16(data + DATA_COUNT) as i16;
    if absolute_index >= 0 && absolute_index < count {
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let base = m.r32(data + DATA_DATA);
        while absolute_index < count {
            let identifier = m.r16(base + size * absolute_index as u32) as i16;
            if identifier != 0 {
                return ((identifier as i32) << 16) | absolute_index as u16 as i32;
            }
            absolute_index += 1;
        }
    }
    NONE
}

/// `data_usable`: whether the array gives out datums.
pub fn data_usable(m: &dyn AiMemory, data: u32) -> bool {
    m.r32(data + DATA_SIGNATURE) == DATA_SIGNATURE_VALUE
        && m.r32(data + DATA_DATA) != 0
        && m.r8(data + DATA_VALID) != 0
        && m.r16(data + DATA_NEXT_IDENTIFIER) != 0
}

/// `datum_initialize`: zeroed, with the array's next identifier.
pub fn datum_initialize(m: &dyn AiMemory, data: u32, header: u32) {
    let size = m.r16(data + DATA_SIZE) as i16 as u32;
    for offset in 0..size {
        m.w8(header + offset, 0);
    }
    let identifier = m.r16(data + DATA_NEXT_IDENTIFIER);
    m.w16(header, identifier);
    let next = identifier.wrapping_add(1);
    m.w16(data + DATA_NEXT_IDENTIFIER, if next == 0 { 0x8000 } else { next });
}

/// `datum_new`.
pub fn datum_new(m: &dyn AiMemory, data: u32) -> i32 {
    if !data_usable(m, data) {
        return NONE;
    }
    let maximum_count = m.r16(data + DATA_MAXIMUM_COUNT) as i16;
    let size = m.r16(data + DATA_SIZE) as i16 as u32;
    let base = m.r32(data + DATA_DATA);
    let mut absolute_index = m.r16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX) as i16;
    while absolute_index < maximum_count {
        let header = base.wrapping_add(size.wrapping_mul(absolute_index as i32 as u32));
        if m.r16(header) == 0 {
            datum_initialize(m, data, header);
            m.w16(data + DATA_ACTUAL_COUNT, m.r16(data + DATA_ACTUAL_COUNT).wrapping_add(1));
            m.w16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX, (absolute_index + 1) as u16);
            if m.r16(data + DATA_COUNT) as i16 <= absolute_index {
                m.w16(data + DATA_COUNT, (absolute_index + 1) as u16);
            }
            return ((m.r16(header) as i16 as i32) << 16) | absolute_index as i32;
        }
        absolute_index += 1;
    }
    // (the port's: none free from here on, so the next search starts at
    // the end, until a datum is deleted)
    m.w16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX, maximum_count as u16);
    NONE
}

/// `datum_new_at_index`.
pub fn datum_new_at_index(m: &dyn AiMemory, data: u32, index: i32) -> i32 {
    if !data_usable(m, data) {
        return NONE;
    }
    let absolute_index = index as i16;
    let identifier = (index >> 16) as i16;
    let maximum_count = m.r16(data + DATA_MAXIMUM_COUNT) as i16;
    if absolute_index >= 0 && absolute_index < maximum_count && identifier != 0 {
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let header = m.r32(data + DATA_DATA).wrapping_add(size * absolute_index as u32);
        if m.r16(header) == 0 {
            m.w16(data + DATA_ACTUAL_COUNT, m.r16(data + DATA_ACTUAL_COUNT).wrapping_add(1));
            if absolute_index >= m.r16(data + DATA_COUNT) as i16 {
                m.w16(data + DATA_COUNT, (absolute_index + 1) as u16);
            }
            datum_initialize(m, data, header);
            m.w16(header, identifier as u16);
            return ((identifier as i32) << 16) | absolute_index as i32;
        }
    }
    NONE
}

/// `datum_delete`.
pub fn datum_delete(m: &dyn AiMemory, data: u32, index: i32) {
    let Some(mut header) = datum_get(m, data, index) else { return };
    m.w16(header, 0);
    if (index as i16) < m.r16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX) as i16 {
        m.w16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX, index as u16);
    }
    if (index as i16) as i32 + 1 == m.r16(data + DATA_COUNT) as i16 as i32 {
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        loop {
            header = header.wrapping_sub(size);
            let count = (m.r16(data + DATA_COUNT) as i16).wrapping_sub(1);
            m.w16(data + DATA_COUNT, count as u16);
            if !(count > 0 && m.r16(header) == 0) {
                break;
            }
        }
    }
    m.w16(data + DATA_ACTUAL_COUNT, m.r16(data + DATA_ACTUAL_COUNT).wrapping_sub(1));
}

/// `data_make_valid` (with `data_delete_all`).
pub fn data_make_valid(m: &dyn AiMemory, data: u32) {
    m.w8(data + DATA_VALID, 1);
    m.w16(data + DATA_COUNT, 0);
    m.w16(data + DATA_ACTUAL_COUNT, 0);
    m.w16(data + DATA_FIRST_FREE_ABSOLUTE_INDEX, 0);
    // (the name's first two characters, as `csstrncpy` copies them)
    let first = m.r8(data + DATA_NAME) as u16;
    let second = if first == 0 { 0 } else { m.r8(data + DATA_NAME + 1) as u16 };
    m.w16(data + DATA_NEXT_IDENTIFIER, first | (second << 8) | 0x8000);
    let maximum_count = m.r16(data + DATA_MAXIMUM_COUNT) as i16;
    let size = m.r16(data + DATA_SIZE) as i16 as u32;
    let base = m.r32(data + DATA_DATA);
    for absolute_index in 0..maximum_count.max(0) as u32 {
        m.w16(base + size * absolute_index, 0);
    }
}

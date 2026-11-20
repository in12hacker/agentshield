//! Read the inspected image's postmortem constants and actual getter instructions.
//! An absent or unfamiliar getter is a capture gap, never a version-table fallback.
use super::Symbols;
use object::{Object, ObjectSection, SectionKind, SymbolKind, read::ReadRef};
use std::collections::BTreeMap;

fn symbol<'a, R: ReadRef<'a>>(
    file: &object::File<'a, R>,
    symbols: &Symbols<'_>,
    name: &str,
) -> Option<(u64, u64, object::SectionIndex)> {
    let mut found = None;
    for entry in symbols.named(name) {
        let section_index = entry.section?;
        let section = file.section_by_index(section_index).ok()?;
        if entry.kind != SymbolKind::Text
            || section.kind() != SectionKind::Text
            || entry.address % 4 != 0
            || entry.size == 0
            || entry.size % 4 != 0
            || !contains(section.address(), section.size(), entry.address, entry.size)
        {
            return None;
        }
        let value = (entry.address, entry.size, section_index);
        if found.is_some_and(|old| old != value) {
            return None;
        }
        found = Some(value);
    }
    found
}

fn bytes<'a, R: ReadRef<'a>>(
    file: &object::File<'a, R>,
    symbols: &Symbols<'_>,
    name: &str,
) -> Option<Vec<u8>> {
    let (address, size, section) = symbol(file, symbols, name)?;
    // Getter code and postmortem scalar sizes must be real, finite symbol bounds.
    if size == 0 || size > 4096 {
        return None;
    }
    let section = file.section_by_index(section).ok()?;
    Some(section.data_range(address, size).ok()??.to_vec())
}

fn contains(start: u64, length: u64, address: u64, bytes: u64) -> bool {
    start
        .checked_add(length)
        .zip(address.checked_add(bytes))
        .is_some_and(|(end, stop)| address >= start && stop <= end)
}

fn words<'a, R: ReadRef<'a>>(
    file: &object::File<'a, R>,
    symbols: &Symbols<'_>,
    name: &str,
) -> Option<Vec<u32>> {
    let data = bytes(file, symbols, name)?;
    if data.len() % 4 != 0 {
        return None;
    }
    data.chunks_exact(4)
        .map(|v| Some(u32::from_le_bytes(v.try_into().ok()?)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    Unknown,
    This,
    ThreadPointer(u64),
    Isolate(u64),
    Field(u64),
    Handle(usize),
}
#[derive(Clone)]
struct Path {
    pc: usize,
    registers: [Value; 32],
    stores: Vec<(Value, u64)>,
    visited: Vec<usize>,
}
fn branch(pc: usize, immediate: u32, bits: u32, length: usize) -> Option<usize> {
    let signed = i64::from((immediate << (32 - bits)) as i32 >> (32 - bits));
    let target = i64::try_from(pc).ok()?.checked_add(signed)?;
    let target = usize::try_from(target).ok()?;
    (target < length).then_some(target)
}
/// Interpret every reachable instruction from the symbol entry. Both successors
/// of unknown conditions must terminate with the same demonstrated source. No
/// suffix scanning, external branches, loops, or unknown instructions are used.
fn getter(code: &[u32], continuation: bool, extension: Option<(u64, u64)>) -> Option<u64> {
    let mut registers = [Value::Unknown; 32];
    if continuation {
        registers[0] = Value::This;
    }
    let mut pending = vec![Path {
        pc: 0,
        registers,
        stores: Vec::new(),
        visited: Vec::new(),
    }];
    let mut result = None;
    let mut returns = 0;
    let mut steps = 0_usize;
    while let Some(mut path) = pending.pop() {
        loop {
            steps = steps.checked_add(1)?;
            if steps > code.len().saturating_mul(8) {
                return None;
            }
            if path.visited.contains(&path.pc) {
                return None;
            }
            path.visited.push(path.pc);
            let word = *code.get(path.pc)?;
            let target = (word & 31) as usize;
            let base = ((word >> 5) & 31) as usize;
            if word == 0xd65f03c0 {
                let offset = if continuation {
                    let returned = path.registers[0];
                    let mut fields = path
                        .stores
                        .iter()
                        .filter(|(pointer, _)| *pointer == returned)
                        .map(|(_, field)| *field);
                    let offset = fields.next()?;
                    if fields.any(|v| v != offset) {
                        return None;
                    }
                    offset
                } else if let Value::Isolate(offset) = path.registers[0] {
                    offset
                } else {
                    return None;
                };
                if result.is_some_and(|v| v != offset) {
                    return None;
                }
                result = Some(offset);
                returns += 1;
                break;
            } else if word & !31 == 0xd53bd040 {
                if continuation || target == 31 {
                    return None;
                }
                path.registers[target] = Value::ThreadPointer(0);
            } else if word & 0xffe0ffe0 == 0xaa0003e0 {
                if target == 31 {
                    return None;
                }
                path.registers[target] = path.registers[((word >> 16) & 31) as usize];
            } else if word & 0xff800000 == 0x91000000 {
                if target == 31 {
                    return None;
                }
                let add =
                    u64::from((word >> 10) & 4095) << if word & (1 << 22) != 0 { 12 } else { 0 };
                path.registers[target] = match path.registers[base] {
                    Value::ThreadPointer(value) => Value::ThreadPointer(value.checked_add(add)?),
                    _ => Value::Unknown,
                };
            } else if word & 0xffc00000 == 0xf9400000 {
                if target == 31 || base == 31 {
                    return None;
                }
                let offset = u64::from((word >> 10) & 4095) * 8;
                path.registers[target] = match path.registers[base] {
                    Value::ThreadPointer(value) if !continuation => {
                        Value::Isolate(value.checked_add(offset)?)
                    }
                    Value::This if continuation => Value::Field(offset),
                    _ => Value::Unknown,
                };
            } else if word & 0xffc00000 == 0xf9000000 {
                if base != 31 && path.registers[base] != Value::This {
                    let pointer = path.registers[base];
                    if !matches!(pointer, Value::Field(_) | Value::Handle(_)) {
                        // An untracked destination may alias a prior handle.
                        path.stores.clear();
                    } else if word >> 10 & 4095 == 0 {
                        // The last write owns the slot. Unknown values and XZR
                        // invalidate an earlier recognized Field store too.
                        path.stores.retain(|(stored, _)| *stored != pointer);
                    }
                    if let Value::Field(offset) = path.registers[target]
                        && matches!(pointer, Value::Field(_) | Value::Handle(_))
                        && pointer != Value::Field(offset)
                        && word >> 10 & 4095 == 0
                    {
                        path.stores.push((pointer, offset));
                    }
                }
            } else if word & 0xfc000000 == 0x94000000 {
                // A call may clobber all argument registers. The continuation
                // getter's extended Handle is independently checked at its RET.
                if continuation {
                    let (address, expected) = extension?;
                    let signed = i64::from(((word & 0x03ffffff) << 6) as i32 >> 6) * 4;
                    let site = address.checked_add(u64::try_from(path.pc).ok()?.checked_mul(4)?)?;
                    if site.checked_add_signed(signed)? != expected {
                        return None;
                    }
                }
                path.registers[..19].fill(Value::Unknown);
                if continuation {
                    path.registers[0] = Value::Handle(path.pc);
                }
            } else if word & 0x9f000000 == 0x90000000 {
                if target == 31 {
                    return None;
                }
                path.registers[target] = Value::Unknown;
            } else if word & 0xff000010 == 0x54000000 || word & 0x7e000000 == 0x34000000 {
                let destination = branch(path.pc, word >> 5 & 0x7ffff, 19, code.len())?;
                let mut taken = path.clone();
                taken.pc = destination;
                pending.push(taken);
            } else if word & 0xff20001f == 0xeb00001f {
                // CMP: its unknown condition is followed on both branches.
            } else if word & 0xffc00000 == 0xa9000000 || word & 0xffc00000 == 0xa9800000 {
                if base != 31 {
                    return None;
                }
            } else if word & 0xffc00000 == 0xa9400000 || word & 0xffc00000 == 0xa8c00000 {
                if base != 31 {
                    return None;
                }
                path.registers[target] = Value::Unknown;
                path.registers[((word >> 10) & 31) as usize] = Value::Unknown;
            } else {
                return None;
            }
            path.pc += 1;
        }
    }
    if returns == 0 { None } else { result }
}
fn tls_offset(code: &[u32]) -> Option<u64> {
    getter(code, false, None)
}
fn continuation_offset(code: &[u32], extension: Option<(u64, u64)>) -> Option<u64> {
    getter(code, true, extension)
}

// AArch64 C++ FieldAccess returns via the hidden result pointer in x8. Its
// first ABI fields are the 32-bit BaseTaggedness and signed byte offset. Decode
// real constant stores, including a callee-saved result pointer across calls.
fn access_offset(code: &[u32]) -> Option<u64> {
    #[derive(Clone, Copy)]
    enum Register {
        Unknown,
        Number(u64),
        Result,
    }
    let mut registers = [Register::Unknown; 32];
    registers[0] = Register::Number(0);
    registers[8] = Register::Result;
    registers[31] = Register::Number(0);
    let mut bytes = BTreeMap::<u64, u8>::new();
    let mut returned = false;
    for word in code {
        let target = (word & 31) as usize;
        let base = ((word >> 5) & 31) as usize;
        if *word == 0xd65f03c0 {
            returned = true;
            break;
        }
        if word & 0x7f800000 == 0x52800000 || word & 0x7f800000 == 0x12800000 {
            if target == 31 {
                return None;
            }
            let shift = (word >> 21 & 3) * 16;
            let mut value = u64::from(word >> 5 & 65535) << shift;
            if word & 0x7f800000 == 0x12800000 {
                value = !value;
            }
            if word >> 31 == 0 {
                value &= u64::from(u32::MAX);
            }
            registers[target] = Register::Number(value);
        } else if word & 0x7f800000 == 0x72800000 {
            let Register::Number(previous) = registers[target] else {
                return None;
            };
            let shift = (word >> 21 & 3) * 16;
            let value =
                (previous & !(65535_u64 << shift)) | (u64::from(word >> 5 & 65535) << shift);
            registers[target] = Register::Number(if word >> 31 == 0 {
                value & u64::from(u32::MAX)
            } else {
                value
            });
        } else if word & 0xff800000 == 0x53000000 {
            let rotate = word >> 16 & 31;
            let limit = word >> 10 & 31;
            if rotate == 0 || limit + 1 != rotate {
                return None;
            }
            registers[target] = match registers[base] {
                Register::Number(value) => {
                    Register::Number((value << (32 - rotate)) & u64::from(u32::MAX))
                }
                _ => Register::Unknown,
            };
        } else if word & 0xffe0ffe0 == 0xaa0003e0 {
            registers[target] = registers[((word >> 16) & 31) as usize];
        } else if word & 0x7f800000 == 0x11000000 {
            let add = u64::from(word >> 10 & 4095) << if word & (1 << 22) != 0 { 12 } else { 0 };
            registers[target] = match registers[base] {
                Register::Number(value) => Register::Number(value.checked_add(add)?),
                _ => Register::Unknown,
            };
        } else if word & 0xffc00000 == 0xa9000000 || word & 0xffc00000 == 0xa9800000 {
            if matches!(registers[base], Register::Result) {
                let start = u64::from(word >> 15 & 127) * 8;
                for (part, reg) in [target, ((word >> 10) & 31) as usize]
                    .into_iter()
                    .enumerate()
                {
                    for index in 0..8_u64 {
                        let offset = start + part as u64 * 8 + index;
                        if let Register::Number(value) = registers[reg] {
                            bytes.insert(offset, (value >> (index * 8)) as u8);
                        } else {
                            bytes.remove(&offset);
                        }
                    }
                }
            } else if base != 31 {
                return None;
            }
        } else if matches!(
            word & 0xffc00000,
            0xf9000000 | 0xb9000000 | 0x79000000 | 0x39000000
        ) {
            let width = 1_u64 << (word >> 30);
            if matches!(registers[base], Register::Result) {
                let start = u64::from(word >> 10 & 4095) * width;
                for index in 0..width {
                    if let Register::Number(value) = registers[target] {
                        bytes.insert(start + index, (value >> (index * 8)) as u8);
                    } else {
                        bytes.remove(&(start + index));
                    }
                }
            } else if base != 31 {
                return None;
            }
        } else if word & 0xfc000000 == 0x94000000 {
            bytes.clear();
            registers[..19].fill(Register::Unknown);
        } else if word & 0x9f000000 == 0x90000000
            || word & 0xffc00000 == 0xf9400000
            || word & 0xffc00000 == 0xb2400000
        {
            registers[target] = Register::Unknown;
        } else if word & 0xffc00000 == 0xa9400000 || word & 0xffc00000 == 0xa8c00000 {
            if base != 31 {
                return None;
            }
            registers[target] = Register::Unknown;
            registers[((word >> 10) & 31) as usize] = Register::Unknown;
        } else if word & 0xffe0fc00 == 0xb800a000 {
            if base != 31 && !matches!(registers[base], Register::Result) {
                return None;
            }
        } else {
            return None;
        }
    }
    if !returned {
        return None;
    }
    let result = u32::from_le_bytes([
        *bytes.get(&4)?,
        *bytes.get(&5)?,
        *bytes.get(&6)?,
        *bytes.get(&7)?,
    ]);
    (result <= i32::MAX as u32).then_some(u64::from(result))
}
fn map_type(code: &[u32], type_offset: u64, tag: u64) -> Option<u64> {
    if code.len() != 9
        || code[..3] != [0xf9400000; 3]
        || code[4] != 0x79400000
        || code[5] != 0x12003c00
        || code[7] != 0x1a9f17e0
        || code[8] != 0xd65f03c0
        || code[3] & 0xffc003ff != 0x91000000
        || code[6] & 0xffc003ff != 0x7100001f
    {
        return None;
    }
    if u64::from(code[3] >> 10 & 4095).checked_add(tag)? != type_offset {
        return None;
    }
    Some(u64::from(code[6] >> 10 & 4095))
}
fn array_length(code: &[u32], tag: u64, smi_shift: u64, number_value: u64) -> Option<u64> {
    if code.len() != 8
        || code[0] != 0xf9400000
        || code[1] & 0xffe00fff != 0xf8400000
        || code[2] != 0x37000060
        || code[4] != 0xd65f03c0
        || code[6] != 0x1e790000
        || code[7] != 0xd65f03c0
        || code[3] & 0xffc0ffff != 0xd340fc00
    {
        return None;
    }
    if u64::from(code[3] >> 16 & 63) != smi_shift {
        return None;
    }
    let offset = i64::from(((code[1] >> 12 & 511) << 23) as i32 >> 23);
    let float_offset = i64::from(((code[5] >> 12 & 511) << 23) as i32 >> 23);
    if code[5] & 0xffe00fff != 0xfc400000
        || u64::try_from(float_offset).ok()?.checked_add(tag)? != number_value
    {
        return None;
    }
    u64::try_from(offset).ok()?.checked_add(tag)
}

fn property_length_mask(code: &[u32], offset: u64, tag: u64, smi_shift: u64) -> Option<u64> {
    // The actual printer reads the high signed Smi word, then applies the
    // generated length mask. All instructions in the symbol must be known.
    let mut found = None;
    for (index, word) in code.iter().enumerate() {
        let known = matches!(
            word & 0xffc00000,
            0xa9800000
                | 0xa9000000
                | 0xa9400000
                | 0xa8c00000
                | 0xf9400000
                | 0xf9000000
                | 0x91000000
        ) || word & 0x9f000000 == 0x90000000
            || word & 0xffe0ffe0 == 0xaa0003e0
            || word & 0x7f800000 == 0x52800000
            || word & 0xfc000000 == 0x94000000
            || word & 0xfc000000 == 0x14000000
            || word & 0xffe00c00 == 0xb8800000
            || word & 0x7f800000 == 0x12000000
            || word & 0xff800000 == 0x53000000;
        if !known || (word & 0xfc000000 == 0x14000000 && index + 1 != code.len()) {
            return None;
        }
        if word & 0x7f800000 != 0x12000000 {
            continue;
        }
        let base = word >> 5 & 31;
        if word & 31 != base || word >> 31 != 0 || word >> 22 & 1 != 0 || word >> 16 & 63 != 0 {
            return None;
        }
        let imms = word >> 10 & 63;
        if imms >= 31 {
            return None;
        }
        let previous = *code.get(index.checked_sub(1)?)?;
        if previous & 0xffe00c00 != 0xb8800000
            || previous & 31 != base
            || code.get(index.checked_sub(3)?)? & 0xffc003ff != (0xf9400280 | (previous >> 5 & 31))
            || *code.get(index.checked_sub(2)?)? != 0xaa1303e0
        {
            return None;
        }
        let displacement = i64::from(((previous >> 12 & 511) << 23) as i32 >> 23);
        if u64::try_from(displacement).ok()?.checked_add(tag)?
            != offset.checked_add(smi_shift / 8)?
        {
            return None;
        }
        let mask = (1_u64 << (imms + 1)) - 1;
        if found.is_some_and(|v| v != mask) {
            return None;
        }
        found = Some(mask);
    }
    found
}

fn collection_table(code: &[u32], tag: u64) -> Option<u64> {
    // The actual JSMap printer's table value is loaded from the original
    // receiver and passed by reference to Brief. Validate the complete symbol,
    // including its register aliases and final tail call; never scan dead bytes.
    if code.len() != 26
        || code[0] != 0xa9bd7bfd
        || code[3] != 0x910003fd
        || code[4] != 0xa90153f3
        || code[5] != 0xaa0003f4
        || code[6] != 0xaa0103f3
        || code[7] != 0xaa0103e0
        || code[8] != 0xf9400281
        || code[10] != 0xaa1303e0
        || code[15] != 0xf9400282
        || code[16] != 0x9100a3e1
        || code[17] != 0xaa1303e0
        || code[18] & 0xffe00fff != 0xf8400042
        || code[19] != 0xf90017e2
        || code[21] != 0xf9400281
        || code[22] != 0xaa1303e0
        || code[23] != 0xa94153f3
        || code[24] != 0xa8c37bfd
        || code[25] & 0xfc000000 != 0x14000000
    {
        return None;
    }
    for i in [1_usize, 12] {
        if code[i] & 0x9f000000 != 0x90000000 {
            return None;
        }
    }
    for i in [2_usize, 13] {
        if code[i] & 0xffc00000 != 0x91000000 {
            return None;
        }
    }
    for i in [9_usize, 14, 20] {
        if code[i] & 0xfc000000 != 0x94000000 {
            return None;
        }
    }
    if code[11] & 0xff80001f != 0xd2800002 {
        return None;
    }
    let displacement = i64::from(((code[18] >> 12 & 511) << 23) as i32 >> 23);
    u64::try_from(displacement).ok()?.checked_add(tag)
}

fn dictionary_capacity(code: &[u32], smi_shift: u64) -> Option<u64> {
    if code.len() != 4
        || code[0] & 0xffc003ff != 0x91000000
        || code[2] != 0xf9000001
        || code[3] != 0xd65f03c0
        || code[1] & 0xffc003ff != 0xd3400021
    {
        return None;
    }
    let rotate = code[1] >> 16 & 63;
    let limit = code[1] >> 10 & 63;
    if rotate == 0 || limit + 1 != rotate || u64::from(64 - rotate) != smi_shift {
        return None;
    }
    Some(u64::from(code[0] >> 10 & 4095))
}
fn dictionary_index(code: &[u32]) -> Option<(u64, u64)> {
    if code.len() != 3
        || code[0] >> 16 & 31 != 0
        || code[0] & 0xffc0ffff != 0x0b000400
        || code[1] & 0xffc003ff != 0x11000000
        || code[2] != 0xd65f03c0
    {
        return None;
    }
    let shift = code[0] >> 10 & 63;
    Some((1 + (1_u64 << shift), u64::from(code[1] >> 10 & 4095)))
}
fn dictionary_slot(code: &[u32], details: bool, smi_shift: u64) -> Option<(u64, u64)> {
    let expected = if details { 7 } else { 6 };
    if code.len() != expected
        || code[0] != 0x0b010421
        || code[1] & 0xffc003ff != 0x11000021
        || code[2] != 0x8b21cc01
        || code[3] & 0xffc003ff != 0x91000021
        || code[4] != 0xf9400020
        || code[expected - 1] != 0xd65f03c0
    {
        return None;
    }
    if details && (code[5] & 0xffc0ffff != 0xd340fc00 || u64::from(code[5] >> 16 & 63) != smi_shift)
    {
        return None;
    }
    Some((
        u64::from(code[1] >> 10 & 4095),
        u64::from(code[3] >> 10 & 4095),
    ))
}

fn ordered_entry_size(
    code: &[u32],
    tag: u64,
    buckets: u64,
    data: u64,
    smi_shift: u64,
    tagged_size: u64,
) -> Option<u64> {
    if code.len() != 14
        || code[0] != 0xf9400001
        || code[1] & 0xffe00fff != 0xf8400020
        || code[2] & 0xffe00fff != 0xb8800021
        || code[3] & 0xffc003ff != 0xd1000002
        || code[4] & 0xffc003ff != 0x91000000
        || code[5] != 0xf9400000
        || code[6] & 0xffc0ffff != 0x9340fc00
        || code[7] & 0xffe003ff != 0x0b000021
        || code[7] >> 16 & 31 != 1
        || code[8] & 0xffc003ff != 0x11000000
        || code[9] != 0x0b010000
        || code[10] != 0x8b20cc40
        || code[11] & 0xffc003ff != 0x91000000
        || code[12] != 0xf9400000
        || code[13] != 0xd65f03c0
    {
        return None;
    }
    let stride = 1_u64.checked_add(1_u64.checked_shl(code[7] >> 10 & 63)?)?;
    let prefix = u64::from(code[8] >> 10 & 4095);
    if u64::from(code[3] >> 10 & 4095) != tag
        || u64::from(code[4] >> 10 & 4095).checked_add(tag)? != buckets
        || u64::from(code[6] >> 16 & 63) != smi_shift
        || u64::from(code[11] >> 10 & 4095) != data
        || data.checked_add(prefix.checked_mul(tagged_size)?)?
            != buckets.checked_add(tagged_size)?
    {
        return None;
    }
    stride.checked_mul(tagged_size)
}

pub(super) fn read<'a, R: ReadRef<'a>>(
    file: &object::File<'a, R>,
    symbols: &Symbols<'_>,
    gaps: &mut Vec<String>,
) -> BTreeMap<String, u64> {
    let mut result = BTreeMap::new();
    if !file.is_little_endian() {
        return result;
    }
    let mut constants = BTreeMap::<String, Option<u64>>::new();
    for entry in symbols.prefixed("v8dbg_") {
        let name = entry.name;
        if entry.size != 4 {
            continue;
        }
        let Some(section) = entry.section.and_then(|v| file.section_by_index(v).ok()) else {
            continue;
        };
        // V8's generated zero-valued constants have static zero initializers.
        // Runtime-initialized nodedbg BSS is deliberately never interpreted here.
        let value = if !contains(section.address(), section.size(), entry.address, 4) {
            None
        } else if section.kind() == SectionKind::UninitializedData {
            Some(0)
        } else {
            section
                .data_range(entry.address, 4)
                .ok()
                .flatten()
                .and_then(|v| Some(u64::from(u32::from_le_bytes(v.try_into().ok()?))))
        };
        if let Some(value) = value {
            let previous = constants.entry(name.to_owned()).or_insert(Some(value));
            if *previous != Some(value) {
                *previous = None;
            }
        } else {
            constants.insert(name.to_owned(), None);
        }
    }
    for (name, value) in constants {
        if let Some(value) = value {
            result.insert(name, value);
        } else {
            gaps.push(format!("runtime_constant_conflict:{name}"));
        }
    }
    if file.architecture() != object::Architecture::Aarch64 {
        return result;
    }
    let current = words(file, symbols, "_ZN2v87Isolate10GetCurrentEv").and_then(|v| tls_offset(&v));
    let try_current =
        words(file, symbols, "_ZN2v87Isolate13TryGetCurrentEv").and_then(|v| tls_offset(&v));
    if let (Some(left), Some(right)) = (current, try_current)
        && left == right
    {
        result.insert("CURRENT_ISOLATE_TLS".into(), left);
    } else {
        gaps.push("runtime_getter_unavailable:current_isolate_tls".into());
    }
    let getter_name = "_ZN2v87Isolate36GetContinuationPreservedEmbedderDataEv";
    let extension = symbol(file, symbols, getter_name)
        .zip(symbol(
            file,
            symbols,
            "_ZN2v88internal11HandleScope6ExtendEPNS0_7IsolateE",
        ))
        .map(|(getter, extend)| (getter.0, extend.0));
    if let Some(value) =
        words(file, symbols, getter_name).and_then(|v| continuation_offset(&v, extension))
    {
        result.insert("CONTINUATION_FRAME".into(), value);
    } else {
        gaps.push("runtime_getter_unavailable:continuation_frame".into());
    }
    for (name, symbol) in [
        (
            "JS_COLLECTION_TABLE",
            "_ZN2v88internal8compiler13AccessBuilder20ForJSCollectionTableEv",
        ),
        (
            "ORDERED_ELEMENTS",
            "_ZN2v88internal8compiler13AccessBuilder38ForOrderedHashMapOrSetNumberOfElementsEv",
        ),
        (
            "ORDERED_DELETED",
            "_ZN2v88internal8compiler13AccessBuilder45ForOrderedHashMapOrSetNumberOfDeletedElementsEv",
        ),
        (
            "ORDERED_BUCKETS",
            "_ZN2v88internal8compiler13AccessBuilder37ForOrderedHashMapOrSetNumberOfBucketsEv",
        ),
        (
            "ORDERED_ENTRY_VALUE",
            "_ZN2v88internal8compiler13AccessBuilder27ForOrderedHashMapEntryValueEv",
        ),
        (
            "PROPERTY_LENGTH",
            "_ZN2v88internal8compiler13AccessBuilder29ForPropertyArrayLengthAndHashEv",
        ),
        (
            "PROPERTY_DATA",
            "_ZN2v88internal8compiler13AccessBuilder20ForPropertyArraySlotEi",
        ),
    ] {
        if let Some(value) = words(file, symbols, symbol).and_then(|v| access_offset(&v)) {
            result.insert(name.into(), value);
        } else {
            gaps.push(format!("runtime_getter_unavailable:{name}"));
        }
    }
    if let (Some(offset), Some(tag)) = (
        result
            .get("v8dbg_class_Map__instance_type__uint16_t")
            .copied(),
        result.get("v8dbg_HeapObjectTag").copied(),
    ) {
        for (name, symbol) in [
            (
                "ORDERED_MAP_TYPE",
                "_ZNK2v88internal8compiler6MapRef19IsOrderedHashMapMapEv",
            ),
            (
                "NAME_DICTIONARY_TYPE",
                "_ZNK2v88internal8compiler6MapRef19IsNameDictionaryMapEv",
            ),
        ] {
            if let Some(value) =
                words(file, symbols, symbol).and_then(|v| map_type(&v, offset, tag))
            {
                result.insert(name.into(), value);
            } else {
                gaps.push(format!("runtime_getter_unavailable:{name}"));
            }
        }
        let shift = result
            .get("v8dbg_SmiShiftSize")
            .zip(result.get("v8dbg_SmiValueShift"))
            .map(|(a, b)| a + b);
        if let Some(value) = shift.and_then(|shift| {
            words(file, symbols, "_ZNK2v85Array6LengthEv").and_then(|v| {
                array_length(
                    &v,
                    tag,
                    shift,
                    *result.get("v8dbg_class_HeapNumber__value__double")?,
                )
            })
        }) {
            result.insert("JS_ARRAY_LENGTH".into(), value);
        } else {
            gaps.push("runtime_getter_unavailable:JS_ARRAY_LENGTH".into());
        }
    }
    if let (Some(offset), Some(tag), Some(shift), Some(value_shift)) = (
        result.get("PROPERTY_LENGTH"),
        result.get("v8dbg_HeapObjectTag"),
        result.get("v8dbg_SmiShiftSize"),
        result.get("v8dbg_SmiValueShift"),
    ) {
        if let Some(value) = words(
            file,
            symbols,
            "_ZN2v88internal13PropertyArray18PropertyArrayPrintERSo",
        )
        .and_then(|code| property_length_mask(&code, *offset, *tag, shift + value_shift))
        {
            result.insert("PROPERTY_LENGTH_MASK".into(), value);
        } else {
            gaps.push("runtime_getter_unavailable:PROPERTY_LENGTH_MASK".into());
        }
    }
    if let Some(shift) = result
        .get("v8dbg_SmiShiftSize")
        .zip(result.get("v8dbg_SmiValueShift"))
        .map(|(a, b)| a + b)
    {
        if let Some(value)=words(file, symbols,"_ZN2v88internal9HashTableINS0_14NameDictionaryENS0_19NameDictionaryShapeEE11SetCapacityEi").and_then(|code|dictionary_capacity(&code,shift)) {
            result.insert("NAME_DICTIONARY_CAPACITY".into(),value);
        } else {gaps.push("runtime_getter_unavailable:NAME_DICTIONARY_CAPACITY".into());}
        let index=words(file, symbols,"_ZN2v88internal9HashTableINS0_14NameDictionaryENS0_19NameDictionaryShapeEE12EntryToIndexENS0_13InternalIndexE").and_then(|code|dictionary_index(&code));
        let value=words(file, symbols,"_ZN2v88internal10DictionaryINS0_14NameDictionaryENS0_19NameDictionaryShapeEE7ValueAtENS0_13InternalIndexE").and_then(|code|dictionary_slot(&code,false,shift));
        let details=words(file, symbols,"_ZN2v88internal10DictionaryINS0_14NameDictionaryENS0_19NameDictionaryShapeEE9DetailsAtENS0_13InternalIndexE").and_then(|code|dictionary_slot(&code,true,shift));
        if let (Some((stride, start)), Some((value, base)), Some((details, other))) =
            (index, value, details)
            && Some(&stride) == result.get("v8dbg_namedictionaryshape_entry_size")
            && result
                .get("v8dbg_namedictionary_prefix_start_index")
                .zip(result.get("v8dbg_namedictionaryshape_prefix_size"))
                .is_some_and(|(a, b)| a + b == start)
            && Some(&base) == result.get("v8dbg_class_FixedArray__data__uintptr_t")
            && base == other
            && value >= start
            && details >= start
        {
            result.insert("NAME_DICTIONARY_ENTRY_BASE".into(), base + start * 8);
            result.insert("NAME_DICTIONARY_ENTRY_SIZE".into(), stride * 8);
            result.insert("NAME_DICTIONARY_VALUE".into(), (value - start) * 8);
            result.insert("NAME_DICTIONARY_DETAILS".into(), (details - start) * 8);
        } else {
            gaps.push("runtime_getter_unavailable:NAME_DICTIONARY_SLOTS".into());
        }
    }
    if let (Some(tag), Some(buckets), Some(data), Some(shift), Some(value_shift), Some(size)) = (
        result.get("v8dbg_HeapObjectTag"),
        result.get("ORDERED_BUCKETS"),
        result.get("v8dbg_class_FixedArray__data__uintptr_t"),
        result.get("v8dbg_SmiShiftSize"),
        result.get("v8dbg_SmiValueShift"),
        result.get("v8dbg_TaggedSize"),
    ) {
        if let Some(value)=words(file, symbols,"_ZN2v88internal24OrderedHashTableIteratorINS0_13JSMapIteratorENS0_14OrderedHashMapEE10CurrentKeyEv")
            .and_then(|code|ordered_entry_size(&code,*tag,*buckets,*data,shift+value_shift,*size))
        {result.insert("ORDERED_ENTRY_SIZE".into(),value);}
        else {gaps.push("runtime_getter_unavailable:ORDERED_ENTRY_SIZE".into());}
    }
    let table_crosscheck = result.get("v8dbg_HeapObjectTag").copied().and_then(|tag| {
        words(file, symbols, "_ZN2v88internal5JSMap10JSMapPrintERSo")
            .and_then(|code| collection_table(&code, tag))
    });
    if result.get("JS_COLLECTION_TABLE").copied() != table_crosscheck || table_crosscheck.is_none()
    {
        result.remove("JS_COLLECTION_TABLE");
        gaps.push("runtime_getter_unavailable:JS_COLLECTION_TABLE_crosscheck".into());
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn declared_sections_do_not_invent_bytes_past_their_actual_range() {
        assert!(super::contains(100, 4, 100, 4));
        assert!(!super::contains(100, 4, 99, 4));
        assert!(!super::contains(100, 4, 101, 4));
        assert!(!super::contains(100, 0, 100, 4));
        assert!(!super::contains(u64::MAX - 2, 8, u64::MAX - 2, 4));
        assert!(!super::contains(0, u64::MAX, u64::MAX - 2, 4));
    }
    use super::*;
    #[test]
    fn tls_instructions_use_the_actual_immediates_and_reject_unknown_dataflow() {
        let code = [0xd53bd040, 0x91400000, 0x91046000, 0xf9400000, 0xd65f03c0];
        assert_eq!(tls_offset(&code), Some(280));
        let mut changed = code;
        changed[2] = 0x91000000;
        assert_eq!(tls_offset(&changed), Some(0));
        changed[2] = 0xd503201f;
        assert_eq!(tls_offset(&changed), None);
        assert_eq!(tls_offset(&[0xd53bd040, 0xf9400000]), None);
        assert_eq!(
            tls_offset(&[0xd65f03c0, 0xd53bd040, 0xf9400000, 0xd65f03c0]),
            None
        );
        assert_eq!(
            tls_offset(&[0xffffffff, 0xd53bd040, 0xf9400000, 0xd65f03c0]),
            None
        );
        assert_eq!(tls_offset(&[0xd53bd05f, 0xf94003e0, 0xd65f03c0]), None);
    }
    #[test]
    fn continuation_requires_the_value_written_into_the_original_handle() {
        let code = [
            0xaa0003f3, 0xf9411801, 0xf9413c14, 0xf9000034, 0xaa0103e0, 0xd65f03c0,
        ];
        assert_eq!(continuation_offset(&code, None), Some(632));
        assert_eq!(continuation_offset(&code[..2], None), None);
        let mut changed = code;
        changed[3] = 0xf9000274; // store back to this, rather than the Local handle
        assert_eq!(continuation_offset(&changed, None), None);
        let mut overwritten = code.to_vec();
        overwritten.insert(4, 0xf900003f); // STR XZR,[X1] replaces the Local value.
        assert_eq!(continuation_offset(&overwritten, None), None);
        let mut unknown = code.to_vec();
        unknown.insert(4, 0xf9000032); // STR unknown X18,[X1].
        assert_eq!(continuation_offset(&unknown, None), None);
    }
    #[test]
    #[ignore = "explicit real-ELF inventory; requires AS_RUNTIME_METADATA_ELF"]
    fn actual_image_inventory() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::var("AS_RUNTIME_METADATA_ELF")?;
        let data = std::fs::read(path)?;
        let file = object::File::parse(&*data)?;
        let mut gaps = Vec::new();
        let symbols = Symbols::new(&file);
        let inventory = read(&file, &symbols, &mut gaps);
        println!("inventory={inventory:?}\ngaps={gaps:?}");
        for field in [
            "CURRENT_ISOLATE_TLS",
            "CONTINUATION_FRAME",
            "JS_COLLECTION_TABLE",
            "JS_ARRAY_LENGTH",
            "ORDERED_ELEMENTS",
            "ORDERED_DELETED",
            "ORDERED_BUCKETS",
            "ORDERED_ENTRY_VALUE",
            "ORDERED_ENTRY_SIZE",
            "ORDERED_MAP_TYPE",
            "PROPERTY_LENGTH",
            "PROPERTY_LENGTH_MASK",
            "PROPERTY_DATA",
            "NAME_DICTIONARY_TYPE",
            "NAME_DICTIONARY_CAPACITY",
            "NAME_DICTIONARY_ENTRY_BASE",
            "NAME_DICTIONARY_ENTRY_SIZE",
            "NAME_DICTIONARY_VALUE",
            "NAME_DICTIONARY_DETAILS",
        ] {
            assert!(
                inventory.contains_key(field),
                "actual-image field unavailable: {field}"
            );
        }
        Ok(())
    }
}

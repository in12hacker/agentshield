//! Actual ELF/libuv metadata; unknown members do not suppress available raw probes.
mod v8;
use as_types::main_ports::ModuleError;
use gimli::{DwarfSections, EndianSlice, RunTimeEndian, SectionId};
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol, SymbolKind, read::ReadRef};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek},
};

pub(super) const PROBES: [&str; 12] = [
    "uv_fs_open",
    "uv__work_submit",
    "uv__fs_work",
    "uv__fs_done",
    "uv_fs_req_cleanup",
    "uv_spawn",
    "uv_tcp_connect",
    "uv_write2",
    "uv_udp_send",
    "uv_write",
    "uv_try_write",
    "uv_try_write2",
];
pub(super) struct Metadata {
    pub(super) machine: u16,
    pub(super) probes: [Option<u64>; 12],
    pub(super) uv: BTreeMap<String, u32>,
    pub(super) runtime: BTreeMap<String, u64>,
    pub(super) gaps: Vec<String>,
}

/// One preparation's borrowed symbol names and actual descriptors. Equal names
/// retain their original order and every declaration for the existing conflict checks.
struct Symbol<'a> {
    name: &'a str,
    address: u64,
    size: u64,
    section: Option<object::SectionIndex>,
    kind: SymbolKind,
}

struct Symbols<'a> {
    entries: Vec<Symbol<'a>>,
}

impl<'a> Symbols<'a> {
    fn new<R: ReadRef<'a>>(file: &object::File<'a, R>) -> Self {
        let mut entries = file
            .symbols()
            .chain(file.dynamic_symbols())
            .filter(|entry| !entry.is_undefined())
            .filter_map(|entry| {
                Some(Symbol {
                    name: entry.name().ok()?,
                    address: entry.address(),
                    size: entry.size(),
                    section: entry.section_index(),
                    kind: entry.kind(),
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.name);
        Self { entries }
    }

    fn named(&self, name: &str) -> &[Symbol<'a>] {
        let start = self.entries.partition_point(|entry| entry.name < name);
        let end = self.entries.partition_point(|entry| entry.name <= name);
        &self.entries[start..end]
    }

    fn prefixed(&self, prefix: &str) -> &[Symbol<'a>] {
        let start = self.entries.partition_point(|entry| entry.name < prefix);
        let rest = &self.entries[start..];
        let end = rest.partition_point(|entry| entry.name.starts_with(prefix));
        &rest[..end]
    }
}

fn offset<'a, R: ReadRef<'a>>(file: &object::File<'a, R>, address: u64, size: u64) -> Option<u64> {
    file.segments().find_map(|segment| {
        let (start, length) = segment.file_range();
        let relative = address.checked_sub(segment.address())?;
        if relative.checked_add(size)? <= length {
            start.checked_add(relative)
        } else {
            None
        }
    })
}

fn member(fields: &mut BTreeMap<String, Option<u32>>, name: String, value: u32) {
    use std::collections::btree_map::Entry;
    match fields.entry(name) {
        Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        Entry::Occupied(mut entry) if *entry.get() != Some(value) => {
            entry.insert(None);
        }
        Entry::Occupied(_) => (),
    }
}

fn uv_layout<'a, R: ReadRef<'a>>(
    file: &object::File<'a, R>,
    gaps: &mut Vec<String>,
) -> Result<BTreeMap<String, u32>, gimli::Error> {
    let sections = DwarfSections::load(|id: SectionId| {
        Ok::<_, gimli::Error>(
            file.section_by_name(id.name())
                .and_then(|s| s.data().ok())
                .unwrap_or(&[]),
        )
    })?;
    let endian = if file.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    let dwarf = sections.borrow(|bytes| EndianSlice::new(bytes, endian));
    let mut units = dwarf.units();
    let mut result = BTreeMap::new();
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        let mut entries = unit.entries();
        let mut depth = 0isize;
        let mut owner: Option<(isize, String)> = None;
        while let Some((delta, entry)) = entries.next_dfs()? {
            depth += delta;
            if owner.as_ref().is_some_and(|(parent, _)| depth <= *parent) {
                owner = None;
            }
            let name = match entry.attr_value(gimli::DW_AT_name)? {
                Some(value) => dwarf
                    .attr_string(&unit, value)?
                    .to_string_lossy()
                    .into_owned(),
                None => String::new(),
            };
            if entry.tag() == gimli::DW_TAG_structure_type && name.starts_with("uv_") {
                owner = Some((depth, name));
            } else if entry.tag() == gimli::DW_TAG_member
                && let Some((parent, structure)) = &owner
                && depth == parent + 1
                && let Some(value) = entry
                    .attr_value(gimli::DW_AT_data_member_location)?
                    .and_then(|v| v.udata_value())
                    .and_then(|v| u32::try_from(v).ok())
            {
                member(&mut result, format!("{structure}.{name}"), value);
            }
        }
    }
    Ok(result
        .into_iter()
        .filter_map(|(name, value)| match value {
            Some(value) => Some((name, value)),
            None => {
                gaps.push(format!("libuv_dwarf_conflict:{name}"));
                None
            }
        })
        .collect())
}

pub(super) fn read(file: &mut File) -> Result<Option<Metadata>, ModuleError> {
    // All interpreters borrow one preparation-owned image. Random ReadCache
    // requests made every symbol/name traversal perform repeated file work;
    // no target context or second image survives this function.
    let length = file.metadata().map_err(|_| ModuleError::Unavailable)?.len();
    let capacity = usize::try_from(length).map_err(|_| ModuleError::Unavailable)?;
    let mut image = Vec::new();
    image
        .try_reserve_exact(capacity)
        .map_err(|_| ModuleError::Unavailable)?;
    file.rewind().map_err(|_| ModuleError::Unavailable)?;
    file.take(length)
        .read_to_end(&mut image)
        .map_err(|_| ModuleError::Unavailable)?;
    let parsed = object::File::parse(image.as_slice()).map_err(|_| ModuleError::Unsupported)?;
    let symbols = Symbols::new(&parsed);
    let mut probes = [None; 12];
    let mut gaps = Vec::new();
    for (index, name) in PROBES.iter().enumerate() {
        let mut found = None;
        let mut conflict = false;
        for entry in symbols.named(name) {
            let position = offset(&parsed, entry.address, entry.size);
            if found.is_some_and(|old| Some(old) != position) {
                conflict = true;
            }
            if let Some(position) = position {
                found = Some(position);
            }
        }
        if conflict {
            gaps.push(format!("probe_symbol_conflict:{name}"));
        } else {
            probes[index] = found;
        }
        if probes[index].is_none() {
            gaps.push(format!("probe_unavailable:{name}"));
        }
    }
    if probes.iter().all(Option::is_none) {
        return Ok(None);
    }
    let machine = match parsed.architecture() {
        object::Architecture::Aarch64 => 183,
        object::Architecture::X86_64 => 62,
        _ => 0,
    };
    let uv = match uv_layout(&parsed, &mut gaps) {
        Ok(fields) => fields,
        Err(error) => {
            gaps.push(format!("libuv_dwarf:{error}"));
            BTreeMap::new()
        }
    };
    let runtime = v8::read(&parsed, &symbols, &mut gaps);
    for field in ["uv_fs_s.result", "uv_fs_s.work_req"] {
        if !uv.contains_key(field) {
            gaps.push(format!("libuv_member_unavailable:{field}"));
        }
    }
    Ok(Some(Metadata {
        machine,
        probes,
        uv,
        runtime,
        gaps,
    }))
}

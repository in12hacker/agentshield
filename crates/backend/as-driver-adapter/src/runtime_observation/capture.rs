//! Public shared-message semantics over one immutable native technical projection.
//! The sole byte layout is documented in `drivers/linux/as_runtime_capture.h`.
//! This parser never reads a target or chooses a candidate by count, PID or time.
use as_types::{
    flow::ChatContent,
    main_ports::{ModuleError, NativeToolContext},
    process_tree::ProcessKey,
    runtime_observation::{
        CaptureAnalysis, CaptureIssue, SharedMessageCandidate, SharedMessageProof,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Property {
    key: u64,
    value: u64,
    slot: u64,
    details: u64,
    representation: u32,
    kind: u32,
}
impl Property {
    fn own_data(&self) -> bool {
        self.kind == 0 && self.representation == 0
    }
    fn semantic_error(&self) -> &'static str {
        match self.kind {
            1 => "non_data_property",
            2 => "property_kind_unavailable",
            _ => "unhandled_property_representation",
        }
    }
}
#[derive(Default)]
struct Graph {
    strings: BTreeMap<u64, Option<String>>,
    objects: BTreeMap<u64, BTreeSet<u32>>,
    properties: BTreeMap<u64, Vec<Property>>,
    scalars: BTreeMap<u64, Option<serde_json::Value>>,
    arrays: BTreeMap<u64, (u64, BTreeMap<u64, u64>)>,
    entries: BTreeMap<u64, BTreeSet<u64>>,
    maps: BTreeSet<u64>,
    complete_arguments: BTreeSet<u64>,
}
struct Input<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ModuleError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(ModuleError::InvalidInput)?;
        let value = self
            .data
            .get(self.at..end)
            .ok_or(ModuleError::InvalidInput)?;
        self.at = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, ModuleError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ModuleError::InvalidInput)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ModuleError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ModuleError::InvalidInput)?,
        ))
    }
    fn end(&self) -> Result<(), ModuleError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(ModuleError::InvalidInput)
        }
    }
}
fn unique<T: PartialEq>(map: &mut BTreeMap<u64, T>, key: u64, value: T) -> Result<(), ModuleError> {
    if map.get(&key).is_some_and(|old| *old != value) {
        return Err(ModuleError::Conflict);
    }
    map.insert(key, value);
    Ok(())
}
impl Graph {
    fn field(&self, holder: u64, name: &str) -> Result<Option<&Property>, &'static str> {
        let mut found = None;
        for property in self.properties.get(&holder).into_iter().flatten() {
            if self.strings.get(&property.key).and_then(Option::as_deref) != Some(name) {
                continue;
            }
            if found.is_some_and(|old| old != property) {
                return Err("conflicting_own_fields");
            }
            found = Some(property);
        }
        Ok(found)
    }
    fn text(&self, holder: u64, name: &str) -> Result<Option<String>, &'static str> {
        let Some(property) = self.field(holder, name)? else {
            return Ok(None);
        };
        if !property.own_data() {
            return Err(property.semantic_error());
        }
        self.strings
            .get(&property.value)
            .cloned()
            .flatten()
            .map(Some)
            .ok_or("string_not_losslessly_acquired")
    }
    fn json(
        &self,
        value: u64,
        active: &mut BTreeSet<u64>,
    ) -> Result<serde_json::Value, &'static str> {
        if let Some(value) = self.strings.get(&value) {
            return value
                .clone()
                .map(serde_json::Value::String)
                .ok_or("string_not_losslessly_acquired");
        }
        if let Some(value) = self.scalars.get(&value) {
            return value.clone().ok_or("non_json_scalar");
        }
        if !active.insert(value) {
            return Err("cyclic_argument");
        }
        let result = if let Some((length, members)) = self.arrays.get(&value) {
            if usize::try_from(*length).ok() != Some(members.len()) {
                return Err("argument_array_incomplete");
            }
            let mut array = Vec::new();
            for index in 0..*length {
                let member = members.get(&index).ok_or("argument_array_incomplete")?;
                array.push(self.json(*member, active)?);
            }
            Ok(serde_json::Value::Array(array))
        } else if self
            .objects
            .get(&value)
            .is_some_and(|scopes| scopes.contains(&3))
        {
            if !self.complete_arguments.contains(&value) {
                return Err("argument_object_incomplete");
            }
            let mut object = serde_json::Map::new();
            for property in self.properties.get(&value).into_iter().flatten() {
                if !property.own_data() {
                    return Err(property.semantic_error());
                }
                let name = self
                    .strings
                    .get(&property.key)
                    .and_then(Option::as_ref)
                    .ok_or("argument_key_not_losslessly_acquired")?;
                let item = self.json(property.value, active)?;
                match object.entry(name.clone()) {
                    serde_json::map::Entry::Vacant(entry) => {
                        entry.insert(item);
                    }
                    serde_json::map::Entry::Occupied(mut entry) => {
                        if entry.get() != &item {
                            return Err("conflicting_argument_fields");
                        }
                        entry.insert(item);
                    }
                }
            }
            Ok(serde_json::Value::Object(object))
        } else {
            Err("argument_node_not_acquired")
        };
        active.remove(&value);
        result
    }
}

/// The raw record is retained separately along the original RequestHandoff.
/// Partial, unavailable, nonfinite and unpaired UTF-16 evidence is never edited.
pub(crate) fn decode(data: &[u8], boot_id: [u8; 16]) -> Result<CaptureAnalysis, ModuleError> {
    let mut input = Input { data, at: 0 };
    let owner = input.u32()?;
    let status = input.u32()?;
    let birth = input.u64()?;
    let begin = input.u64()?;
    let end = input.u64()?;
    let _thread_pointer = input.u64()?;
    let _isolate = input.u64()?;
    let frame = input.u64()?;
    let native_error = input.u32()? as i32;
    let presence = input.u32()?;
    if owner == 0
        || status > 3
        || presence & !7 != 0
        || begin > end
        || native_error > 0
        || boot_id == [0; 16]
    {
        return Err(ModuleError::InvalidInput);
    }
    let mut graph = Graph::default();
    let mut identities = BTreeMap::new();
    let mut original_values = BTreeMap::new();
    let mut array_identities = BTreeMap::new();
    while input.at < data.len() {
        let kind = input.u32()?;
        let count = input.u32()? as usize;
        let mut value = Input {
            data: input.take(count)?,
            at: 0,
        };
        match kind {
            1 => {
                let object = value.u64()?;
                let map = value.u64()?;
                let instance_type = value.u32()?;
                let scope = value.u32()?;
                if scope > 4 {
                    return Err(ModuleError::InvalidInput);
                }
                unique(&mut identities, object, (map, instance_type))?;
                graph.objects.entry(object).or_default().insert(scope);
            }
            2 => {
                let holder = value.u64()?;
                let property = Property {
                    key: value.u64()?,
                    value: value.u64()?,
                    slot: value.u64()?,
                    details: value.u64()?,
                    representation: value.u32()?,
                    kind: value.u32()?,
                };
                if property.representation > 2 || property.kind > 2 {
                    return Err(ModuleError::InvalidInput);
                }
                let properties = graph.properties.entry(holder).or_default();
                if !properties.contains(&property) {
                    properties.push(property);
                }
            }
            3 => {
                let object = value.u64()?;
                unique(&mut original_values, object, (kind, value.data))?;
                let units = value.u32()? as usize;
                let state = value.u32()?;
                let bytes = value.u32()? as usize;
                let error = value.u32()? as i32;
                let raw = value.take(bytes)?;
                if state > 1
                    || error > 0
                    || bytes > units.checked_mul(2).ok_or(ModuleError::InvalidInput)?
                    || state == 0 && (bytes != units * 2 || error != 0)
                {
                    return Err(ModuleError::InvalidInput);
                }
                let decoded = if state == 0 {
                    let units: Vec<_> = raw
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect();
                    String::from_utf16(&units).ok()
                } else {
                    None
                };
                unique(&mut graph.strings, object, decoded)?;
            }
            4 => {
                let object = value.u64()?;
                unique(&mut original_values, object, (kind, value.data))?;
                let number = value.u64()? as i64;
                unique(
                    &mut graph.scalars,
                    object,
                    Some(serde_json::Value::Number(number.into())),
                )?;
            }
            5 => {
                let object = value.u64()?;
                unique(&mut original_values, object, (kind, value.data))?;
                let number = f64::from_bits(value.u64()?);
                unique(
                    &mut graph.scalars,
                    object,
                    serde_json::Number::from_f64(number).map(serde_json::Value::Number),
                )?;
            }
            6 => {
                let object = value.u64()?;
                unique(&mut original_values, object, (kind, value.data))?;
                let _original_kind = value.u64()?;
                let semantic = value.u32()?;
                if semantic > 4 || value.u32()? != 0 {
                    return Err(ModuleError::InvalidInput);
                }
                let item = match semantic {
                    1 => Some(false.into()),
                    2 => Some(true.into()),
                    3 => Some(serde_json::Value::Null),
                    _ => None,
                };
                unique(&mut graph.scalars, object, item)?;
            }
            7 => {
                let object = value.u64()?;
                let elements = value.u64()?;
                let length = value.u64()?;
                let capacity = value.u64()?;
                if length > capacity || capacity > i64::MAX as u64 {
                    return Err(ModuleError::InvalidInput);
                }
                unique(&mut array_identities, object, (elements, length, capacity))?;
                if let Some((old, _)) = graph.arrays.get(&object)
                    && *old != length
                {
                    return Err(ModuleError::Conflict);
                }
                graph
                    .arrays
                    .entry(object)
                    .or_insert_with(|| (length, BTreeMap::new()));
            }
            8 => {
                let object = value.u64()?;
                let index = value.u64()?;
                let member = value.u64()?;
                let _slot = value.u64()?;
                let (length, members) = graph
                    .arrays
                    .get_mut(&object)
                    .ok_or(ModuleError::InvalidInput)?;
                if index >= *length {
                    return Err(ModuleError::InvalidInput);
                }
                unique(members, index, member)?;
            }
            9 => {
                let object = value.u64()?;
                let _table = value.u64()?;
                for _ in 0..4 {
                    if value.u64()? > i64::MAX as u64 {
                        return Err(ModuleError::InvalidInput);
                    }
                }
                graph.maps.insert(object);
            }
            10 => {
                let map = value.u64()?;
                let _index = value.u64()?;
                let _key = value.u64()?;
                let store = value.u64()?;
                let _entry = value.u64()?;
                graph.entries.entry(map).or_default().insert(store);
            }
            11 => {
                let _object = value.u64()?;
                let _address = value.u64()?;
                let error = value.u32()? as i32;
                let reason = value.u32()?;
                if error > 0 || !(1..=7).contains(&reason) {
                    return Err(ModuleError::InvalidInput);
                }
            }
            12 => {
                let object = value.u64()?;
                let scope = value.u32()?;
                let complete = value.u32()?;
                if scope > 4 || complete > 1 {
                    return Err(ModuleError::InvalidInput);
                }
                if scope == 3 && complete == 1 {
                    graph.complete_arguments.insert(object);
                }
            }
            _ => return Err(ModuleError::Unsupported),
        }
        value.end()?;
    }
    let mut candidates = Vec::new();
    if presence == 7 && graph.maps.contains(&frame) {
        for store in graph.entries.get(&frame).into_iter().flatten() {
            let direct: BTreeSet<_> = graph
                .properties
                .get(store)
                .into_iter()
                .flatten()
                .filter(|p| p.own_data())
                .map(|p| p.value)
                .collect();
            for assistant in &direct {
                if graph.text(*assistant, "role").ok().flatten().as_deref() != Some("assistant") {
                    continue;
                }
                let Ok(Some(content)) = graph.field(*assistant, "content") else {
                    continue;
                };
                if !content.own_data() {
                    continue;
                }
                let Some((_, members)) = graph.arrays.get(&content.value) else {
                    continue;
                };
                for (index, tool) in members {
                    if !direct.contains(tool)
                        || graph.text(*tool, "type").ok().flatten().as_deref() != Some("toolCall")
                    {
                        continue;
                    }
                    let Ok(Some(id)) = graph.text(*tool, "id") else {
                        continue;
                    };
                    let Ok(Some(name)) = graph.text(*tool, "name") else {
                        continue;
                    };
                    let mut issues = Vec::new();
                    let mut optional = |holder, name| match graph.text(holder, name) {
                        Ok(value) => value,
                        Err(reason) => {
                            issues.push(CaptureIssue {
                                object: holder,
                                field: name.into(),
                                reason: reason.into(),
                            });
                            None
                        }
                    };
                    let response_id = optional(*assistant, "responseId");
                    let provider = optional(*assistant, "provider");
                    let api = optional(*assistant, "api");
                    let model = optional(*assistant, "model");
                    let arguments = match graph.field(*tool, "arguments") {
                        Ok(Some(field)) if field.own_data() => {
                            match graph.json(field.value, &mut BTreeSet::new()) {
                                Ok(value) => Some(ChatContent::Json(value)),
                                Err(reason) => {
                                    issues.push(CaptureIssue {
                                        object: field.value,
                                        field: "arguments".into(),
                                        reason: reason.into(),
                                    });
                                    None
                                }
                            }
                        }
                        Ok(Some(field)) => {
                            issues.push(CaptureIssue {
                                object: field.value,
                                field: "arguments".into(),
                                reason: field.semantic_error().into(),
                            });
                            None
                        }
                        Ok(None) => None,
                        Err(reason) => {
                            issues.push(CaptureIssue {
                                object: *tool,
                                field: "arguments".into(),
                                reason: reason.into(),
                            });
                            None
                        }
                    };
                    candidates.push(SharedMessageCandidate {
                        context: NativeToolContext {
                            process: ProcessKey {
                                boot_id,
                                host_tgid: owner,
                                birth_boottime_ns: birth,
                            },
                            tool_call_id: id,
                            tool_name: name,
                            response_id,
                            run_id: None,
                            session_id: None,
                            session_key: None,
                            agent_id: None,
                            provider,
                            api,
                            model,
                            arguments,
                        },
                        proof: SharedMessageProof {
                            frame,
                            store: *store,
                            tool: *tool,
                            assistant: *assistant,
                            content: content.value,
                            content_index: *index,
                            reads_consistent: status < 2,
                        },
                        issues,
                    });
                }
            }
        }
    }
    Ok(CaptureAnalysis { status, candidates })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Record {
        bytes: Vec<u8>,
        next: u64,
    }
    impl Record {
        fn new() -> Self {
            let mut bytes = Vec::new();
            bytes.extend(42_u32.to_le_bytes());
            bytes.extend(0_u32.to_le_bytes());
            for value in [123_u64, 100, 101, 800, 900, 1000] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(0_u32.to_le_bytes());
            bytes.extend(7_u32.to_le_bytes());
            Self { bytes, next: 10000 }
        }
        fn id(&mut self) -> u64 {
            self.next += 8;
            self.next
        }
        fn tlv(&mut self, kind: u32, body: &[u8]) {
            self.bytes.extend(kind.to_le_bytes());
            self.bytes.extend((body.len() as u32).to_le_bytes());
            self.bytes.extend(body);
        }
        fn words(&mut self, kind: u32, words: &[u64]) {
            let body: Vec<_> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.tlv(kind, &body);
        }
        fn object(&mut self, id: u64, scope: u32) {
            let mut body = Vec::new();
            body.extend(id.to_le_bytes());
            body.extend(55_u64.to_le_bytes());
            body.extend(1057_u32.to_le_bytes());
            body.extend(scope.to_le_bytes());
            self.tlv(1, &body);
        }
        fn raw_string(&mut self, units: &[u16]) -> u64 {
            let id = self.id();
            let mut body = Vec::new();
            body.extend(id.to_le_bytes());
            body.extend((units.len() as u32).to_le_bytes());
            body.extend(0_u32.to_le_bytes());
            body.extend((units.len() as u32 * 2).to_le_bytes());
            body.extend(0_u32.to_le_bytes());
            body.extend(units.iter().flat_map(|v| v.to_le_bytes()));
            self.tlv(3, &body);
            id
        }
        fn string(&mut self, text: &str) -> u64 {
            self.raw_string(&text.encode_utf16().collect::<Vec<_>>())
        }
        fn property(&mut self, holder: u64, name: &str, value: u64) {
            let key = self.string(name);
            let mut body = Vec::new();
            for item in [holder, key, value, holder + 88, 0] {
                body.extend(item.to_le_bytes());
            }
            body.extend(0_u32.to_le_bytes());
            body.extend(0_u32.to_le_bytes());
            self.tlv(2, &body);
        }
        fn text(&mut self, holder: u64, name: &str, text: &str) {
            let value = self.string(text);
            self.property(holder, name, value);
        }
        fn array(&mut self, array: u64, values: &[u64]) {
            self.words(
                7,
                &[array, array + 100, values.len() as u64, values.len() as u64],
            );
            for (index, value) in values.iter().enumerate() {
                self.words(
                    8,
                    &[array, index as u64, *value, array + 100 + index as u64 * 8],
                );
            }
        }
        fn end(&mut self, object: u64, complete: bool) {
            let mut body = Vec::new();
            body.extend(object.to_le_bytes());
            body.extend(3_u32.to_le_bytes());
            body.extend(u32::from(complete).to_le_bytes());
            self.tlv(12, &body);
        }
        fn pair(&mut self, store: u64, tool: u64, assistant: u64, content: u64) {
            self.object(store, 1);
            self.object(tool, 2);
            self.object(assistant, 2);
            self.property(store, "arbitrary public holder A", tool);
            self.property(store, "arbitrary public holder B", assistant);
            self.text(tool, "type", "toolCall");
            self.text(tool, "id", "id|compound");
            self.text(tool, "name", "read");
            self.text(assistant, "role", "assistant");
            self.text(assistant, "responseId", "");
            self.array(content, &[tool]);
            self.property(assistant, "content", content);
            self.words(10, &[1000, store, 444, store, 888]);
        }
        fn root(&mut self) {
            self.words(9, &[1000, 1100, 300, 2, 0, 4]);
        }
    }
    #[test]
    fn original_shared_object_and_empty_zero_false_values_survive() {
        let mut r = Record::new();
        r.root();
        r.pair(2000, 3000, 4000, 5000);
        let args = 6000;
        r.object(args, 3);
        let empty = r.string("");
        r.property(args, "", empty);
        r.words(4, &[0, 0]);
        r.property(args, "zero", 0);
        let no = 7000;
        r.words(6, &[no, 0, 1]);
        r.property(args, "false", no);
        r.array(8000, &[]);
        r.property(args, "array", 8000);
        r.object(9000, 3);
        r.end(9000, true);
        r.property(args, "object", 9000);
        r.end(args, true);
        r.property(3000, "arguments", args);
        let analysis = decode(&r.bytes, [1; 16]).unwrap();
        assert_eq!(analysis.status, 0);
        assert_eq!(analysis.candidates.len(), 1);
        let candidate = &analysis.candidates[0];
        assert_eq!(candidate.context.process.host_tgid, 42);
        assert_eq!(candidate.context.process.birth_boottime_ns, 123);
        assert_eq!(candidate.context.tool_call_id, "id|compound");
        assert_eq!(candidate.context.response_id.as_deref(), Some(""));
        assert_eq!(
            candidate.context.arguments,
            Some(ChatContent::Json(
                serde_json::json!({"":"","zero":0,"false":false,"array":[],"object":{}})
            ))
        );
        assert_eq!(
            (
                candidate.proof.store,
                candidate.proof.tool,
                candidate.proof.assistant,
                candidate.proof.content,
                candidate.proof.content_index
            ),
            (2000, 3000, 4000, 5000, 0)
        );
        assert!(candidate.proof.reads_consistent);
        assert!(candidate.issues.is_empty());
    }
    #[test]
    fn a_tool_list_without_the_same_direct_store_reference_is_not_a_match() {
        let mut r = Record::new();
        r.root();
        r.pair(2000, 3000, 4000, 5000);
        let mut graph = decode(&r.bytes, [1; 16]).unwrap();
        assert_eq!(graph.candidates.len(), 1);
        // Change only the original store's tool edge; the Assistant list still
        // contains ToolCall 3000. No count/time/latest fallback fills this gap.
        let mut at = 64;
        while at < r.bytes.len() {
            let kind = u32::from_le_bytes(r.bytes[at..at + 4].try_into().unwrap());
            let len = u32::from_le_bytes(r.bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if kind == 2
                && u64::from_le_bytes(r.bytes[at + 8..at + 16].try_into().unwrap()) == 2000
                && u64::from_le_bytes(r.bytes[at + 24..at + 32].try_into().unwrap()) == 3000
            {
                r.bytes[at + 24..at + 32].copy_from_slice(&3333_u64.to_le_bytes());
                break;
            }
            at += 8 + len;
        }
        graph = decode(&r.bytes, [1; 16]).unwrap();
        assert!(graph.candidates.is_empty());
    }
    #[test]
    fn retained_non_data_or_undecoded_fields_never_become_a_direct_tool_reference() {
        for property_kind in [1_u32, 2] {
            let mut r = Record::new();
            r.root();
            r.pair(2000, 3000, 4000, 5000);
            let mut at = 64;
            while at < r.bytes.len() {
                let kind = u32::from_le_bytes(r.bytes[at..at + 4].try_into().unwrap());
                let len = u32::from_le_bytes(r.bytes[at + 4..at + 8].try_into().unwrap()) as usize;
                if kind == 2
                    && u64::from_le_bytes(r.bytes[at + 8..at + 16].try_into().unwrap()) == 2000
                    && u64::from_le_bytes(r.bytes[at + 24..at + 32].try_into().unwrap()) == 3000
                {
                    r.bytes[at + 52..at + 56].copy_from_slice(&property_kind.to_le_bytes());
                    break;
                }
                at += 8 + len;
            }
            let original = r.bytes.clone();
            assert!(decode(&r.bytes, [1; 16]).unwrap().candidates.is_empty());
            assert_eq!(
                r.bytes, original,
                "semantic rejection preserves the acquired tuple"
            );
        }
    }
    #[test]
    fn multiple_proofs_and_inconsistent_reads_are_never_selected_implicitly() {
        let mut r = Record::new();
        r.root();
        r.pair(2000, 3000, 4000, 5000);
        r.pair(2100, 3100, 4100, 5100);
        assert_eq!(decode(&r.bytes, [1; 16]).unwrap().candidates.len(), 2);
        r.bytes[4..8].copy_from_slice(&3_u32.to_le_bytes());
        let analysis = decode(&r.bytes, [1; 16]).unwrap();
        assert_eq!(analysis.candidates.len(), 2);
        assert!(
            analysis
                .candidates
                .iter()
                .all(|c| !c.proof.reads_consistent)
        );
    }
    #[test]
    fn partial_objects_unpaired_utf16_and_nonfinite_numbers_keep_raw_gaps() {
        for variant in 0..3 {
            let mut r = Record::new();
            r.root();
            r.pair(2000, 3000, 4000, 5000);
            let argument = match variant {
                0 => {
                    r.object(6000, 3);
                    r.text(6000, "actual prefix", "");
                    r.end(6000, false);
                    6000
                }
                1 => r.raw_string(&[0xd800]),
                _ => {
                    r.words(5, &[6000, f64::NAN.to_bits()]);
                    6000
                }
            };
            r.property(3000, "arguments", argument);
            let original = r.bytes.clone();
            let analysis = decode(&r.bytes, [1; 16]).unwrap();
            let candidate = &analysis.candidates[0];
            assert_eq!(candidate.context.arguments, None);
            assert_eq!(candidate.issues.len(), 1);
            assert_eq!(candidate.issues[0].object, argument);
            assert_eq!(r.bytes, original);
        }
    }
    #[test]
    fn conflicting_same_capture_object_and_unrepresentable_bytes_are_not_overwritten() {
        let mut record = Record::new();
        record.root();
        record.pair(2000, 3000, 4000, 5000);
        let mut changed = Vec::new();
        changed.extend(3000_u64.to_le_bytes());
        changed.extend(66_u64.to_le_bytes());
        changed.extend(1057_u32.to_le_bytes());
        changed.extend(2_u32.to_le_bytes());
        record.tlv(1, &changed);
        assert!(matches!(
            decode(&record.bytes, [1; 16]),
            Err(ModuleError::Conflict)
        ));

        let mut record = Record::new();
        let original = record.raw_string(&[0xd800]);
        let mut changed = Vec::new();
        changed.extend(original.to_le_bytes());
        for value in [1_u32, 0, 2, 0] {
            changed.extend(value.to_le_bytes());
        }
        changed.extend(0xd801_u16.to_le_bytes());
        record.tlv(3, &changed);
        assert!(matches!(
            decode(&record.bytes, [1; 16]),
            Err(ModuleError::Conflict)
        ));
    }
    #[test]
    #[ignore = "explicit owned-byte component replay; requires AS_RUNTIME_CAPTURE_DIR"]
    fn actual_capture_records() -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::path::PathBuf::from(std::env::var("AS_RUNTIME_CAPTURE_DIR")?);
        for sequence in 1..=5 {
            let data = std::fs::read(directory.join(format!("projection-{sequence}.bin")))?;
            let analysis = decode(&data, [1; 16])?;
            println!(
                "sequence={sequence} status={} candidates={:?}",
                analysis.status, analysis.candidates
            );
        }
        Ok(())
    }
}

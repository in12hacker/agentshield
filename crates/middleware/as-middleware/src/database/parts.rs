//! Ordered source parts, exact native scalar IDs and original evidence byte ranges.
use super::{StoreError, row};
use as_types::flow::*;
use rusqlite::{Connection, params};

pub(super) fn write_evidence(conn: &Connection, body: &EvidenceBody) -> Result<i64, StoreError> {
    conn.execute(
        "INSERT INTO evidence_sets(original_length) VALUES(?1)",
        [body.original_length.map(|v| v.to_be_bytes().to_vec())],
    )?;
    let id = conn.last_insert_rowid();
    for (n, segment) in body.segments.iter().enumerate() {
        let (kind, offset, length, bytes, reason) = match segment {
            EvidenceSegment::Bytes { offset, data } => {
                ("bytes", *offset, None, Some(data.as_slice()), None)
            }
            EvidenceSegment::Missing {
                offset,
                length,
                reason,
            } => ("missing", *offset, *length, None, Some(reason.as_str())),
        };
        conn.execute(
            "INSERT INTO evidence_segments VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                n as i64,
                kind,
                offset.to_be_bytes().as_slice(),
                length.map(|v| v.to_be_bytes().to_vec()),
                bytes,
                reason
            ],
        )?;
    }
    Ok(id)
}
pub(super) fn read_evidence(conn: &Connection, id: i64) -> Result<EvidenceBody, StoreError> {
    read_evidence_range(conn, id, None).map(|(body, _)| body)
}

/// One owner for full originals and bounded byte pages. A page never loads the full BLOB.
pub(super) fn read_evidence_range(
    conn: &Connection,
    id: i64,
    range: Option<std::ops::Range<u64>>,
) -> Result<(EvidenceBody, Option<u64>), StoreError> {
    let length: Option<Vec<u8>> = conn.query_row(
        "SELECT original_length FROM evidence_sets WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    let mut statement = conn.prepare(
        "SELECT ordinal,kind,offset,length,CASE WHEN ?2 THEN bytes ELSE NULL END,length(bytes),reason
         FROM evidence_segments WHERE set_id=?1 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![id, range.is_none()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Vec<u8>>(2)?,
            r.get::<_, Option<Vec<u8>>>(3)?,
            r.get::<_, Option<Vec<u8>>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut page = range
        .as_ref()
        .map(|_| {
            conn.prepare(
                "SELECT substr(bytes,?3,?4) FROM evidence_segments WHERE set_id=?1 AND ordinal=?2",
            )
        })
        .transpose()?;
    let mut segments = Vec::new();
    let mut more = false;
    for entry in rows {
        let (ordinal, kind, offset, length, full_bytes, byte_length, reason) = entry?;
        let offset = u64_blob(offset)?;
        let length = length.map(u64_blob).transpose()?;
        match kind.as_str() {
            "bytes" => {
                let data_length = u64::try_from(byte_length.ok_or(StoreError::Invalid)?)
                    .map_err(|_| StoreError::Invalid)?;
                let (start, count, data) = if let Some(range) = &range {
                    let end = offset.checked_add(data_length).ok_or(StoreError::Invalid)?;
                    more |= end > range.end || offset >= range.end;
                    if !(data_length == 0 && offset >= range.start && offset < range.end)
                        && (end <= range.start || offset >= range.end)
                    {
                        continue;
                    }
                    let start = range.start.max(offset);
                    let count = end.min(range.end).saturating_sub(start);
                    let from =
                        i64::try_from(start - offset + 1).map_err(|_| StoreError::Invalid)?;
                    let count_sql = i64::try_from(count).map_err(|_| StoreError::Invalid)?;
                    let data: Vec<u8> = page
                        .as_mut()
                        .ok_or(StoreError::Invalid)?
                        .query_row(params![id, ordinal, from, count_sql], |r| r.get(0))?;
                    (start, count, data)
                } else {
                    (offset, data_length, full_bytes.ok_or(StoreError::Invalid)?)
                };
                if data.len() as u64 != count {
                    return Err(StoreError::Invalid);
                }
                segments.push(EvidenceSegment::Bytes {
                    offset: start,
                    data,
                });
            }
            "missing" => {
                let reason = reason.ok_or(StoreError::Invalid)?;
                if let Some(range) = &range {
                    let end = length
                        .map(|length| offset.checked_add(length).ok_or(StoreError::Invalid))
                        .transpose()?;
                    more |= offset >= range.end || end.is_some_and(|end| end > range.end);
                    if length.is_none() || length == Some(0) {
                        if offset >= range.start && offset < range.end {
                            segments.push(EvidenceSegment::Missing {
                                offset,
                                length,
                                reason,
                            });
                        }
                        continue;
                    }
                    if offset >= range.end || end.is_some_and(|end| end <= range.start) {
                        continue;
                    }
                    let start = offset.max(range.start);
                    segments.push(EvidenceSegment::Missing {
                        offset: start,
                        length: end.map(|end| end.min(range.end).saturating_sub(start)),
                        reason,
                    });
                } else {
                    segments.push(EvidenceSegment::Missing {
                        offset,
                        length,
                        reason,
                    });
                }
            }
            _ => return Err(StoreError::Invalid),
        }
    }
    Ok((
        EvidenceBody {
            original_length: length.map(u64_blob).transpose()?,
            segments,
        },
        range.and_then(|range| more.then_some(range.end)),
    ))
}
pub(super) fn u64_blob(value: Vec<u8>) -> Result<u64, StoreError> {
    Ok(u64::from_be_bytes(
        value.try_into().map_err(|_| StoreError::Invalid)?,
    ))
}

pub(super) fn write_native(
    conn: &Connection,
    table: &str,
    owner: i64,
    values: &[NativeProtocolId],
) -> Result<(), StoreError> {
    for (n, value) in values.iter().enumerate() {
        let (kind, text) = match &value.value {
            NativeIdValue::JsonString(v) => ("json_string", v),
            NativeIdValue::JsonNumber(v) => ("json_number", v),
        };
        conn.execute(
            &format!("INSERT INTO {table} VALUES(?1,?2,?3,?4,?5,?6)"),
            params![
                owner,
                n as i64,
                row::string(&value.namespace)?,
                value.field_path,
                kind,
                text
            ],
        )?;
    }
    Ok(())
}
pub(super) fn read_native(
    conn: &Connection,
    table: &str,
    owner: i64,
) -> Result<Vec<NativeProtocolId>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT namespace,field_path,value_kind,value FROM {table} WHERE owner=?1 ORDER BY ordinal"
    ))?;
    let rows = stmt.query_map([owner], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut values = Vec::new();
    for entry in rows {
        let (namespace, field_path, kind, value) = entry?;
        values.push(NativeProtocolId {
            namespace: row::named(&namespace)?,
            field_path,
            value: match kind.as_str() {
                "json_string" => NativeIdValue::JsonString(value),
                "json_number" => NativeIdValue::JsonNumber(value),
                _ => return Err(StoreError::Invalid),
            },
        });
    }
    Ok(values)
}
pub(super) fn quality(
    conn: &Connection,
    table: &str,
    owner: i64,
    values: &[CaptureIssue],
) -> Result<(), StoreError> {
    for (n, value) in values.iter().enumerate() {
        conn.execute(
            &format!("INSERT INTO {table} VALUES(?1,?2,?3)"),
            params![owner, n as i64, row::string(value)?],
        )?;
    }
    Ok(())
}
pub(super) fn read_quality(
    conn: &Connection,
    table: &str,
    owner: i64,
) -> Result<Vec<CaptureIssue>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT value FROM {table} WHERE owner=?1 ORDER BY ordinal"
    ))?;
    stmt.query_map([owner], |r| r.get::<_, String>(0))?
        .map(|v| row::named(&v?))
        .collect()
}
pub(super) fn content(value: &ChatContent) -> Result<(&'static str, String), StoreError> {
    Ok(match value {
        ChatContent::Text(v) => ("text", v.clone()),
        ChatContent::Json(v) => ("json", serde_json::to_string(v)?),
    })
}
pub(super) fn read_content(kind: &str, value: String) -> Result<ChatContent, StoreError> {
    match kind {
        "text" => Ok(ChatContent::Text(value)),
        "json" => Ok(ChatContent::Json(as_types::json::parse(value.as_bytes())?)),
        _ => Err(StoreError::Invalid),
    }
}
pub(super) fn write(conn: &Connection, parts: &[ChatPart]) -> Result<i64, StoreError> {
    conn.execute("INSERT INTO part_sets DEFAULT VALUES", [])?;
    let set = conn.last_insert_rowid();
    for (n, part) in parts.iter().enumerate() {
        let mut text_kind = None;
        let mut text = None;
        let mut name = None;
        let mut native = None;
        let mut stored_content = None;
        let mut reason = None;
        let mut provider = None;
        let mut media = None;
        let mut label = None;
        let (id, kind) = match part {
            ChatPart::Text {
                id,
                kind,
                text: value,
            } => {
                text_kind = Some(row::string(kind)?);
                text = Some(value.as_str());
                (*id, "text")
            }
            ChatPart::ToolCall {
                id,
                native_id,
                name: value,
                arguments,
            } => {
                native = native_id.as_ref();
                name = Some(value.as_str());
                stored_content = arguments.as_ref().map(content).transpose()?;
                (*id, "tool_call")
            }
            ChatPart::ToolResult {
                id,
                native_id,
                content: value,
            } => {
                native = native_id.as_ref();
                stored_content = Some(content(value)?);
                (*id, "tool_result")
            }
            ChatPart::Opaque {
                id,
                provider_type,
                reason: value,
            } => {
                provider = Some(provider_type.as_str());
                reason = Some(value.as_str());
                (*id, "opaque")
            }
            ChatPart::MediaRef {
                id,
                kind,
                safe_label,
            } => {
                media = Some(kind.as_str());
                label = Some(safe_label.as_str());
                (*id, "media_ref")
            }
        };
        conn.execute("INSERT INTO parts(set_id,ordinal,part_id,type,text_kind,text,name,content_present,content_kind,content_value,reason,provider_type,media_kind,safe_label) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![set,n as i64,id,kind,text_kind,text,name,stored_content.is_some(),stored_content.as_ref().map(|v|v.0),stored_content.as_ref().map(|v|v.1.as_str()),reason,provider,media,label])?;
        if let Some(native) = native {
            write_native(
                conn,
                "part_native_ids",
                conn.last_insert_rowid(),
                std::slice::from_ref(native),
            )?;
        }
    }
    Ok(set)
}
pub(super) fn read(conn: &Connection, set: i64) -> Result<Vec<ChatPart>, StoreError> {
    let mut stmt=conn.prepare("SELECT id,part_id,type,text_kind,text,name,content_present,content_kind,content_value,reason,provider_type,media_kind,safe_label FROM parts WHERE set_id=?1 ORDER BY ordinal")?;
    let rows = stmt.query_map([set], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, Option<String>>(10)?,
            r.get::<_, Option<String>>(11)?,
            r.get::<_, Option<String>>(12)?,
        ))
    })?;
    let mut out = Vec::new();
    for entry in rows {
        let (
            owner,
            id,
            kind,
            text_kind,
            text,
            name,
            present,
            content_kind,
            content_value,
            reason,
            provider,
            media,
            label,
        ) = entry?;
        let content = if present {
            Some(match (content_kind.as_deref(), content_value) {
                (Some(kind), Some(v)) => read_content(kind, v)?,
                _ => return Err(StoreError::Invalid),
            })
        } else {
            None
        };
        let natives = read_native(conn, "part_native_ids", owner)?;
        if natives.len() > 1 {
            return Err(StoreError::Invalid);
        }
        let native_id = natives.into_iter().next();
        out.push(match kind.as_str() {
            "text" => ChatPart::Text {
                id,
                kind: row::named(&text_kind.ok_or(StoreError::Invalid)?)?,
                text: text.ok_or(StoreError::Invalid)?,
            },
            "tool_call" => ChatPart::ToolCall {
                id,
                native_id,
                name: name.ok_or(StoreError::Invalid)?,
                arguments: content,
            },
            "tool_result" => ChatPart::ToolResult {
                id,
                native_id,
                content: content.ok_or(StoreError::Invalid)?,
            },
            "opaque" => ChatPart::Opaque {
                id,
                provider_type: provider.ok_or(StoreError::Invalid)?,
                reason: reason.ok_or(StoreError::Invalid)?,
            },
            "media_ref" => ChatPart::MediaRef {
                id,
                kind: media.ok_or(StoreError::Invalid)?,
                safe_label: label.ok_or(StoreError::Invalid)?,
            },
            _ => return Err(StoreError::Invalid),
        });
    }
    Ok(out)
}

pub(super) fn ensure_reference(
    conn: &Connection,
    reference: &crate::context::FactReference,
    kind: &str,
) -> Result<String, StoreError> {
    let reference = super::key(reference)?;
    conn.execute(
        "INSERT OR IGNORE INTO facts(reference,kind) VALUES(?1,?2)",
        params![reference, kind],
    )?;
    Ok(reference)
}

pub(super) fn reference(
    conn: &Connection,
    reference: &crate::context::FactReference,
    kind: &str,
    id: i64,
) -> Result<(), StoreError> {
    let reference = ensure_reference(conn, reference, kind)?;
    conn.execute(
        "INSERT OR IGNORE INTO fact_records VALUES(?1,?2,?3)",
        params![reference, kind, id],
    )?;
    Ok(())
}

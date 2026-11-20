//! Selected fields of the OTLP v1 trace schema. The original HTTP payload is retained unchanged.
use super::Span;
use as_types::process_tree::ProcessKey;
use serde_json::Value;
use std::{collections::HashMap, io::Read};

fn varint(input: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, next) = input.split_first()?;
        *input = next;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Some(value);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Field<'a> {
    Bytes(&'a [u8]),
    Number(u64),
    Other,
}
fn field<'a>(input: &mut &'a [u8]) -> Option<(u64, Field<'a>)> {
    let tag = varint(input)?;
    if tag >> 3 == 0 {
        return None;
    }
    let value = match tag & 7 {
        0 => Field::Number(varint(input)?),
        1 => {
            *input = input.get(8..)?;
            Field::Other
        }
        2 => {
            let length = usize::try_from(varint(input)?).ok()?;
            let (value, rest) = input.split_at_checked(length)?;
            *input = rest;
            Field::Bytes(value)
        }
        5 => {
            *input = input.get(4..)?;
            Field::Other
        }
        _ => return None,
    };
    Some((tag >> 3, value))
}

fn bytes_fields(mut input: &[u8], number: u64) -> Option<Vec<&[u8]>> {
    let mut out = Vec::new();
    while !input.is_empty() {
        let (id, value) = field(&mut input)?;
        if id == number {
            let Field::Bytes(value) = value else {
                return None;
            };
            out.push(value);
        }
    }
    Some(out)
}

fn id(value: &[u8], length: usize) -> Option<String> {
    (value.len() == length && value.iter().any(|&v| v != 0))
        .then(|| value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn span(mut input: &[u8], process: ProcessKey) -> Option<Span> {
    let (mut trace_id, mut span_id, mut parent_span_id, mut kind) = (None, None, None, None);
    let mut attributes = HashMap::new();
    while !input.is_empty() {
        match field(&mut input)? {
            (1, Field::Bytes(value)) if trace_id.is_none() => trace_id = Some(id(value, 16)?),
            (2, Field::Bytes(value)) if span_id.is_none() => span_id = Some(id(value, 8)?),
            (4, Field::Bytes(value)) if parent_span_id.is_none() => {
                parent_span_id = Some(if value.is_empty() {
                    None
                } else {
                    Some(id(value, 8)?)
                })
            }
            (6, Field::Number(value)) if kind.is_none() => kind = Some(u8::try_from(value).ok()?),
            (9, Field::Bytes(value)) => {
                let [key] = bytes_fields(value, 1)?[..] else {
                    return None;
                };
                let key = std::str::from_utf8(key).ok()?;
                if !matches!(
                    key,
                    "http.request.method"
                        | "url.full"
                        | "url.path"
                        | "server.address"
                        | "server.port"
                        | "network.peer.port"
                        | "http.response.status_code"
                        | "gen_ai.operation.name"
                        | "gen_ai.tool.call.id"
                        | "gen_ai.conversation.id"
                        | "gen_ai.response.id"
                        | "gen_ai.task.id"
                ) {
                    continue;
                }
                let [mut value] = bytes_fields(value, 2)?[..] else {
                    return None;
                };
                let decoded = match field(&mut value)? {
                    (1, Field::Bytes(text)) => {
                        Value::String(std::str::from_utf8(text).ok()?.to_owned())
                    }
                    (3, Field::Number(number)) => Value::from(number as i64),
                    _ => return None,
                };
                if !value.is_empty() || attributes.insert(key.to_owned(), decoded).is_some() {
                    return None;
                }
            }
            (1 | 2 | 4 | 6 | 9, _) => return None,
            _ => {}
        }
    }
    Some(Span {
        process,
        trace_id: trace_id?,
        span_id: span_id?,
        parent_span_id: parent_span_id.flatten(),
        kind: kind.unwrap_or(0),
        attributes,
    })
}

pub(super) fn decode(
    body: &[u8],
    encoding: Option<&str>,
    process: ProcessKey,
) -> Option<Vec<(Span, [u8; 32])>> {
    use sha2::{Digest, Sha256};
    let mut decoded = Vec::new();
    let body = match encoding {
        Some("gzip") => {
            flate2::read::MultiGzDecoder::new(body)
                .take(super::http::MESSAGE_MAX as u64 + 1)
                .read_to_end(&mut decoded)
                .ok()?;
            if decoded.len() > super::http::MESSAGE_MAX {
                return None;
            }
            decoded.as_slice()
        }
        None | Some("identity") => body,
        _ => return None,
    };
    let mut out = Vec::new();
    for resource in bytes_fields(body, 1)? {
        for scope in bytes_fields(resource, 2)? {
            for original in bytes_fields(scope, 2)? {
                out.push((span(original, process)?, Sha256::digest(original).into()));
            }
        }
    }
    Some(out)
}

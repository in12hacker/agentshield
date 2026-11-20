//! Provider decoding of complete JSON units, independent of transport framing.
use crate::message::{Item, usage};
use as_types::flow::{
    ChatRole, ChatState, NativeIdNamespace, NativeProtocolId, ProviderProfile, TextKind,
};
use indexmap::IndexMap;
use serde_json::Value;

pub(crate) struct Codec {
    profile: ProviderProfile,
    pub items: IndexMap<String, Item>,
    pub provider_failed: bool,
}
impl Codec {
    pub fn new(profile: ProviderProfile) -> Self {
        Self {
            profile,
            items: IndexMap::new(),
            provider_failed: false,
        }
    }
    pub fn request(&mut self, v: &Value, ids: &[NativeProtocolId]) {
        let mut out = Vec::new();
        if let Some(instructions) = v.get("instructions").or_else(|| v.get("system")) {
            out.push((
                "system".into(),
                request_item("system", instructions, &[], ""),
            ));
        }
        let input = match self.profile {
            ProviderProfile::OpenAiResponses => v.get("input"),
            ProviderProfile::OpenAiChatCompletions | ProviderProfile::AnthropicMessages => {
                v.get("messages")
            }
        };
        if let Some(value @ Value::String(_)) = input {
            out.push(("input".into(), request_item("user", value, &[], "")));
        }
        if let Some(Value::Array(messages)) = input {
            for (n, msg) in messages.iter().enumerate() {
                let role = match msg.get("role").and_then(Value::as_str) {
                    Some(role) => role,
                    None => match msg.get("type").and_then(Value::as_str) {
                        Some("function_call_output") => "tool",
                        Some("function_call") => "assistant",
                        _ => continue,
                    },
                };
                let anthropic_ids = if self.profile == ProviderProfile::AnthropicMessages {
                    ids
                } else {
                    &[]
                };
                let mut item = request_item(
                    role,
                    msg.get("content").unwrap_or(msg),
                    anthropic_ids,
                    &format!("/messages/{n}/content"),
                );
                if msg.get("type").and_then(Value::as_str) == Some("function_call_output") {
                    item.set_role(ChatRole::Tool);
                    item.clear_parts();
                    if let Some(content) = msg.get("output") {
                        item.tool_result(0, content);
                        bind(
                            &mut item,
                            0,
                            ids,
                            &format!("/input/{n}/call_id"),
                            NativeIdNamespace::ToolResultReference,
                        );
                    }
                }
                if msg.get("type").and_then(Value::as_str) == Some("function_call") {
                    item.clear_parts();
                    item.tool(
                        0,
                        msg.get("name").and_then(Value::as_str),
                        msg.get("arguments"),
                        false,
                    );
                    bind(
                        &mut item,
                        0,
                        ids,
                        &format!("/input/{n}/call_id"),
                        NativeIdNamespace::ToolCall,
                    );
                }
                if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    let base = item.parts.last_key_value().map_or(0, |(&id, _)| id + 1);
                    for (offset, call) in calls.iter().enumerate() {
                        item.tool(
                            base + offset as u32,
                            call.pointer("/function/name").and_then(Value::as_str),
                            call.pointer("/function/arguments"),
                            false,
                        );
                        bind(
                            &mut item,
                            base + offset as u32,
                            ids,
                            &format!("/messages/{n}/tool_calls/{offset}/id"),
                            NativeIdNamespace::ToolCall,
                        );
                    }
                }
                if msg.get("tool_call_id").is_some() {
                    item.clear_parts();
                    if let Some(content) = msg.get("content") {
                        item.tool_result(0, content);
                        bind(
                            &mut item,
                            0,
                            ids,
                            &format!("/messages/{n}/tool_call_id"),
                            NativeIdNamespace::ToolResultReference,
                        );
                    }
                }
                out.push((format!("input-{n}"), item));
            }
        }
        self.items.extend(out);
    }
    pub fn response(
        &mut self,
        v: &Value,
        event: Option<&str>,
        streaming: bool,
        ids: &[NativeProtocolId],
    ) {
        let response = v.get("response").unwrap_or(v);
        self.provider_failed = v.get("error").is_some_and(|e| !e.is_null())
            || response.get("error").is_some_and(|e| !e.is_null())
            || event == Some("error")
            || v.get("type").and_then(Value::as_str) == Some("error")
            || (self.profile == ProviderProfile::OpenAiResponses
                && response.get("status").and_then(Value::as_str) == Some("failed"));
        match self.profile {
            ProviderProfile::OpenAiChatCompletions => self.chat(v, streaming, ids),
            ProviderProfile::OpenAiResponses => self.responses(v, event, streaming, ids),
            ProviderProfile::AnthropicMessages => self.anthropic(v, event, streaming, ids),
        }
        if self.provider_failed {
            for item in self.items.values_mut() {
                if item.state == ChatState::Streaming {
                    item.set_state(ChatState::Failed);
                }
            }
        }
    }
    fn chat(&mut self, v: &Value, streaming: bool, ids: &[NativeProtocolId]) {
        if let Some(choices) = v.get("choices").and_then(Value::as_array) {
            for (choice_position, choice) in choices.iter().enumerate() {
                let Some(index) = choice
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|x| u32::try_from(x).ok())
                else {
                    continue;
                };
                let item = self
                    .items
                    .entry(format!("choice-{index}"))
                    .or_insert_with(|| Item::new(ChatRole::Assistant, strval(v, "model")));
                item.touch();
                item.set_candidate(index);
                let field = if choice.get("delta").is_some() {
                    "delta"
                } else {
                    "message"
                };
                let d = choice.get(field).unwrap_or(&Value::Null);
                if let Some(role) = d.get("role").and_then(Value::as_str) {
                    item.set_role(provider_role(role));
                }
                if let Some(s) = d.get("content").and_then(Value::as_str) {
                    item.text(0, TextKind::Plain, s, streaming);
                }
                if let Some(s) = d.get("refusal").and_then(Value::as_str) {
                    item.text(1, TextKind::Refusal, s, streaming);
                }
                if let Some(calls) = d.get("tool_calls").and_then(Value::as_array) {
                    for (ordinal, call) in calls.iter().enumerate() {
                        let index = if streaming {
                            let Some(index) = call
                                .get("index")
                                .and_then(Value::as_u64)
                                .and_then(|n| usize::try_from(n).ok())
                            else {
                                continue;
                            };
                            index
                        } else {
                            ordinal
                        };
                        if index < 1024 {
                            item.tool(
                                index as u32 + 2,
                                call.pointer("/function/name").and_then(Value::as_str),
                                call.pointer("/function/arguments"),
                                streaming,
                            );
                            bind(
                                item,
                                index as u32 + 2,
                                ids,
                                &format!(
                                    "/choices/{choice_position}/{field}/tool_calls/{ordinal}/id"
                                ),
                                NativeIdNamespace::ToolCall,
                            );
                        }
                    }
                }
                if let Some(s) = choice.get("finish_reason").and_then(Value::as_str) {
                    item.set_finish(Some(s.into()));
                    item.set_state(ChatState::Complete);
                }
                if !streaming {
                    item.set_state(ChatState::Complete);
                }
            }
        }
        if let Some(usage) = v.get("usage").filter(|u| !u.is_null()) {
            for i in self.items.values_mut() {
                i.set_usage(Some(crate::message::usage(usage)));
            }
        }
    }
    fn responses(
        &mut self,
        v: &Value,
        event: Option<&str>,
        streaming: bool,
        ids: &[NativeProtocolId],
    ) {
        let ty = v
            .get("type")
            .and_then(Value::as_str)
            .or(event)
            .unwrap_or("");
        let response = v.get("response").unwrap_or(v);
        if !streaming
            || matches!(
                ty,
                "response.completed" | "response.failed" | "response.incomplete"
            )
        {
            if let Some(output) = response.get("output").and_then(Value::as_array) {
                for (n, o) in output.iter().enumerate() {
                    let path = if v.get("response").is_some() {
                        format!("/response/output/{n}/call_id")
                    } else {
                        format!("/output/{n}/call_id")
                    };
                    self.response_item(n, o, response, ids, &path);
                }
            }
            if !streaming || !ty.is_empty() {
                for i in self.items.values_mut() {
                    i.set_provider_status(strval(response, "status"));
                    let state = match i.provider_status.as_deref() {
                        Some("completed") => ChatState::Complete,
                        Some("incomplete" | "cancelled") => ChatState::Interrupted,
                        Some("failed") => ChatState::Failed,
                        Some(_) => ChatState::Streaming,
                        None => match ty {
                            "response.failed" => ChatState::Failed,
                            "response.incomplete" => ChatState::Interrupted,
                            _ => ChatState::Complete,
                        },
                    };
                    i.set_state(state);
                    i.set_usage(response.get("usage").map(usage));
                    // Responses has a status field, not Chat Completions' finish_reason.
                    i.set_finish(None);
                }
            }
            return;
        }
        let Some(index) = v
            .get("output_index")
            .and_then(Value::as_u64)
            .and_then(|x| usize::try_from(x).ok())
            .filter(|x| *x < 1024)
        else {
            return;
        };
        if matches!(
            ty,
            "response.output_item.added" | "response.output_item.done"
        ) {
            if let Some(o) = v.get("item") {
                self.response_item(index, o, response, ids, "/item/call_id");
            }
            return;
        }
        let item = self
            .items
            .entry(format!("output-{index}"))
            .or_insert_with(|| Item::new(ChatRole::Assistant, strval(response, "model")));
        let part = v.get("content_index").and_then(Value::as_u64).unwrap_or(0);
        if part >= 1024 {
            return;
        }
        let kind = if matches!(ty, "response.refusal.delta" | "response.refusal.done") {
            TextKind::Refusal
        } else {
            TextKind::Plain
        };
        match ty {
            "response.output_text.delta" | "response.refusal.delta" => {
                if let Some(s) = v.get("delta").and_then(Value::as_str) {
                    item.text(part as u32, kind, s, true);
                }
            }
            "response.output_text.done" | "response.refusal.done" => {
                if let Some(s) = v
                    .get("text")
                    .or_else(|| v.get("refusal"))
                    .and_then(Value::as_str)
                {
                    item.text(part as u32, kind, s, false);
                }
            }
            "response.function_call_arguments.delta" => item.tool(0, None, v.get("delta"), true),
            "response.function_call_arguments.done" => {
                item.tool(0, None, v.get("arguments"), false)
            }
            _ => {}
        }
    }
    fn response_item(
        &mut self,
        n: usize,
        o: &Value,
        response: &Value,
        ids: &[NativeProtocolId],
        path: &str,
    ) {
        let key = format!("output-{n}");
        let mut item = Item::new(
            provider_role(o.get("role").and_then(Value::as_str).unwrap_or("assistant")),
            strval(response, "model"),
        );
        match o.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                item.tool(
                    0,
                    o.get("name").and_then(Value::as_str),
                    o.get("arguments"),
                    false,
                );
                bind(&mut item, 0, ids, path, NativeIdNamespace::ToolCall);
            }
            Some("message") => {
                if let Some(c) = o.get("content").and_then(Value::as_array) {
                    for (n, c) in c.iter().enumerate() {
                        if let Some(t) = c
                            .get("text")
                            .or_else(|| c.get("refusal"))
                            .and_then(Value::as_str)
                        {
                            item.text(n as u32, text_kind(c), t, false);
                        }
                    }
                }
            }
            _ => item.opaque(0, o.get("type")),
        }
        match self.items.entry(key) {
            indexmap::map::Entry::Occupied(mut entry) => entry.get_mut().replace(item),
            indexmap::map::Entry::Vacant(entry) => {
                entry.insert(item);
            }
        }
    }
    fn anthropic(
        &mut self,
        v: &Value,
        event: Option<&str>,
        streaming: bool,
        ids: &[NativeProtocolId],
    ) {
        let ty = v
            .get("type")
            .and_then(Value::as_str)
            .or(event)
            .unwrap_or("");
        if ty == "ping" {
            return;
        }
        let msg = v.get("message").unwrap_or(v);
        let item = self
            .items
            .entry("message".into())
            .or_insert_with(|| Item::new(ChatRole::Assistant, strval(msg, "model")));
        item.touch();
        if ty == "message_start" || !streaming {
            item.set_model(strval(msg, "model"));
            item.set_usage(msg.get("usage").map(usage));
        }
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        if index >= 1024 {
            return;
        }
        if ty == "content_block_start" {
            if let Some(c) = v.get("content_block") {
                anthropic_block(item, index as u32, c, ids, "/content_block/id");
            }
        } else if ty == "content_block_delta" {
            if let Some(c) = v.get("delta") {
                match c.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(s) = c.get("text").and_then(Value::as_str) {
                            item.text(index as u32, TextKind::Plain, s, true);
                        }
                    }
                    Some("input_json_delta") => {
                        item.tool(index as u32, None, c.get("partial_json"), true)
                    }
                    _ => {}
                }
            }
        } else if ty == "message_delta" {
            item.set_finish(
                v.pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .map(String::from),
            );
            if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                let mut counters = item.usage.clone().unwrap_or_default();
                for counter in usage(u) {
                    match counters.binary_search_by(|old| old.name.cmp(&counter.name)) {
                        Ok(i) => counters[i] = counter,
                        Err(i) => counters.insert(i, counter),
                    }
                }
                item.set_usage(Some(counters));
            }
        } else if ty == "message_stop" {
            item.set_state(ChatState::Complete);
        }
        if !streaming {
            if let Some(c) = v.get("content").and_then(Value::as_array) {
                for (n, c) in c.iter().enumerate() {
                    anthropic_block(item, n as u32, c, ids, &format!("/content/{n}/id"));
                }
            }
            item.set_state(ChatState::Complete);
            item.set_finish(strval(v, "stop_reason"));
        }
    }
}
fn anthropic_block(item: &mut Item, n: u32, c: &Value, ids: &[NativeProtocolId], path: &str) {
    match c.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(t) = c.get("text").and_then(Value::as_str) {
                item.text(n, TextKind::Plain, t, false);
            }
        }
        Some("tool_use") => {
            item.tool(
                n,
                c.get("name").and_then(Value::as_str),
                c.get("input"),
                false,
            );
            bind(item, n, ids, path, NativeIdNamespace::ToolCall);
        }
        _ => item.opaque(n, c.get("type")),
    }
}
fn strval(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}
fn text_kind(value: &Value) -> TextKind {
    if value.get("type").and_then(Value::as_str) == Some("refusal") {
        TextKind::Refusal
    } else {
        TextKind::Plain
    }
}

fn request_item(role: &str, content: &Value, ids: &[NativeProtocolId], path: &str) -> Item {
    let mut item = Item::new(provider_role(role), None);
    item.set_state(ChatState::Complete);
    match content {
        Value::String(s) => item.text(0, TextKind::Plain, s, false),
        Value::Array(parts) => {
            for (n, p) in parts.iter().enumerate() {
                let n = n as u32;
                match p.get("type").and_then(Value::as_str) {
                    Some("text" | "input_text" | "output_text" | "refusal") => {
                        if let Some(t) = p
                            .get("text")
                            .or_else(|| p.get("refusal"))
                            .and_then(Value::as_str)
                        {
                            item.text(n, text_kind(p), t, false);
                        }
                    }
                    Some("tool_use") => {
                        item.tool(
                            n,
                            p.get("name").and_then(Value::as_str),
                            p.get("input"),
                            false,
                        );
                        bind(
                            &mut item,
                            n,
                            ids,
                            &format!("{path}/{n}/id"),
                            NativeIdNamespace::ToolCall,
                        );
                    }
                    Some("tool_result") => {
                        if let Some(content) = p.get("content") {
                            item.tool_result(n, content);
                            bind(
                                &mut item,
                                n,
                                ids,
                                &format!("{path}/{n}/tool_use_id"),
                                NativeIdNamespace::ToolResultReference,
                            );
                        }
                    }
                    _ => item.opaque(n, p.get("type")),
                }
            }
        }
        _ => item.opaque(0, content.get("type")),
    }
    item
}

fn bind(
    item: &mut Item,
    part: u32,
    ids: &[NativeProtocolId],
    path: &str,
    namespace: NativeIdNamespace,
) {
    if let Some(native) = ids
        .iter()
        .find(|id| id.field_path == path && id.namespace == namespace)
    {
        item.bind(part, native);
    }
}

/// WHATWG SSE line handling, preserving exact event bytes separately from joined data.
pub(crate) struct Sse {
    bytes: Vec<u8>,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Option<String>,
    saw_cr: bool,
    pending: Option<SseUnit>,
    limit: usize,
}
pub(crate) struct SseUnit {
    pub raw: Vec<u8>,
    pub data: Vec<u8>,
    pub event: Option<String>,
}
impl Sse {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            line: Vec::new(),
            data: Vec::new(),
            event: None,
            saw_cr: false,
            pending: None,
            limit,
        }
    }
    pub fn feed(&mut self, input: &[u8]) -> Vec<Result<SseUnit, &'static str>> {
        let mut out = Vec::new();
        for &b in input {
            if let Some(mut pending) = self.pending.take() {
                if self.saw_cr && b == b'\n' {
                    pending.raw.push(b);
                    out.push(Ok(pending));
                    self.saw_cr = false;
                    continue;
                }
                out.push(Ok(pending));
                self.saw_cr = false;
            }
            if self.bytes.len() >= self.limit {
                out.push(Err("sse_event_too_large"));
                break;
            }
            self.bytes.push(b);
            if self.saw_cr {
                self.saw_cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            if b == b'\r' || b == b'\n' {
                if self.line.is_empty() {
                    if !self.data.is_empty() {
                        self.data.pop();
                        let unit = SseUnit {
                            raw: std::mem::take(&mut self.bytes),
                            data: std::mem::take(&mut self.data),
                            event: self.event.take(),
                        };
                        if b == b'\r' {
                            self.pending = Some(unit);
                        } else {
                            out.push(Ok(unit));
                        }
                    } else {
                        self.bytes.clear();
                        self.event = None;
                    }
                } else {
                    let line = std::mem::take(&mut self.line);
                    let line = line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&line);
                    let colon = line.iter().position(|x| *x == b':');
                    let (key, value) = match colon {
                        Some(n) => (&line[..n], &line[n + 1..]),
                        None => (line, &[][..]),
                    };
                    let value = value.strip_prefix(b" ").unwrap_or(value);
                    if key == b"data" {
                        self.data.extend_from_slice(value);
                        self.data.push(b'\n');
                    }
                    if key == b"event" {
                        match std::str::from_utf8(value) {
                            Ok(event) => self.event = Some(event.to_owned()),
                            Err(_) => {
                                out.push(Err("invalid_utf8"));
                                break;
                            }
                        }
                    }
                }
                self.saw_cr = b == b'\r';
            } else {
                self.line.push(b);
            }
        }
        out
    }
    pub fn finish(&mut self) -> Option<SseUnit> {
        self.pending.take()
    }
    pub fn incomplete(&self) -> bool {
        !self.line.is_empty() || !self.data.is_empty()
    }
}

fn provider_role(value: &str) -> ChatRole {
    match value {
        "system" => ChatRole::System,
        "developer" => ChatRole::Developer,
        "user" => ChatRole::User,
        "assistant" => ChatRole::Assistant,
        "tool" => ChatRole::Tool,
        _ => ChatRole::Other(value.into()),
    }
}

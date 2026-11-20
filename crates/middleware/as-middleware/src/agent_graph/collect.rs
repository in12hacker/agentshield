//! In-flight TCP/HTTP state owned by the middleware consumer.
use super::{Exchange, Span, http, otlp, websocket};
use crate::gateway::GatewayConnection;
#[path = "collect_gateway.rs"]
mod gateway;
use crate::database::{BusinessLog, StoreError};
use as_types::{
    configuration::GroupObservation,
    driver::{NativeGroupObservation, NativeGroupTcp},
    process_tree::ProcessKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    num::NonZeroU64,
};

const FIN: u8 = 1;
const SYN: u8 = 2;
const RST: u8 = 4;
const ACK: u8 = 16;
const MAX_CONNECTIONS: usize = 256;
const MAX_FRAGMENTS: usize = 4096;
const MAX_PENDING: usize = 32;
const UNFRAMED_BATCH: usize = 16;

type Ports = (u16, u16);
type Source = ([u8; 16], u64);
type OriginalMessage = Vec<u8>;

#[derive(Serialize, Deserialize)]
struct ConnectionEvidence {
    source: [u8; 16],
    generation: u64,
    first_syn_ordinal: u64,
    request_offset: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RecordedExchange {
    pub(crate) capture: [u8; 32],
    connection: ConnectionEvidence,
    pub(crate) parent: ProcessKey,
    pub(crate) child: ProcessKey,
    pub(super) exchange: Exchange,
}

pub(crate) enum Intake {
    A2a(Box<RecordedExchange>, OriginalMessage, OriginalMessage),
    Otlp(ProcessKey, OriginalMessage, Vec<(Span, [u8; 32])>),
    GatewayHandshake(Box<GatewayConnection>, OriginalMessage, OriginalMessage),
    GatewayMessage {
        connection: [u8; 32],
        from_client: bool,
        sender: ProcessKey,
        receiver: ProcessKey,
        message: websocket::Message,
    },
    GatewayGap {
        connection: [u8; 32],
        from_client: bool,
        frames: Vec<websocket::WireFrame>,
        reason: &'static str,
    },
    GatewayUnverified {
        source: [u8; 16],
        generation: u64,
        first_syn_ordinal: u64,
        client_port: u16,
        server_port: u16,
        from_client: bool,
        frames: Vec<websocket::WireFrame>,
        reason: &'static str,
    },
}

#[derive(Default)]
struct Direction {
    initial: Option<u32>,
    offset: u64,
    packets: BTreeMap<u64, Vec<u8>>,
    assembled: Vec<u8>,
    end: Option<u64>,
    sent: BTreeMap<u64, (u64, ProcessKey)>,
    received: BTreeMap<u64, (u64, ProcessKey)>,
}

impl Direction {
    // TCP serial arithmetic around the current consumed position; the retained window is <2^31.
    fn position(&self, sequence: u32) -> Result<u64, ()> {
        let current = self.initial.ok_or(())?.wrapping_add(self.offset as u32);
        let delta = i64::from(sequence.wrapping_sub(current) as i32);
        self.offset.checked_add_signed(delta).ok_or(())
    }

    fn retained(&self) -> usize {
        self.assembled.len() + self.packets.values().map(Vec::len).sum::<usize>()
    }

    fn packet(&mut self, start: u64, payload: &mut Vec<u8>) -> Result<(), ()> {
        let end = start.checked_add(payload.len() as u64).ok_or(())?;
        if end <= self.offset {
            return Ok(());
        }
        if end - self.offset > http::MESSAGE_MAX as u64 {
            return Err(());
        }
        let start = if start < self.offset {
            payload.drain(..(self.offset - start) as usize);
            self.offset
        } else {
            start
        };
        let contiguous_end = self
            .offset
            .checked_add(self.assembled.len() as u64)
            .ok_or(())?;
        let shared = end.min(contiguous_end);
        if shared > start
            && self.assembled[(start - self.offset) as usize..(shared - self.offset) as usize]
                != payload[..(shared - start) as usize]
        {
            return Err(());
        }
        if end <= contiguous_end {
            return Ok(());
        }
        if let Some(prior) = self.packets.get(&start) {
            if prior == payload {
                return Ok(());
            }
            return Err(());
        }
        if self.packets.len() == MAX_FRAGMENTS
            || self.retained() + payload.len() > http::MESSAGE_MAX
        {
            return Err(());
        }
        self.packets.insert(start, std::mem::take(payload));
        while let Some((&at, next)) = self.packets.first_key_value() {
            let current = self.offset + self.assembled.len() as u64;
            if at > current {
                break;
            }
            let overlap = ((current - at) as usize).min(next.len());
            let from = (at - self.offset) as usize;
            if next[..overlap] != self.assembled[from..from + overlap] {
                return Err(());
            }
            self.assembled.extend_from_slice(&next[overlap..]);
            self.packets.pop_first();
        }
        Ok(())
    }

    fn transfer(
        &mut self,
        received: bool,
        start: u64,
        length: u32,
        actor: ProcessKey,
    ) -> Result<(), ()> {
        let end = start.checked_add(u64::from(length)).ok_or(())?;
        if end <= self.offset {
            return Ok(());
        }
        if end - self.offset > http::MESSAGE_MAX as u64 {
            return Err(());
        }
        let ranges = if received {
            &mut self.received
        } else {
            &mut self.sent
        };
        let start = start.max(self.offset);
        if ranges.len() == MAX_FRAGMENTS
            || ranges
                .range(..=start)
                .next_back()
                .is_some_and(|(_, (e, _))| *e > start)
            || ranges.range(start..).next().is_some_and(|(s, _)| *s < end)
        {
            return Err(());
        }
        ranges.insert(start, (end, actor));
        Ok(())
    }

    fn owner(&self, len: usize, received: bool) -> Result<Option<ProcessKey>, ()> {
        let ranges = if received { &self.received } else { &self.sent };
        let mut cursor = self.offset;
        let end = cursor.checked_add(len as u64).ok_or(())?;
        let mut owner = None;
        for (&start, &(next, actor)) in ranges.range(..end) {
            if start != cursor {
                return Ok(None);
            }
            if owner.is_some_and(|owner| owner != actor) {
                return Err(());
            }
            owner = Some(actor);
            cursor = next;
            if cursor >= end {
                return Ok(owner);
            }
        }
        Ok(None)
    }

    fn consume(&mut self, len: usize) -> OriginalMessage {
        let remaining = self.assembled.split_off(len);
        let original = std::mem::replace(&mut self.assembled, remaining);
        self.offset += len as u64;
        for ranges in [&mut self.sent, &mut self.received] {
            while let Some((&start, &(end, owner))) = ranges.first_key_value() {
                if start >= self.offset {
                    break;
                }
                ranges.pop_first();
                if end > self.offset {
                    ranges.insert(self.offset, (end, owner));
                }
            }
        }
        original
    }
}

struct Pending {
    offset: u64,
    raw: OriginalMessage,
    parent: ProcessKey,
    receiver: Option<ProcessKey>,
}
struct Connection {
    first_syn: u64,
    established: bool,
    first_send_cookie: Option<u64>,
    client_socket_cookie: Option<NonZeroU64>,
    server_socket_cookie: Option<NonZeroU64>,
    request: Direction,
    reply: Direction,
    requests: VecDeque<Pending>,
    gateway: Option<gateway::State>,
}

impl Connection {
    fn start(ordinal: u64, sequence: u32) -> Self {
        Self {
            first_syn: ordinal,
            established: false,
            first_send_cookie: None,
            client_socket_cookie: None,
            server_socket_cookie: None,
            request: Direction {
                initial: Some(sequence.wrapping_add(1)),
                ..Direction::default()
            },
            reply: Direction::default(),
            requests: VecDeque::new(),
            gateway: None,
        }
    }

    fn from_first_send(fact: &NativeGroupTcp) -> Self {
        let mut connection = Self::start(fact.ordinal, fact.sequence.wrapping_sub(1));
        connection.established = true;
        connection.first_send_cookie = Some(fact.socket_cookie);
        connection
    }

    fn verified_gateway(&self) -> bool {
        self.gateway.is_some()
            && self.client_socket_cookie.is_some()
            && self.server_socket_cookie.is_some()
    }

    /// A data packet identifies its sender; actual I/O identifies the actor's endpoint.
    fn gateway_socket(&mut self, fact: &NativeGroupTcp, from_client: bool) -> bool {
        let (client, bind) = match &fact.observation {
            NativeGroupObservation::Packet { flags, payload } => {
                (from_client, *flags & SYN == 0 && !payload.is_empty())
            }
            NativeGroupObservation::Transfer { received, .. } => (from_client != *received, true),
            NativeGroupObservation::FirstSend => return false,
        };
        let Some(cookie) = NonZeroU64::new(fact.socket_cookie) else {
            return !bind && self.gateway.is_none();
        };
        let endpoint = if client {
            &mut self.client_socket_cookie
        } else {
            &mut self.server_socket_cookie
        };
        match endpoint {
            Some(original) => *original == cookie,
            None if bind => {
                *endpoint = Some(cookie);
                true
            }
            None => self.gateway.is_none(),
        }
    }

    /// False leaves the original fact untouched because it cannot enter this socket instance.
    fn offer(
        &mut self,
        fact: &mut NativeGroupTcp,
        boot: [u8; 16],
        from_client: bool,
        gateway: bool,
    ) -> Result<bool, ()> {
        if gateway && !self.gateway_socket(fact, from_client) {
            return Ok(false);
        }
        let request_initial = self.request.initial;
        let reply_initial = self.reply.initial;
        let direction = if from_client {
            &mut self.request
        } else {
            &mut self.reply
        };
        match &mut fact.observation {
            NativeGroupObservation::FirstSend => Ok(true),
            NativeGroupObservation::Packet { flags, payload } => {
                if *flags & (RST | 32) != 0 {
                    return Err(());
                }
                if *flags & SYN != 0 {
                    if !payload.is_empty() {
                        return Err(());
                    }
                    let initial = fact.sequence.wrapping_add(1);
                    if from_client {
                        return (*flags & ACK == 0 && direction.initial == Some(initial))
                            .then_some(true)
                            .ok_or(());
                    }
                    if *flags & ACK == 0
                        || request_initial != Some(fact.acknowledgment)
                        || direction.initial.is_some_and(|prior| prior != initial)
                    {
                        return Err(());
                    }
                    direction.initial = Some(initial);
                    return Ok(true);
                }
                if !self.established {
                    if !from_client
                        || reply_initial != Some(fact.acknowledgment)
                        || direction.initial != Some(fact.sequence)
                        || *flags & ACK == 0
                    {
                        return Ok(true);
                    }
                    self.established = true;
                }
                // Reattached OTLP connections have only a proven client sequence.
                if !from_client && self.first_send_cookie.is_some() {
                    if *flags & FIN != 0 {
                        direction.end = Some(0);
                    }
                    return Ok(true);
                }
                let offset = direction.position(fact.sequence)?;
                let end = offset.checked_add(payload.len() as u64).ok_or(())?;
                if direction.end.is_some_and(|old| {
                    !payload.is_empty() && end > old || *flags & FIN != 0 && end != old
                }) {
                    return Err(());
                }
                if *flags & FIN != 0 {
                    direction.end = Some(end);
                }
                if payload.is_empty() {
                    return Ok(true);
                }
                direction.packet(offset, payload).map(|()| true)
            }
            NativeGroupObservation::Transfer {
                received,
                length,
                tgid,
                birth_boottime_ns,
            } => {
                if !self.established {
                    return Ok(!gateway);
                }
                let offset = direction.position(fact.sequence)?;
                direction
                    .transfer(
                        *received,
                        offset,
                        *length,
                        ProcessKey {
                            boot_id: boot,
                            host_tgid: *tgid,
                            birth_boottime_ns: *birth_boottime_ns,
                        },
                    )
                    .map(|()| true)
            }
        }
    }

    fn retained(&self) -> usize {
        self.gateway.as_ref().map_or(0, gateway::State::retained)
            + self.request.retained()
            + self.reply.retained()
            + self
                .requests
                .iter()
                .map(|request| request.raw.len())
                .sum::<usize>()
    }

    fn completed(&self, bidirectional: bool) -> bool {
        self.request
            .end
            .is_some_and(|end| self.request.offset == end)
            && self
                .reply
                .end
                .is_some_and(|end| !bidirectional || self.reply.offset == end)
            && self.requests.is_empty()
    }

    fn consume_http(
        &mut self,
        a2a: bool,
        ports: Ports,
        source: Source,
        output: &mut Vec<Intake>,
    ) -> Result<(), ()> {
        while let Some((http::Start::Request { method, target }, request)) =
            http::read(&self.request.assembled, false, true)?
        {
            let len = request.raw.len();
            let Some(parent) = self.request.owner(len, false)? else {
                break;
            };
            let receiver = if a2a {
                let Some(actor) = self.request.owner(len, true)? else {
                    break;
                };
                Some(actor)
            } else {
                None
            };
            if request.upgrade.is_some() {
                return Err(());
            }
            if !a2a {
                let spans = if method == "POST"
                    && target == "/v1/traces"
                    && matches!(
                        request.content_type,
                        Some("application/x-protobuf" | "application/protobuf")
                    ) {
                    otlp::decode(&request.body()?, request.encoding, parent)
                } else {
                    None
                };
                let raw = self.request.consume(len);
                if let Some(spans) = spans.filter(|spans| !spans.is_empty()) {
                    output.push(Intake::Otlp(parent, raw, spans));
                }
                continue;
            }
            if self.requests.len() == MAX_PENDING
                || self
                    .requests
                    .iter()
                    .map(|request| request.raw.len())
                    .sum::<usize>()
                    + len
                    > http::MESSAGE_MAX
            {
                return Err(());
            }
            self.requests.push_back(Pending {
                offset: self.request.offset,
                raw: self.request.consume(len),
                parent,
                receiver,
            });
        }
        if !a2a {
            return Ok(());
        }
        while let Some(pending) = self.requests.front() {
            let Some((http::Start::Request { method, target }, request)) =
                http::read(&pending.raw, false, true)?
            else {
                return Err(());
            };
            let Some((http::Start::Response(status), reply)) =
                http::read(&self.reply.assembled, method == "HEAD", false)?
            else {
                break;
            };
            let len = reply.raw.len();
            let Some(child) = self.reply.owner(len, false)? else {
                break;
            };
            let Some(receiver) = self.reply.owner(len, true)? else {
                break;
            };
            if receiver != pending.parent || Some(child) != pending.receiver {
                return Err(());
            }
            if reply.upgrade.is_some() {
                return Err(());
            }
            if status < 200 {
                self.reply.consume(len);
                continue;
            }
            let authority = format!("127.0.0.1:{}", ports.1);
            let exchange = if request.host == Some(&authority)
                && method == "POST"
                && target.starts_with('/')
                && !target.starts_with("//")
                && request.content_type == Some("application/json")
                && request.encoding.is_none()
                && reply.encoding.is_none()
                && pending.parent != child
            {
                let path = target.split('?').next().ok_or(())?.to_owned();
                let url = format!("http://{authority}{target}");
                serde_json::from_slice(&request.body()?)
                    .ok()
                    .zip(serde_json::from_slice(&reply.body()?).ok())
                    .filter(|(request, _): &(super::RpcRequest, super::RpcReply)| {
                        matches!(request.method.as_str(), "message/send" | "SendMessage")
                    })
                    .map(|(request_body, reply)| Exchange {
                        url,
                        server_address: "127.0.0.1".into(),
                        server_port: ports.1,
                        client_port: ports.0,
                        path,
                        status,
                        traceparent: request.traceparent.map(str::to_owned),
                        request: request_body,
                        reply,
                    })
            } else {
                None
            };
            let pending = self.requests.pop_front().ok_or(())?;
            let response = self.reply.consume(len);
            if let Some(exchange) = exchange {
                let mut hash = Sha256::new();
                hash.update(source.0);
                hash.update(source.1.to_le_bytes());
                hash.update(self.first_syn.to_le_bytes());
                hash.update(pending.offset.to_le_bytes());
                output.push(Intake::A2a(
                    Box::new(RecordedExchange {
                        capture: hash.finalize().into(),
                        connection: ConnectionEvidence {
                            source: source.0,
                            generation: source.1,
                            first_syn_ordinal: self.first_syn,
                            request_offset: pending.offset,
                        },
                        parent: pending.parent,
                        child,
                        exchange,
                    }),
                    pending.raw,
                    response,
                ));
            }
        }
        Ok(())
    }
}

pub(crate) struct Collector {
    scope: GroupObservation,
    source: Option<Source>,
    ordinal: u64,
    gaps: u64,
    invalid_connections: u64,
    a2a_records: u64,
    otlp_batches: u64,
    gateway_messages: u64,
    connections: HashMap<Ports, Connection>,
    unframed: Vec<NativeGroupTcp>,
}

impl Collector {
    async fn record(&mut self, business: &BusinessLog, intake: Intake) -> Result<(), StoreError> {
        let count = match &intake {
            Intake::A2a(..) => Some(&mut self.a2a_records),
            Intake::Otlp(..) => Some(&mut self.otlp_batches),
            Intake::GatewayMessage { .. } => Some(&mut self.gateway_messages),
            _ => None,
        };
        business.record_group(intake).await?;
        if let Some(count) = count {
            *count = count.saturating_add(1);
        }
        Ok(())
    }

    async fn retire(
        &mut self,
        business: &BusinessLog,
        ports: Ports,
        reason: &'static str,
        diagnostics: &mut Vec<String>,
    ) -> Result<(), StoreError> {
        if let Some(connection) = self.connections.remove(&ports) {
            if let Some(source) = self.source {
                diagnostics.push(format!(
                    "AS_GROUP_CONNECTION_RETIRED source={:02x?} generation={} first_syn={} client_port={} server_port={} reason={reason}",
                    source.0, source.1, connection.first_syn, ports.0, ports.1
                ));
            }
            if self
                .scope
                .gateway_ws_port
                .is_some_and(|port| port.get() == ports.1)
                && let Some(source) = self.source
            {
                let mut output = Vec::new();
                connection.retire_gateway(ports, source, reason, &mut output);
                for intake in output {
                    self.record(business, intake).await?;
                }
            }
        }
        Ok(())
    }

    async fn retire_all(
        &mut self,
        business: &BusinessLog,
        reason: &'static str,
        diagnostics: &mut Vec<String>,
    ) -> Result<(), StoreError> {
        let ports: Vec<_> = self.connections.keys().copied().collect();
        for ports in ports {
            self.retire(business, ports, reason, diagnostics).await?;
        }
        Ok(())
    }

    pub(crate) async fn finish(
        &mut self,
        business: &BusinessLog,
        boot: [u8; 16],
    ) -> Result<(), StoreError> {
        let mut diagnostics = Vec::new();
        let result = async {
            self.retire_all(business, "source_stopped", &mut diagnostics)
                .await?;
            self.flush_unframed(business, boot).await
        }
        .await;
        for diagnostic in diagnostics {
            as_types::diagnostics::record_stderr(diagnostic).await;
        }
        result
    }

    pub(crate) fn new(scope: GroupObservation) -> Self {
        Self {
            scope,
            source: None,
            ordinal: 0,
            gaps: 0,
            invalid_connections: 0,
            a2a_records: 0,
            otlp_batches: 0,
            gateway_messages: 0,
            connections: HashMap::new(),
            unframed: Vec::new(),
        }
    }

    pub(crate) async fn flush_unframed(
        &mut self,
        business: &BusinessLog,
        boot: [u8; 16],
    ) -> Result<(), StoreError> {
        business
            .record_group_unframed(boot, std::mem::take(&mut self.unframed))
            .await
    }

    async fn retain_unframed(
        &mut self,
        fact: NativeGroupTcp,
        business: &BusinessLog,
        boot: [u8; 16],
    ) -> Result<(), StoreError> {
        match &fact.observation {
            NativeGroupObservation::Packet { payload, .. } if !payload.is_empty() => {}
            NativeGroupObservation::Packet { .. }
                if self.scope.gateway_ws_port.is_some_and(|port| {
                    port.get() == fact.source_port || port.get() == fact.destination_port
                }) => {}
            NativeGroupObservation::Transfer { .. } => {}
            _ => return Ok(()),
        }
        self.unframed.push(fact);
        if self.unframed.len() == UNFRAMED_BATCH {
            self.flush_unframed(business, boot).await?;
        }
        Ok(())
    }

    pub(crate) async fn report(&self) {
        as_types::diagnostics::record_stderr(format!(
            "AS_GROUP_TCP frames_through={} loss_boundaries={} invalid_connections={} a2a_records={} otlp_batches={} gateway_messages={}",
            self.ordinal,
            self.gaps,
            self.invalid_connections,
            self.a2a_records,
            self.otlp_batches,
            self.gateway_messages
        )).await;
    }

    pub(crate) async fn consume(
        &mut self,
        business: &BusinessLog,
        fact: NativeGroupTcp,
        boot: [u8; 16],
    ) -> Result<(), StoreError> {
        // At most this call's bounded connection retirement batch and source notices.
        // The original fact is processed before auxiliary stdio can wait.
        let mut diagnostics = Vec::new();
        let result = self
            .consume_inner(business, fact, boot, &mut diagnostics)
            .await;
        let result = if result.is_ok() && !diagnostics.is_empty() {
            self.flush_unframed(business, boot).await
        } else {
            result
        };
        for diagnostic in diagnostics {
            as_types::diagnostics::record_stderr(diagnostic).await;
        }
        result
    }

    async fn consume_inner(
        &mut self,
        business: &BusinessLog,
        mut fact: NativeGroupTcp,
        boot: [u8; 16],
        diagnostics: &mut Vec<String>,
    ) -> Result<(), StoreError> {
        let source = (fact.source, fact.generation);
        let previous = self.ordinal;
        self.ordinal = fact.ordinal;
        if self.source != Some(source) {
            self.retire_all(business, "source_changed", diagnostics)
                .await?;
            if let Some(prior) = self.source {
                diagnostics.push(format!(
                    "AS_GROUP_SOURCE_CHANGED previous_source={:02x?} previous_generation={} previous_ordinal={} source={:02x?} generation={} ordinal={}",
                    prior.0, prior.1, previous, source.0, source.1, fact.ordinal
                ));
            }
        } else if fact.ordinal <= previous {
            self.gaps = self.gaps.saturating_add(1);
            self.retire_all(business, "source_nonmonotonic", diagnostics)
                .await?;
            diagnostics.push(format!(
                "AS_GROUP_SOURCE_NONMONOTONIC source={:02x?} generation={} previous={} ordinal={}",
                source.0, source.1, previous, fact.ordinal
            ));
        } else if previous.checked_add(1) != Some(fact.ordinal) {
            self.gaps = self.gaps.saturating_add(1);
            let ports: Vec<_> = self
                .connections
                .iter()
                .filter_map(|(&ports, connection)| {
                    let verified = self
                        .scope
                        .gateway_ws_port
                        .is_some_and(|port| port.get() == ports.1)
                        && connection.verified_gateway();
                    (!verified).then_some(ports)
                })
                .collect();
            for ports in ports {
                self.retire(business, ports, "source_discontinuity", diagnostics)
                    .await?;
            }
            diagnostics.push(format!(
                "AS_GROUP_SOURCE_GAP source={:02x?} generation={} previous={} ordinal={}",
                source.0, source.1, previous, fact.ordinal
            ));
        }
        self.source = Some(source);
        let selected = |port| {
            [
                self.scope.a2a_port,
                self.scope.otlp_http_port,
                self.scope.gateway_ws_port,
            ]
            .into_iter()
            .flatten()
            .any(|selected| selected.get() == port)
        };
        let from_client = selected(fact.destination_port);
        if from_client == selected(fact.source_port) {
            return self.retain_unframed(fact, business, boot).await;
        }
        let ports = if from_client {
            (fact.source_port, fact.destination_port)
        } else {
            (fact.destination_port, fact.source_port)
        };
        let a2a = self
            .scope
            .a2a_port
            .is_some_and(|port| port.get() == ports.1);
        let gateway = self
            .scope
            .gateway_ws_port
            .is_some_and(|port| port.get() == ports.1);
        let bidirectional = a2a || gateway;
        let verified_gateway = gateway
            && self
                .connections
                .get(&ports)
                .is_some_and(Connection::verified_gateway);
        if from_client
            && (!verified_gateway || fact.socket_cookie != 0)
            && matches!(&fact.observation, NativeGroupObservation::Packet { flags, payload }
                if flags & (SYN | ACK | FIN | RST) == SYN && payload.is_empty())
        {
            let initial = fact.sequence.wrapping_add(1);
            if self.connections.get(&ports).is_none_or(|conn| {
                conn.established
                    || conn.request.initial != Some(initial)
                    || (fact.socket_cookie != 0
                        && conn
                            .client_socket_cookie
                            .is_some_and(|original| original.get() != fact.socket_cookie))
            }) {
                self.retire(
                    business,
                    ports,
                    if verified_gateway {
                        "new_handshake_prior_terminal_unobserved"
                    } else {
                        "connection_replaced"
                    },
                    diagnostics,
                )
                .await?;
                if self.connections.len() == MAX_CONNECTIONS {
                    self.retire_all(business, "connection_limit", diagnostics)
                        .await?;
                }
                self.connections
                    .insert(ports, Connection::start(fact.ordinal, fact.sequence));
            }
        }
        if matches!(&fact.observation, NativeGroupObservation::FirstSend) {
            if !bidirectional
                && from_client
                && fact.socket_cookie != 0
                && self.connections.len() < MAX_CONNECTIONS
            {
                self.connections
                    .entry(ports)
                    .or_insert_with(|| Connection::from_first_send(&fact));
            }
            return Ok(());
        }
        if self
            .connections
            .values()
            .map(Connection::retained)
            .sum::<usize>()
            > 8 * http::MESSAGE_MAX
        {
            self.retire(business, ports, "retained_byte_limit", diagnostics)
                .await?;
            return self.retain_unframed(fact, business, boot).await;
        }
        let Some(connection) = self.connections.get_mut(&ports) else {
            return self.retain_unframed(fact, business, boot).await;
        };
        if !bidirectional
            && !from_client
            && !matches!(&fact.observation,
                NativeGroupObservation::Packet { flags, .. } if flags & (SYN | FIN | RST) != 0
            )
        {
            return Ok(());
        }
        if from_client
            && connection
                .first_send_cookie
                .is_some_and(|cookie| cookie != fact.socket_cookie)
        {
            return self.retain_unframed(fact, business, boot).await;
        }
        let mut output = Vec::new();
        let offered = connection.offer(&mut fact, boot, from_client, gateway);
        let result = offered.and_then(|accepted| {
            if !accepted {
                return Ok(());
            }
            if gateway {
                connection.consume_gateway(ports, source, &mut output)
            } else {
                connection.consume_http(a2a, ports, source, &mut output)
            }
        });
        if result.is_err() {
            self.invalid_connections = self.invalid_connections.saturating_add(1);
        }
        let completed = connection.completed(bidirectional);
        for intake in output {
            self.record(business, intake).await?;
        }
        if result.is_err() || completed {
            self.retire(
                business,
                ports,
                if result.is_err() {
                    "connection_invalid"
                } else {
                    "connection_closed"
                },
                diagnostics,
            )
            .await?;
        }
        if result.is_err() || matches!(offered, Ok(false)) {
            self.retain_unframed(fact, business, boot).await?;
        }
        Ok(())
    }
}

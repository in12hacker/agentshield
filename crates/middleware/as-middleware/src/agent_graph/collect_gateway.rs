//! HTTP Upgrade and passive WS states of the original Group connection, with one byte owner.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

pub(super) struct State {
    capture: [u8; 32],
    client: ProcessKey,
    server: ProcessKey,
    request: websocket::Assembler,
    reply: websocket::Assembler,
    unindexed: Option<&'static str>,
}

impl State {
    pub(super) fn retained(&self) -> usize {
        self.request.retained() + self.reply.retained()
    }
}
fn token(value: Option<&str>, expected: &str) -> bool {
    value.is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case(expected))
    })
}
fn upgrade(message: &http::Message<'_>) -> bool {
    message
        .upgrade
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && token(message.connection, "upgrade")
        && message.encoding.is_none()
        && message.body().is_ok_and(|body| body.is_empty())
}
impl Direction {
    fn take_frames(&mut self) -> Vec<websocket::WireFrame> {
        let mut frames = Vec::new();
        if !self.assembled.is_empty() {
            frames.push(websocket::WireFrame {
                offset: self.offset,
                wire: std::mem::take(&mut self.assembled),
            });
        }
        frames.extend(
            std::mem::take(&mut self.packets)
                .into_iter()
                .map(|(offset, wire)| websocket::WireFrame { offset, wire }),
        );
        frames
    }
}
impl Connection {
    pub(super) fn consume_gateway(
        &mut self,
        ports: Ports,
        source: Source,
        output: &mut Vec<Intake>,
    ) -> Result<(), ()> {
        if self.gateway.is_none() {
            if self.requests.is_empty() {
                let Some((http::Start::Request { method, .. }, request)) =
                    http::read(&self.request.assembled, false, true)?
                else {
                    return Ok(());
                };
                if method != "GET"
                    || !upgrade(&request)
                    || request.websocket_version != Some("13")
                    || request.websocket_key.is_none_or(|key| {
                        !STANDARD.decode(key).is_ok_and(|bytes| bytes.len() == 16)
                    })
                {
                    return Err(());
                }
                let len = request.raw.len();
                let (Some(parent), Some(receiver)) = (
                    self.request.owner(len, false)?,
                    self.request.owner(len, true)?,
                ) else {
                    return Ok(());
                };
                self.requests.push_back(Pending {
                    offset: self.request.offset,
                    raw: self.request.consume(len),
                    parent,
                    receiver: Some(receiver),
                });
            }
            let pending = self.requests.front().ok_or(())?;
            let Some((_, request)) = http::read(&pending.raw, false, true)? else {
                return Err(());
            };
            let Some((http::Start::Response(status), reply)) =
                http::read(&self.reply.assembled, false, false)?
            else {
                return Ok(());
            };
            let len = reply.raw.len();
            let (Some(server), Some(client)) =
                (self.reply.owner(len, false)?, self.reply.owner(len, true)?)
            else {
                return Ok(());
            };
            if Some(server) != pending.receiver || client != pending.parent {
                return Err(());
            }
            if status != 101
                || !upgrade(&reply)
                || reply.websocket_accept
                    != Some(derive_accept_key(request.websocket_key.ok_or(())?.as_bytes()).as_str())
            {
                return Err(());
            }
            let client_socket_cookie = self.client_socket_cookie.ok_or(())?.get();
            let server_socket_cookie = self.server_socket_cookie.ok_or(())?.get();
            let unindexed = reply
                .websocket_extensions
                .filter(|value| !value.is_empty())
                .map(|_| "websocket_extension_unindexed");
            let mut hash = Sha256::new();
            hash.update(source.0);
            hash.update(source.1.to_le_bytes());
            hash.update(self.first_syn.to_le_bytes());
            hash.update(ports.0.to_le_bytes());
            hash.update(ports.1.to_le_bytes());
            let capture = hash.finalize().into();
            let pending = self.requests.pop_front().ok_or(())?;
            output.push(Intake::GatewayHandshake(
                Box::new(crate::gateway::GatewayConnection {
                    capture,
                    source: source.0,
                    generation: source.1,
                    first_syn_ordinal: self.first_syn,
                    client_port: ports.0,
                    server_port: ports.1,
                    client,
                    server,
                    client_socket_cookie,
                    server_socket_cookie,
                }),
                pending.raw,
                self.reply.consume(len),
            ));
            self.gateway = Some(State {
                capture,
                client,
                server,
                request: websocket::Assembler::new(true, http::MESSAGE_MAX),
                reply: websocket::Assembler::new(false, http::MESSAGE_MAX),
                unindexed,
            });
        }
        let state = self.gateway.as_mut().ok_or(())?;
        for (from_client, direction, assembler, expected_sender, expected_receiver) in [
            (
                true,
                &mut self.request,
                &mut state.request,
                state.client,
                state.server,
            ),
            (
                false,
                &mut self.reply,
                &mut state.reply,
                state.server,
                state.client,
            ),
        ] {
            while let Some(frame) =
                websocket::decode_frame(&direction.assembled, from_client, http::MESSAGE_MAX)
                    .map_err(|_| ())?
            {
                let len = frame.wire_len;
                let (Some(sender), Some(receiver)) =
                    (direction.owner(len, false)?, direction.owner(len, true)?)
                else {
                    break;
                };
                let offset = direction.offset;
                let raw = direction.consume(len);
                if sender != expected_sender || receiver != expected_receiver {
                    output.push(Intake::GatewayGap {
                        connection: state.capture,
                        from_client,
                        frames: vec![websocket::WireFrame { offset, wire: raw }],
                        reason: "websocket_owner_changed",
                    });
                    state.unindexed = Some("websocket_owner_changed");
                    continue;
                }
                if let Some(reason) = state.unindexed {
                    output.push(Intake::GatewayGap {
                        connection: state.capture,
                        from_client,
                        frames: vec![websocket::WireFrame { offset, wire: raw }],
                        reason,
                    });
                    continue;
                }
                match assembler.push(offset, raw, frame) {
                    Ok(Some(message)) => output.push(Intake::GatewayMessage {
                        connection: state.capture,
                        from_client,
                        sender,
                        receiver,
                        message,
                    }),
                    Ok(None) => (),
                    Err(rejected) => {
                        eprintln!("AS_GROUP_WS_UNINDEXED reason={}", rejected.error);
                        output.push(Intake::GatewayGap {
                            connection: state.capture,
                            from_client,
                            frames: rejected.frames,
                            reason: "websocket_message_invalid",
                        });
                        state.unindexed = Some("websocket_message_invalid");
                    }
                }
            }
        }
        if let Some(reason) = state.unindexed {
            for (from_client, assembler) in [(true, &mut state.request), (false, &mut state.reply)]
            {
                let frames = assembler.take_pending();
                if !frames.is_empty() {
                    output.push(Intake::GatewayGap {
                        connection: state.capture,
                        from_client,
                        frames,
                        reason,
                    });
                }
            }
        }
        Ok(())
    }
    pub(super) fn retire_gateway(
        mut self,
        ports: Ports,
        source: Source,
        reason: &'static str,
        output: &mut Vec<Intake>,
    ) {
        if let Some(mut state) = self.gateway.take() {
            for (from_client, direction, assembler) in [
                (true, &mut self.request, &mut state.request),
                (false, &mut self.reply, &mut state.reply),
            ] {
                let mut frames = assembler.take_pending();
                frames.extend(direction.take_frames());
                if !frames.is_empty() {
                    output.push(Intake::GatewayGap {
                        connection: state.capture,
                        from_client,
                        frames,
                        reason,
                    });
                }
            }
        } else {
            let mut requests = Vec::new();
            for pending in self.requests {
                requests.push(websocket::WireFrame {
                    offset: pending.offset,
                    wire: pending.raw,
                });
            }
            requests.extend(self.request.take_frames());
            for (from_client, frames) in [(true, requests), (false, self.reply.take_frames())] {
                if !frames.is_empty() {
                    output.push(Intake::GatewayUnverified {
                        source: source.0,
                        generation: source.1,
                        first_syn_ordinal: self.first_syn,
                        client_port: ports.0,
                        server_port: ports.1,
                        from_client,
                        frames,
                        reason,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: Source = ([11; 16], 2);
    const PORTS: Ports = (43001, 18789);
    const BOOT: [u8; 16] = [5; 16];
    const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:18789\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
    const REPLY: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";

    fn actor(client: bool) -> ProcessKey {
        ProcessKey {
            boot_id: BOOT,
            host_tgid: if client { 100 } else { 200 },
            birth_boottime_ns: if client { 1000 } else { 2000 },
        }
    }

    fn fact(
        from_client: bool,
        sequence: u32,
        acknowledgment: u32,
        observation: NativeGroupObservation,
    ) -> NativeGroupTcp {
        let client = match &observation {
            NativeGroupObservation::Transfer { received, .. } => from_client != *received,
            _ => from_client,
        };
        NativeGroupTcp {
            source: SOURCE.0,
            generation: SOURCE.1,
            ordinal: 1,
            source_port: if from_client { PORTS.0 } else { PORTS.1 },
            destination_port: if from_client { PORTS.1 } else { PORTS.0 },
            sequence,
            acknowledgment,
            socket_cookie: if client { 41 } else { 42 },
            observation,
        }
    }

    fn connection(first_syn: u64) -> Connection {
        let mut connection = Connection::start(first_syn, 10);
        connection
            .offer(
                &mut fact(
                    false,
                    20,
                    11,
                    NativeGroupObservation::Packet {
                        flags: SYN | ACK,
                        payload: vec![],
                    },
                ),
                BOOT,
                false,
                true,
            )
            .expect("SYN ACK");
        connection
            .offer(
                &mut fact(
                    true,
                    11,
                    21,
                    NativeGroupObservation::Packet {
                        flags: ACK,
                        payload: vec![],
                    },
                ),
                BOOT,
                true,
                true,
            )
            .expect("final ACK");
        assert!(connection.established);
        assert!(connection.client_socket_cookie.is_none());
        assert!(connection.server_socket_cookie.is_none());
        connection
    }

    fn packet(connection: &mut Connection, from_client: bool, offset: u64, bytes: &[u8]) {
        let direction = if from_client {
            &connection.request
        } else {
            &connection.reply
        };
        let sequence = direction
            .initial
            .expect("handshake sequence")
            .wrapping_add(offset as u32);
        connection
            .offer(
                &mut fact(
                    from_client,
                    sequence,
                    0,
                    NativeGroupObservation::Packet {
                        flags: ACK,
                        payload: bytes.to_vec(),
                    },
                ),
                BOOT,
                from_client,
                true,
            )
            .expect("packet");
    }

    fn transfer(
        connection: &mut Connection,
        from_client: bool,
        offset: u64,
        length: usize,
        received: bool,
    ) {
        let direction = if from_client {
            &connection.request
        } else {
            &connection.reply
        };
        let sequence = direction
            .initial
            .expect("handshake sequence")
            .wrapping_add(offset as u32);
        let owner = actor(from_client != received);
        connection
            .offer(
                &mut fact(
                    from_client,
                    sequence,
                    0,
                    NativeGroupObservation::Transfer {
                        received,
                        length: length as u32,
                        tgid: owner.host_tgid,
                        birth_boottime_ns: owner.birth_boottime_ns,
                    },
                ),
                BOOT,
                from_client,
                true,
            )
            .expect("actual transfer");
    }

    fn verified(connection: &mut Connection, from_client: bool, offset: u64, bytes: &[u8]) {
        packet(connection, from_client, offset, bytes);
        transfer(connection, from_client, offset, bytes.len(), false);
        transfer(connection, from_client, offset, bytes.len(), true);
    }

    fn upgraded(first_syn: u64) -> (Connection, [u8; 32]) {
        let mut connection = connection(first_syn);
        verified(&mut connection, true, 0, REQUEST);
        verified(&mut connection, false, 0, REPLY);
        let mut output = Vec::new();
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("verified upgrade");
        let [Intake::GatewayHandshake(evidence, request, reply)] = output.as_slice() else {
            panic!("one handshake");
        };
        assert_eq!(request, REQUEST);
        assert_eq!(reply, REPLY);
        assert_eq!(evidence.client_socket_cookie, 41);
        assert_eq!(evidence.server_socket_cookie, 42);
        (connection, evidence.capture)
    }

    async fn log(directory: &std::path::Path) -> BusinessLog {
        use as_types::context::{ConfigPaths, ResolvedPaths};
        let paths = ConfigPaths::from_configuration(ResolvedPaths {
            backend_executable: directory.join("as-service"),
            self_protection_root: directory.to_path_buf(),
            configuration_file: directory.join("as.yaml"),
            rules_file: directory.join("rules.yaml"),
            driver_assets_directory: directory.join("driver"),
            middleware_endpoint: directory.join("middleware.sock"),
            data_directory: directory.join("data"),
            credential_store_file: directory.join("credentials"),
            ca_public_file: directory.join("ca.pem"),
            security_log_file: directory.join("security.sqlite"),
            middleware_database_file: directory.join("business.sqlite"),
        });
        BusinessLog::initialize(
            &paths,
            as_types::log_storage::LogStorageBudget::new(Default::default()),
        )
        .await
        .expect("actual temporary business owner")
    }

    async fn observe(collector: &mut Collector, business: &BusinessLog, mut input: NativeGroupTcp) {
        input.ordinal = collector.ordinal + 1;
        collector
            .consume(business, input, BOOT)
            .await
            .expect("original observation");
    }

    async fn captured(business: &BusinessLog) -> Collector {
        let mut collector = Collector::new(GroupObservation {
            a2a_port: None,
            otlp_http_port: None,
            gateway_ws_port: std::num::NonZeroU16::new(PORTS.1),
        });
        for (client, sequence, acknowledgment, flags) in [
            (true, 10, 0, SYN),
            (false, 20, 11, SYN | ACK),
            (true, 11, 21, ACK),
        ] {
            let mut input = fact(
                client,
                sequence,
                acknowledgment,
                NativeGroupObservation::Packet {
                    flags,
                    payload: vec![],
                },
            );
            if !client {
                input.socket_cookie = 9000;
            }
            observe(&mut collector, business, input).await;
        }
        assert!(collector.connections[&PORTS].client_socket_cookie.is_none());
        assert!(collector.connections[&PORTS].server_socket_cookie.is_none());
        for (client, sequence, bytes) in [(true, 11, REQUEST), (false, 21, REPLY)] {
            observe(
                &mut collector,
                business,
                fact(
                    client,
                    sequence,
                    0,
                    NativeGroupObservation::Packet {
                        flags: ACK,
                        payload: bytes.to_vec(),
                    },
                ),
            )
            .await;
            for received in [false, true] {
                let owner = actor(client != received);
                observe(
                    &mut collector,
                    business,
                    fact(
                        client,
                        sequence,
                        0,
                        NativeGroupObservation::Transfer {
                            received,
                            length: bytes.len() as u32,
                            tgid: owner.host_tgid,
                            birth_boottime_ns: owner.birth_boottime_ns,
                        },
                    ),
                )
                .await;
            }
        }
        assert!(collector.connections[&PORTS].verified_gateway());
        collector
    }

    #[tokio::test]
    async fn sampling_gap_keeps_only_proven_sockets_and_still_requires_original_bytes_and_io() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let business = log(directory.path()).await;
        let mut collector = captured(&business).await;
        let wire = [0x81, 3, b'a', b'b', b'c'];
        let sequence = 21 + REPLY.len() as u32;
        observe(
            &mut collector,
            &business,
            fact(
                false,
                sequence,
                0,
                NativeGroupObservation::Packet {
                    flags: ACK,
                    payload: wire[..3].to_vec(),
                },
            ),
        )
        .await;
        let mut candidate = fact(
            true,
            70,
            0,
            NativeGroupObservation::Packet {
                flags: SYN,
                payload: vec![],
            },
        );
        candidate.source_port += 1;
        observe(&mut collector, &business, candidate).await;
        let mut gap = fact(
            false,
            sequence,
            0,
            NativeGroupObservation::Packet {
                flags: ACK,
                payload: vec![],
            },
        );
        gap.ordinal = collector.ordinal + 3;
        collector
            .consume(&business, gap, BOOT)
            .await
            .expect("sampling discontinuity");
        assert_eq!(collector.connections.len(), 1);
        assert_eq!(collector.connections[&PORTS].reply.assembled, wire[..3]);
        observe(
            &mut collector,
            &business,
            fact(
                false,
                sequence + 3,
                0,
                NativeGroupObservation::Packet {
                    flags: ACK,
                    payload: wire[3..].to_vec(),
                },
            ),
        )
        .await;
        assert_eq!(
            collector.connections[&PORTS].reply.offset,
            REPLY.len() as u64
        );
        for received in [false, true] {
            let owner = actor(received);
            observe(
                &mut collector,
                &business,
                fact(
                    false,
                    sequence,
                    0,
                    NativeGroupObservation::Transfer {
                        received,
                        length: wire.len() as u32,
                        tgid: owner.host_tgid,
                        birth_boottime_ns: owner.birth_boottime_ns,
                    },
                ),
            )
            .await;
        }
        assert_eq!(
            collector.connections[&PORTS].reply.offset,
            (REPLY.len() + wire.len()) as u64
        );
        let read = rusqlite::Connection::open_with_flags(
            directory.path().join("business.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("committed evidence");
        assert_eq!(
            read.query_row("SELECT count(*) FROM group_gateway_messages", [], |row| row
                .get::<_, i64>(0))
                .expect("one complete message"),
            1
        );
        // A later fragment with its own actual byte gap cannot be treated as a new JSON boundary.
        observe(
            &mut collector,
            &business,
            fact(
                false,
                sequence + wire.len() as u32 + 3,
                0,
                NativeGroupObservation::Packet {
                    flags: ACK,
                    payload: wire[3..].to_vec(),
                },
            ),
        )
        .await;
        assert_eq!(
            collector.connections[&PORTS].reply.offset,
            (REPLY.len() + wire.len()) as u64
        );
        assert_eq!(
            read.query_row("SELECT count(*) FROM group_gateway_messages", [], |row| row
                .get::<_, i64>(0))
                .expect("missing bytes remain missing"),
            1
        );
        let mut reinitialized = fact(
            false,
            sequence,
            0,
            NativeGroupObservation::Packet {
                flags: ACK,
                payload: vec![],
            },
        );
        reinitialized.generation += 1;
        reinitialized.ordinal = 1;
        collector
            .consume(&business, reinitialized, BOOT)
            .await
            .expect("a new source generation cannot continue the old protocol");
        assert!(collector.connections.is_empty());
        let mut next_syn = fact(
            true,
            70,
            0,
            NativeGroupObservation::Packet {
                flags: SYN,
                payload: vec![],
            },
        );
        next_syn.generation += 1;
        next_syn.ordinal = 2;
        collector
            .consume(&business, next_syn, BOOT)
            .await
            .expect("fresh boundary in the new generation");
        let mut duplicate = fact(
            false,
            21,
            0,
            NativeGroupObservation::Packet {
                flags: ACK,
                payload: vec![],
            },
        );
        duplicate.generation += 1;
        duplicate.ordinal = 2;
        collector
            .consume(&business, duplicate, BOOT)
            .await
            .expect("non-increasing source ordinal retires its contexts");
        assert!(collector.connections.is_empty());
        collector
            .finish(&business, BOOT)
            .await
            .expect("retire incomplete original");
        business.close().await.expect("physical close");
    }

    #[tokio::test]
    async fn unbound_controls_and_foreign_io_stay_original_while_a_real_syn_starts_a_new_protocol()
    {
        let directory = tempfile::tempdir().expect("temporary directory");
        let business = log(directory.path()).await;
        let mut collector = captured(&business).await;
        let original_syn = collector.connections[&PORTS].first_syn;
        let sequence = 21 + REPLY.len() as u32;
        let mut unknown_fin = fact(
            false,
            sequence,
            0,
            NativeGroupObservation::Packet {
                flags: FIN | ACK,
                payload: vec![],
            },
        );
        unknown_fin.socket_cookie = 0;
        observe(&mut collector, &business, unknown_fin).await;
        assert!(collector.connections[&PORTS].reply.end.is_none());
        let mut foreign = fact(
            false,
            sequence,
            0,
            NativeGroupObservation::Packet {
                flags: ACK,
                payload: vec![0x81, 1, b'x'],
            },
        );
        foreign.socket_cookie = 99;
        observe(&mut collector, &business, foreign).await;
        assert!(collector.connections[&PORTS].reply.assembled.is_empty());
        assert_eq!(
            collector.connections[&PORTS]
                .server_socket_cookie
                .map(NonZeroU64::get),
            Some(42)
        );
        let mut foreign_receive = fact(
            false,
            sequence,
            0,
            NativeGroupObservation::Transfer {
                received: true,
                length: 1,
                tgid: actor(true).host_tgid,
                birth_boottime_ns: actor(true).birth_boottime_ns,
            },
        );
        foreign_receive.socket_cookie = 42;
        observe(&mut collector, &business, foreign_receive).await;
        assert!(collector.connections[&PORTS].reply.received.is_empty());
        let mut unknown_syn = fact(
            true,
            70,
            0,
            NativeGroupObservation::Packet {
                flags: SYN,
                payload: vec![],
            },
        );
        unknown_syn.socket_cookie = 0;
        observe(&mut collector, &business, unknown_syn).await;
        assert_eq!(collector.connections[&PORTS].first_syn, original_syn);
        assert_eq!(collector.unframed.len(), 4);
        // Even the same stable socket cookie does not turn a new initial SYN into the old protocol.
        observe(
            &mut collector,
            &business,
            fact(
                true,
                70,
                0,
                NativeGroupObservation::Packet {
                    flags: SYN,
                    payload: vec![],
                },
            ),
        )
        .await;
        let replacement = &collector.connections[&PORTS];
        assert_ne!(replacement.first_syn, original_syn);
        assert_eq!(replacement.request.initial, Some(71));
        assert!(!replacement.verified_gateway());
        assert!(replacement.client_socket_cookie.is_none());
        assert!(replacement.server_socket_cookie.is_none());
        let prior_unframed = collector.unframed.len();
        observe(
            &mut collector,
            &business,
            fact(
                true,
                71,
                0,
                NativeGroupObservation::Transfer {
                    received: false,
                    length: 1,
                    tgid: actor(true).host_tgid,
                    birth_boottime_ns: actor(true).birth_boottime_ns,
                },
            ),
        )
        .await;
        assert_eq!(collector.unframed.len(), prior_unframed + 1);
        assert!(collector.unframed.last().is_some_and(|original| {
            original.socket_cookie == 41
                && matches!(
                    original.observation,
                    NativeGroupObservation::Transfer {
                        received: false,
                        length: 1,
                        ..
                    }
                )
        }));
        let candidate = &collector.connections[&PORTS];
        assert!(!candidate.established);
        assert_eq!(
            candidate.client_socket_cookie.map(NonZeroU64::get),
            Some(41)
        );
        let candidate_syn = candidate.first_syn;
        let mut no_identity = fact(
            true,
            70,
            0,
            NativeGroupObservation::Packet {
                flags: SYN,
                payload: vec![],
            },
        );
        no_identity.socket_cookie = 0;
        observe(&mut collector, &business, no_identity).await;
        assert_eq!(collector.connections[&PORTS].first_syn, candidate_syn);
        let mut fresh_socket = fact(
            true,
            70,
            0,
            NativeGroupObservation::Packet {
                flags: SYN,
                payload: vec![],
            },
        );
        fresh_socket.socket_cookie = 99;
        observe(&mut collector, &business, fresh_socket).await;
        let fresh = &collector.connections[&PORTS];
        assert_ne!(fresh.first_syn, candidate_syn);
        assert_eq!(fresh.request.initial, Some(71));
        assert!(
            fresh.client_socket_cookie.is_none(),
            "the new SYN is a boundary, not data binding"
        );
        collector
            .finish(&business, BOOT)
            .await
            .expect("source retired");
        business.close().await.expect("physical close");
    }

    #[test]
    fn upgrade_and_frame_wait_for_complete_actual_send_and_receive_ranges() {
        let mut connection = connection(1);
        let mut output = Vec::new();
        packet(&mut connection, true, 0, REQUEST);
        transfer(&mut connection, true, 0, REQUEST.len(), false);
        transfer(&mut connection, true, 0, REQUEST.len() - 1, true);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("incomplete owner");
        assert_eq!(connection.request.offset, 0);
        assert!(connection.requests.is_empty());
        assert!(output.is_empty());
        transfer(&mut connection, true, (REQUEST.len() - 1) as u64, 1, true);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("request only");
        assert_eq!(connection.request.offset, REQUEST.len() as u64);
        assert_eq!(connection.requests.len(), 1);
        packet(&mut connection, false, 0, REPLY);
        transfer(&mut connection, false, 0, REPLY.len(), false);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("reply missing receive");
        assert_eq!(connection.reply.offset, 0);
        assert!(output.is_empty());
        transfer(&mut connection, false, 0, REPLY.len(), true);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("upgrade");
        assert!(matches!(output.as_slice(), [Intake::GatewayHandshake(..)]));
        output.clear();

        let wire = [0x81, 126, 0, 3, b'a', b'b', b'c'];
        let offset = REPLY.len() as u64;
        packet(&mut connection, false, offset, &wire[..3]);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("TCP partial header");
        assert_eq!(connection.reply.offset, offset);
        packet(&mut connection, false, offset + 3, &wire[3..]);
        transfer(&mut connection, false, offset, wire.len(), true);
        transfer(&mut connection, false, offset, wire.len() - 1, false);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("frame missing send byte");
        assert_eq!(connection.reply.offset, offset);
        assert!(output.is_empty());
        transfer(
            &mut connection,
            false,
            offset + wire.len() as u64 - 1,
            1,
            false,
        );
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("complete original frame");
        let [Intake::GatewayMessage { message, .. }] = output.as_slice() else {
            panic!("one message");
        };
        assert_eq!(message.text(), Some("abc"));
        assert_eq!(message.frames[0].offset, offset);
        assert_eq!(message.frames[0].wire, wire);
        assert_eq!(connection.reply.offset, offset + wire.len() as u64);
    }

    #[test]
    fn fragmented_message_controls_and_reused_ports_remain_in_their_original_connection() {
        let (mut first, first_capture) = upgraded(1);
        let (mut second, second_capture) = upgraded(30);
        assert_ne!(first_capture, second_capture);
        let first_fragment = [0x01, 1, b'a'];
        let ping = [0x89, 0];
        let last_fragment = [0x80, 1, b'b'];
        let separate = [0x81, 1, b'z'];
        let offset = REPLY.len() as u64;
        let mut output = Vec::new();
        verified(&mut first, false, offset, &first_fragment);
        first
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("first fragment");
        assert!(output.is_empty());
        verified(&mut second, false, offset, &separate);
        second
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("separate connection");
        let [
            Intake::GatewayMessage {
                connection,
                message,
                ..
            },
        ] = output.as_slice()
        else {
            panic!("second message");
        };
        assert_eq!(*connection, second_capture);
        assert_eq!(message.text(), Some("z"));
        output.clear();
        let control_offset = offset + first_fragment.len() as u64;
        verified(&mut first, false, control_offset, &ping);
        verified(
            &mut first,
            false,
            control_offset + ping.len() as u64,
            &last_fragment,
        );
        first
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("fragment plus interleaved control");
        let [
            Intake::GatewayMessage {
                message: control, ..
            },
            Intake::GatewayMessage {
                connection,
                message,
                ..
            },
        ] = output.as_slice()
        else {
            panic!("control and first message");
        };
        assert_eq!(*connection, first_capture);
        assert!(control.text().is_none());
        assert_eq!(control.frames[0].wire, ping);
        assert_eq!(message.text(), Some("ab"));
        assert_eq!(message.frames.len(), 2);
        assert_eq!(message.frames[0].wire, first_fragment);
        assert_eq!(message.frames[1].wire, last_fragment);
        assert_eq!(message.frames[1].offset, control_offset + ping.len() as u64);
    }

    #[test]
    fn sequence_wrap_keeps_long_stream_positions_distinct_from_tcp_sequence_numbers() {
        let offset = (1_u64 << 32) - 2;
        let mut direction = Direction {
            initial: Some(0),
            offset,
            ..Direction::default()
        };
        assert_eq!(direction.position(u32::MAX - 1), Ok(offset));
        let mut bytes = vec![0x81, 3, b'a', b'b', b'c'];
        direction
            .packet(offset, &mut bytes)
            .expect("frame across serial wrap");
        direction
            .transfer(false, offset, 5, actor(false))
            .expect("actual send");
        direction
            .transfer(true, offset, 5, actor(true))
            .expect("actual receive");
        assert_eq!(direction.owner(5, false), Ok(Some(actor(false))));
        assert_eq!(direction.owner(5, true), Ok(Some(actor(true))));
        assert_eq!(direction.consume(5), [0x81, 3, b'a', b'b', b'c']);
        assert_eq!(direction.offset, (1_u64 << 32) + 3);
        assert_eq!(direction.position(3), Ok((1_u64 << 32) + 3));
        assert_eq!(direction.position(u32::MAX), Ok((1_u64 << 32) - 1));
    }

    #[test]
    fn retirement_returns_consumed_fragments_partial_wire_and_disjoint_packets_once() {
        let (mut connection, capture) = upgraded(1);
        let offset = REPLY.len() as u64;
        let first = [0x01, 1, b'a'];
        let incomplete = [0x80, 4, b'b'];
        let disjoint = [b'e', b'f'];
        let mut output = Vec::new();
        verified(&mut connection, false, offset, &first);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("first fragment");
        assert!(output.is_empty());
        packet(
            &mut connection,
            false,
            offset + first.len() as u64,
            &incomplete,
        );
        let disjoint_offset = offset + (first.len() + incomplete.len() + 5) as u64;
        packet(&mut connection, false, disjoint_offset, &disjoint);
        connection.retire_gateway(PORTS, SOURCE, "source_discontinuity", &mut output);
        let [
            Intake::GatewayGap {
                connection,
                from_client,
                frames,
                reason,
            },
        ] = output.as_slice()
        else {
            panic!("one source gap");
        };
        assert_eq!(*connection, capture);
        assert!(!from_client);
        assert_eq!(*reason, "source_discontinuity");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].wire, first);
        assert_eq!(frames[1].wire, incomplete);
        assert_eq!(frames[2].wire, disjoint);
        assert_eq!(frames[2].offset, disjoint_offset);

        let mut connection = self::connection(30);
        packet(&mut connection, true, 0, &REQUEST[..8]);
        output.clear();
        connection.retire_gateway(PORTS, SOURCE, "source_stopped", &mut output);
        let [
            Intake::GatewayUnverified {
                source,
                first_syn_ordinal,
                frames,
                ..
            },
        ] = output.as_slice()
        else {
            panic!("unverified original");
        };
        assert_eq!(*source, SOURCE.0);
        assert_eq!(*first_syn_ordinal, 30);
        assert_eq!(frames[0].wire, REQUEST[..8]);
    }

    #[test]
    fn changed_byte_owner_unindexes_the_connection_and_returns_both_pending_messages() {
        let (mut connection, _) = upgraded(1);
        let client_fragment = [0x01, 0x81, 0, 0, 0, 0, b'c'];
        let server_fragment = [0x01, 1, b's'];
        let changed_owner_wire = [0x81, 1, b'x'];
        let mut output = Vec::new();
        verified(
            &mut connection,
            true,
            REQUEST.len() as u64,
            &client_fragment,
        );
        verified(&mut connection, false, REPLY.len() as u64, &server_fragment);
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("both incomplete messages");
        assert!(output.is_empty());

        let offset = (REPLY.len() + server_fragment.len()) as u64;
        packet(&mut connection, false, offset, &changed_owner_wire);
        transfer(
            &mut connection,
            false,
            offset,
            changed_owner_wire.len(),
            true,
        );
        let sequence = connection
            .reply
            .initial
            .expect("server sequence")
            .wrapping_add(offset as u32);
        connection
            .offer(
                &mut fact(
                    false,
                    sequence,
                    0,
                    NativeGroupObservation::Transfer {
                        received: false,
                        length: changed_owner_wire.len() as u32,
                        tgid: 300,
                        birth_boottime_ns: 3000,
                    },
                ),
                BOOT,
                false,
                true,
            )
            .expect("different actual sending lifetime");
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("owner change is retained as gap");
        assert_eq!(output.len(), 3);
        let originals: Vec<_> = output
            .iter()
            .flat_map(|intake| {
                let Intake::GatewayGap { frames, .. } = intake else {
                    panic!("only gaps after owner change");
                };
                frames.iter().map(|frame| frame.wire.as_slice())
            })
            .collect();
        assert_eq!(
            originals,
            [
                &changed_owner_wire[..],
                &client_fragment[..],
                &server_fragment[..]
            ]
        );
        assert_eq!(
            connection
                .gateway
                .as_ref()
                .expect("same connection")
                .retained(),
            0
        );

        output.clear();
        let later = [0x81, 1, b'z'];
        verified(
            &mut connection,
            false,
            offset + changed_owner_wire.len() as u64,
            &later,
        );
        connection
            .consume_gateway(PORTS, SOURCE, &mut output)
            .expect("original owner later resumes");
        assert!(matches!(
            output.as_slice(),
            [Intake::GatewayGap {
                reason: "websocket_owner_changed",
                ..
            }]
        ));
    }
}

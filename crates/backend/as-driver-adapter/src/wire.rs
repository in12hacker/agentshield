//! One current native body contract, paired with drivers/linux/as_protocol.h.
use as_types::{
    RuleId, RuleVersion,
    configuration::{DriverConfigVersion, ProtectionDefaults, ProtectionKind},
    context::TargetPlatform,
    dispatch_event::EventData,
    driver::{
        DirectHitCount, DriverInfo, NativeFutexSelection, NativeFutexWait, NativeRequestOperation,
        NativeRequestReturn, NativeSyncThread,
    },
    driver_control::*,
    driver_image::DriverInitParts,
    main_ports::{Decision, EventKind, ModuleError, RuleHit, RuleSourceVersion, Termination},
    operation::{ExecutionOutcome, SignalOutcome, TerminationReport},
    process_source::{
        ProcessBinding, ProcessFactsBarrier, ProcessInspection, ProcessKind, SampleWindow,
    },
    process_tree::EventKey,
};
use sha2::{Digest, Sha256};
use std::num::{NonZeroU32, NonZeroU64};

pub(crate) const MAX_FRAME: usize = 16384;
pub(crate) const MAX_TRANSFER: u64 = 4 * 1024 * 1024;
pub(crate) const STATUS_BYTES: usize = 144;
pub(crate) fn file_close(
    mut input: Vec<u8>,
) -> Result<as_types::security_log::FileCloseAuditRecord, ModuleError> {
    let reporter_tid = u32_at(&input, 0)? as i32;
    let device_major = u32_at(&input, 4)?;
    let device_minor = u32_at(&input, 8)?;
    let error = u32_at(&input, 12)? as i32;
    let inode = u64_at(&input, 16)?;
    let length = u32_at(&input, 24)? as usize;
    if u32_at(&input, 28)? != 0
        || reporter_tid < 0
        || input.len().checked_sub(32) != Some(length)
        || !(-4095..=0).contains(&error)
        || (error != 0 && length != 0)
    {
        return Err(ModuleError::InvalidInput);
    }
    let path = if error == 0 {
        input.drain(..32);
        Some(input)
    } else {
        None
    };
    Ok(
        as_types::security_log::FileCloseAuditRecord::FileWriteClose {
            reporter_tid,
            device_major,
            device_minor,
            inode,
            path,
            path_error: (error != 0).then_some(error),
        },
    )
}
pub(crate) fn group_tcp(
    mut input: Vec<u8>,
) -> Result<as_types::driver::NativeGroupTcp, ModuleError> {
    use as_types::driver::{NativeGroupObservation, NativeGroupTcp};
    let source = bytes(&input, 0)?;
    let ordinal = u64_at(&input, 16)?;
    let generation = u64_at(&input, 24)?;
    let sequence = u32_at(&input, 32)?;
    let acknowledgment = u32_at(&input, 36)?;
    let length = u32_at(&input, 40)?;
    let tgid = u32_at(&input, 44)?;
    let birth_boottime_ns = u64_at(&input, 48)?;
    let source_port = u16::from_le_bytes(bytes(&input, 56)?);
    let destination_port = u16::from_le_bytes(bytes(&input, 58)?);
    let [kind, flags, reserved1, reserved2] = bytes(&input, 60)?;
    let socket_cookie = u64_at(&input, 64)?;
    if source == [0; 16]
        || ordinal == 0
        || generation == 0
        || source_port == 0
        || destination_port == 0
        || reserved1 != 0
        || reserved2 != 0
    {
        return Err(ModuleError::InvalidInput);
    }
    let payload_len = input.len() - 72;
    let observation = match kind {
        0 if tgid == 0
            && birth_boottime_ns == 0
            && payload_len == length as usize
            && (length == 0 || socket_cookie != 0) =>
        {
            input.drain(..72);
            NativeGroupObservation::Packet {
                flags,
                payload: input,
            }
        }
        1 | 2
            if tgid > 0
                && birth_boottime_ns > 0
                && length > 0
                && socket_cookie != 0
                && flags == 0
                && acknowledgment == 0
                && payload_len == 0 =>
        {
            NativeGroupObservation::Transfer {
                received: kind == 2,
                length,
                tgid,
                birth_boottime_ns,
            }
        }
        3 if socket_cookie != 0
            && tgid == 0
            && birth_boottime_ns == 0
            && length == 0
            && flags == 0
            && acknowledgment == 0
            && payload_len == 0 =>
        {
            NativeGroupObservation::FirstSend
        }
        _ => return Err(ModuleError::InvalidInput),
    };
    Ok(NativeGroupTcp {
        source,
        ordinal,
        generation,
        source_port,
        destination_port,
        sequence,
        acknowledgment,
        socket_cookie,
        observation,
    })
}

pub(crate) fn futex_selection(input: &[u8]) -> Result<NativeFutexSelection, ModuleError> {
    if input.len() != 80 {
        return Err(ModuleError::InvalidInput);
    }
    let event = EventKey {
        source_instance: bytes(input, 0)?,
        sequence_domain: 1,
        sequence: u64_at(input, 16)?,
    };
    if event.source_instance == [0; 16] || event.sequence == 0 {
        return Err(ModuleError::InvalidInput);
    }
    let selected_boottime_ns = u64_at(input, 24)?;
    let read_thread = |offset| {
        let tgid = u32_at(input, offset)?;
        let tid = u32_at(input, offset + 4)?;
        if tgid == 0 || tid == 0 {
            return Err(ModuleError::InvalidInput);
        }
        Ok(NativeSyncThread {
            tgid,
            tid,
            birth_boottime_ns: u64_at(input, offset + 8)?,
        })
    };
    let id = u64_at(input, 64)?;
    let observed_boottime_ns = u64_at(input, 72)?;
    let wait = match id {
        0 if observed_boottime_ns == 0 => None,
        0 => return Err(ModuleError::InvalidInput),
        id if observed_boottime_ns <= selected_boottime_ns => Some(NativeFutexWait {
            id,
            observed_boottime_ns,
        }),
        _ => return Err(ModuleError::InvalidInput),
    };
    Ok(NativeFutexSelection {
        event,
        waker: read_thread(32)?,
        waiter: read_thread(48)?,
        selected_boottime_ns,
        wait,
    })
}

pub(crate) fn request_return(
    input: &[u8],
    session: [u8; 16],
) -> Result<NativeRequestReturn, ModuleError> {
    if input.len() != 56 {
        return Err(ModuleError::InvalidInput);
    }
    let event = EventKey {
        source_instance: bytes(input, 0)?,
        sequence_domain: u64_at(input, 16)?,
        sequence: u64_at(input, 24)?,
    };
    let wait_id = u64_at(input, 32)?;
    let returned_boottime_ns = u64_at(input, 40)?;
    let return_value = i32_at(input, 48)?;
    let operation = match u32_at(input, 52)? {
        1 => NativeRequestOperation::FileOpen,
        2 => NativeRequestOperation::Execute,
        3 => NativeRequestOperation::Connect,
        4 => NativeRequestOperation::Send,
        _ => return Err(ModuleError::InvalidInput),
    };
    if event.source_instance == [0; 16]
        || event.sequence_domain == 0
        || event.sequence == 0
        || wait_id == 0
        || return_value < -4095
    {
        return Err(ModuleError::InvalidInput);
    }
    Ok(NativeRequestReturn {
        event,
        operation,
        decision_id: crate::operation::native_decision_id(session, wait_id),
        wait_id,
        returned_boottime_ns,
        return_value,
    })
}

const CAP_BYTES: usize = 40;
fn bytes<const N: usize>(input: &[u8], at: usize) -> Result<[u8; N], ModuleError> {
    input
        .get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(ModuleError::InvalidInput)
}
pub(crate) fn u32_at(input: &[u8], at: usize) -> Result<u32, ModuleError> {
    Ok(u32::from_le_bytes(bytes(input, at)?))
}
pub(crate) fn u64_at(input: &[u8], at: usize) -> Result<u64, ModuleError> {
    Ok(u64::from_le_bytes(bytes(input, at)?))
}
pub(crate) fn facts_marker(input: &[u8]) -> Result<ProcessFactsBarrier, ModuleError> {
    if input.len() != 48 {
        return Err(ModuleError::InvalidInput);
    }
    let marker = ProcessFactsBarrier {
        tracking_epoch: u64_at(input, 0)?,
        marker: u64_at(input, 8)?,
        loss_epoch: u64_at(input, 16)?,
        host_pid_namespace_inode: u64_at(input, 24)?,
        backend_rejected: 0,
        cutoff_boottime_ns: u64_at(input, 32)?,
        native_hit_loss: u64_at(input, 40)?,
        backend_hit_rejected: 0,
    };
    if marker.tracking_epoch == 0 || marker.marker == 0 || marker.host_pid_namespace_inode == 0 {
        return Err(ModuleError::InvalidInput);
    }
    Ok(marker)
}
pub(crate) fn process_inspections(
    input: &[u8],
    requested: &[u32],
) -> Result<Vec<ProcessInspection>, ModuleError> {
    if input.len() != 8 + 64 * requested.len()
        || u32_at(input, 0)? as usize != requested.len()
        || u32_at(input, 4)? != 0
    {
        return Err(ModuleError::InvalidInput);
    }
    let mut results = Vec::with_capacity(requested.len());
    for (row, requested) in input[8..].chunks_exact(64).zip(requested) {
        let pid = u32_at(row, 0)?;
        if pid != *requested {
            return Err(ModuleError::InvalidInput);
        }
        results.push(match i32_at(row, 4)? {
            -3 => ProcessInspection::Gone { host_tgid: pid },
            -1 | -13 => ProcessInspection::Inaccessible { host_tgid: pid },
            0 => {
                let kind = match u32_at(row, 20)? {
                    0 => ProcessKind::User,
                    1 => ProcessKind::Kernel,
                    _ => return Err(ModuleError::InvalidInput),
                };
                let window = SampleWindow {
                    begin_boottime_ns: u64_at(row, 48)?,
                    end_boottime_ns: u64_at(row, 56)?,
                };
                if pid == 0 || window.begin_boottime_ns > window.end_boottime_ns {
                    return Err(ModuleError::InvalidInput);
                }
                ProcessInspection::Bound {
                    binding: ProcessBinding {
                        host_tgid: pid,
                        birth_boottime_ns: u64_at(row, 8)?,
                        parent_pid: u32_at(row, 16)?,
                        kind,
                        parent_birth_boottime_ns: u64_at(row, 24)?,
                        exec_cookie: u64_at(row, 32)?,
                        cgroup_id: match u64_at(row, 40)? {
                            0 => None,
                            value => Some(value),
                        },
                        window,
                    },
                }
            }
            _ => return Err(ModuleError::InvalidInput),
        });
    }
    Ok(results)
}
fn i32_at(input: &[u8], at: usize) -> Result<i32, ModuleError> {
    Ok(i32::from_le_bytes(bytes(input, at)?))
}
pub(crate) fn error(errno: i32) -> ModuleError {
    match errno.unsigned_abs() {
        1 | 13 => ModuleError::Forbidden,
        22 | 71 | 90 => ModuleError::InvalidInput,
        95 => ModuleError::Unsupported,
        116 => ModuleError::Conflict,
        11 => ModuleError::Busy,
        16 | 28 | 12 => ModuleError::Busy,
        2 | 62 | 108 | 114 => ModuleError::Stopped,
        _ => ModuleError::Unavailable,
    }
}
pub(crate) fn rejected_reply(input: &[u8]) -> bool {
    input.len() >= 8
        && matches!(u32_at(input, 4), Ok(0))
        && i32_at(input, 0).is_ok_and(|code| code < 0)
}
pub(crate) fn reply(input: &[u8]) -> Result<&[u8], ModuleError> {
    if input.len() < 8 || u32_at(input, 4)? != 0 {
        return Err(ModuleError::InvalidInput);
    }
    let code = i32_at(input, 0)?;
    if code > 0 {
        return Err(ModuleError::InvalidInput);
    }
    if code != 0 {
        tracing::error!(errno = code, "native request rejected");
        return Err(error(code));
    }
    Ok(&input[8..])
}
fn feature(value: u32) -> Result<FeatureState, ModuleError> {
    match value {
        0 => Ok(FeatureState::Unavailable),
        1 => Ok(FeatureState::Enabled),
        2 => Ok(FeatureState::Disabled),
        _ => Err(ModuleError::InvalidInput),
    }
}
fn family_status(
    defaults: u32,
    overrides: u32,
    effective: u32,
    index: u32,
) -> Result<FeatureStatus, ModuleError> {
    Ok(FeatureStatus {
        startup_default: defaults & (1 << index) != 0,
        runtime_override: match (overrides >> (index * 2)) & 3 {
            0 => RuntimeOverride::Default,
            1 => RuntimeOverride::Enabled,
            2 => RuntimeOverride::Disabled,
            _ => return Err(ModuleError::InvalidInput),
        },
        effective_state: feature((effective >> (index * 2)) & 3)?,
    })
}
pub(crate) fn status(input: &[u8]) -> Result<DriverStatus, ModuleError> {
    if input.len() != STATUS_BYTES {
        return Err(ModuleError::InvalidInput);
    }
    let instance = bytes(input, 0)?;
    let flags = u32_at(input, 20)?;
    let defaults = u32_at(input, 104)?;
    let overrides = u32_at(input, 108)?;
    let effective = u32_at(input, 112)?;
    if instance == [0; 16]
        || flags & !1 != 0
        || defaults & !7 != 0
        || overrides & !63 != 0
        || effective & !63 != 0
    {
        return Err(ModuleError::InvalidInput);
    }
    let persistent = flags & 1 != 0;
    let native_error = i32_at(input, 136)?;
    if native_error > 0 {
        return Err(ModuleError::InvalidInput);
    }
    let config_digest = bytes(input, 40)?;
    let rule_digest = bytes(input, 72)?;
    Ok(DriverStatus {
        instance,
        revision: DRIVER_REVISION,
        source_state: match u32_at(input, 16)? {
            0 => SourceState::Dormant,
            1 => SourceState::Running,
            2 => SourceState::Quiescing,
            3 => SourceState::Failed,
            _ => return Err(ModuleError::InvalidInput),
        },
        persistent_generation: u64_at(input, 24)?,
        state_revision: u64_at(input, 32)?,
        lifecycle_epoch: u32_at(input, 140)?,
        driver_config_version: persistent.then_some(DriverConfigVersion(config_digest)),
        rule_version: persistent.then_some(RuleVersion::from_digest(rule_digest)),
        startup_protection: persistent.then_some(ProtectionDefaults {
            process: defaults & 1 != 0,
            file: defaults & 2 != 0,
            network: defaults & 4 != 0,
        }),
        process: family_status(defaults, overrides, effective, 0)?,
        file: family_status(defaults, overrides, effective, 1)?,
        network: family_status(defaults, overrides, effective, 2)?,
        self_protection: feature(u32_at(input, 116)?)?,
        pending: u32_at(input, 120)?,
        pending_receipts: u32_at(input, 124)?,
        dropped: u64_at(input, 128)?,
        error: (native_error != 0).then(|| format!("native errno {native_error}")),
    })
}
pub(crate) fn hello(input: &[u8], session_id: [u8; 16]) -> Result<DriverInfo, ModuleError> {
    if input.len() != STATUS_BYTES + CAP_BYTES || session_id == [0; 16] {
        return Err(ModuleError::InvalidInput);
    }
    let current = status(&input[..STATUS_BYTES])?;
    let c = &input[STATUS_BYTES..];
    let caps = DriverCapabilities {
        max_frame_bytes: u32_at(c, 0)?,
        max_pending: u32_at(c, 4)?,
        max_transfer_bytes: u64_at(c, 8)?,
        max_hold_ms: u32_at(c, 16)?,
        operation_mask: u32_at(c, 20)?,
        condition_mask: u64_at(c, 24)?,
        features: u32_at(c, 32)?,
    };
    if u32_at(c, 36)? != 0
        || caps.max_frame_bytes < 256
        || caps.max_frame_bytes as usize > MAX_FRAME
        || caps.max_pending == 0
        || caps.max_hold_ms == 0
        || caps.max_transfer_bytes < 136
        || caps.max_transfer_bytes > MAX_TRANSFER
        || caps.operation_mask & !DriverCapabilities::OPERATION_MASK != 0
        || caps.condition_mask & !((1u64 << 26) - 1) != 0
        || caps.features & !(127 | 256 | 512 | 1024 | 2048) != 0
    {
        return Err(ModuleError::InvalidInput);
    }
    if !caps.supports_feature(DriverFeature::ControlPriority) {
        return Err(ModuleError::Unsupported);
    }
    Ok(DriverInfo {
        session_id,
        platform: TargetPlatform::Linux,
        capabilities: caps,
        status: current,
    })
}
pub(crate) fn feature_id(kind: ProtectionKind) -> u32 {
    match kind {
        ProtectionKind::Process => 1,
        ProtectionKind::File => 2,
        ProtectionKind::Network => 3,
    }
}
pub(crate) fn decision(value: Decision) -> (u32, u32) {
    match value {
        Decision::Allow => (0, 0),
        Decision::Deny { terminate } => (
            1,
            match terminate {
                Termination::None => 0,
                Termination::Actor => 1,
                Termination::ActorAndParent => 2,
            },
        ),
    }
}
pub(crate) fn images(parts: &DriverInitParts, info: &DriverInfo) -> Result<Vec<u8>, ModuleError> {
    if parts.configuration.platform != info.platform
        || parts.rules.platform != info.platform
        || parts.configuration.encoding_revision != 1
        || parts.rules.encoding_revision != 1
        || parts.configuration.immutable_buffer.is_empty()
        || parts.rules.immutable_buffer.is_empty()
    {
        return Err(ModuleError::InvalidInput);
    }
    let config = &parts.configuration.immutable_buffer;
    let rules = &parts.rules.immutable_buffer;
    let size = 136usize
        .checked_add(config.len())
        .and_then(|n| n.checked_add(rules.len()))
        .ok_or(ModuleError::InvalidInput)?;
    if size as u64 > info.capabilities.max_transfer_bytes {
        return Err(ModuleError::Busy);
    }
    let mut body = Vec::new();
    body.try_reserve_exact(size)
        .map_err(|_| ModuleError::Busy)?;
    body.resize(136, 0);
    body[0..8].copy_from_slice(&parts.expected_persistent_generation.to_le_bytes());
    body[8..10].copy_from_slice(&1u16.to_le_bytes());
    body[20..24].copy_from_slice(&parts.expected_lifecycle_epoch.to_le_bytes());
    body[12..16].copy_from_slice(&parts.configuration.encoding_revision.to_le_bytes());
    body[16..20].copy_from_slice(&parts.rules.encoding_revision.to_le_bytes());
    body[24..32].copy_from_slice(&(config.len() as u64).to_le_bytes());
    body[32..40].copy_from_slice(&(rules.len() as u64).to_le_bytes());
    body[40..72].copy_from_slice(&parts.configuration.driver_config_version.0);
    body[72..104].copy_from_slice(&parts.rules.rule_version.digest());
    let mut hash = Sha256::new();
    hash.update(config);
    hash.update(rules);
    body[104..136].copy_from_slice(&hash.finalize());
    body.extend_from_slice(config);
    body.extend_from_slice(rules);
    Ok(body)
}
pub(crate) struct Controlled {
    pub id: u64,
    pub started: u64,
    pub deadline: u64,
    pub generation: u64,
    pub rules: RuleVersion,
    pub event: EventData,
    pub hit: Option<RuleHit>,
}
fn hit(input: &[u8], config: [u8; 32], rules: RuleVersion) -> Result<RuleHit, ModuleError> {
    if input.len() < 20 {
        return Err(ModuleError::InvalidInput);
    }
    let group = NonZeroU32::new(u32_at(input, 0)?).ok_or(ModuleError::InvalidInput)?;
    let number = NonZeroU32::new(u32_at(input, 4)?).ok_or(ModuleError::InvalidInput)?;
    let log = match u32_at(input, 8)? {
        0 => false,
        1 => true,
        _ => return Err(ModuleError::InvalidInput),
    };
    let source = match u32_at(input, 12)? {
        1 => RuleSourceVersion::Configuration(DriverConfigVersion(config)),
        2 => RuleSourceVersion::RulesDocument(rules),
        _ => return Err(ModuleError::InvalidInput),
    };
    let group_size = u16::from_le_bytes(bytes(input, 16)?) as usize;
    let id_size = u16::from_le_bytes(bytes(input, 18)?) as usize;
    if group_size == 0
        || id_size == 0
        || 20usize
            .checked_add(group_size)
            .and_then(|n| n.checked_add(id_size))
            != Some(input.len())
    {
        return Err(ModuleError::InvalidInput);
    }
    let group_id = std::str::from_utf8(&input[20..20 + group_size])
        .map_err(|_| ModuleError::InvalidInput)?
        .to_owned();
    let id =
        std::str::from_utf8(&input[20 + group_size..]).map_err(|_| ModuleError::InvalidInput)?;
    Ok(RuleHit {
        group_id,
        group_number: group,
        rule_id: RuleId::from(id),
        rule_number: number,
        source,
        log,
        deny_message: None,
    })
}
pub(crate) fn direct_hits(input: &[u8]) -> Result<Vec<DirectHitCount>, ModuleError> {
    let source_instance = bytes(input, 0)?;
    let mut records = &input[16..];
    if source_instance == [0; 16] || records.is_empty() {
        return Err(ModuleError::InvalidInput);
    }
    let mut batch = Vec::new();
    while !records.is_empty() {
        let first = u64_at(records, 0)?;
        let last = u64_at(records, 8)?;
        let count = NonZeroU64::new(u64_at(records, 24)?).ok_or(ModuleError::InvalidInput)?;
        let kind = EventKind::try_from(u16::from_le_bytes(bytes(records, 36)?))
            .map_err(|_| ModuleError::InvalidInput)?;
        let end = 72 + usize::from(u16::from_le_bytes(bytes(records, 38)?));
        let version = bytes(records, 40)?;
        let hit = hit(
            records.get(72..end).ok_or(ModuleError::InvalidInput)?,
            version,
            RuleVersion::from_digest(version),
        )?;
        if first == 0
            || last < first
            || count.get() > last - first + 1
            || (count.get() == 1) != (first == last)
            || (matches!(hit.source, RuleSourceVersion::Configuration(_))
                && (kind != EventKind::File || hit.group_number.get() != 1))
        {
            return Err(ModuleError::InvalidInput);
        }
        let key = |sequence| EventKey {
            source_instance,
            sequence_domain: 1,
            sequence,
        };
        batch.push(DirectHitCount {
            host_tgid: NonZeroU32::new(u32_at(records, 32)?).ok_or(ModuleError::InvalidInput)?,
            birth_boottime_ns: u64_at(records, 16)?,
            kind,
            hit,
            count,
            first: key(first),
            last: key(last),
        });
        records = &records[end..];
    }
    Ok(batch)
}
pub(crate) fn controlled(input: &[u8], direct: bool) -> Result<(Controlled, &[u8]), ModuleError> {
    if input.len() < 128 || input[112..128] != [0; 16] {
        return Err(ModuleError::InvalidInput);
    }
    let id = u64_at(input, 0)?;
    if (id == 0) != direct {
        return Err(ModuleError::InvalidInput);
    }
    let event_bytes = u32_at(input, 104)? as usize;
    let hit_bytes = u32_at(input, 108)? as usize;
    let end = 128usize
        .checked_add(hit_bytes)
        .and_then(|n| n.checked_add(event_bytes))
        .ok_or(ModuleError::InvalidInput)?;
    if end > input.len() || (!direct && end != input.len()) {
        return Err(ModuleError::InvalidInput);
    }
    let rules = RuleVersion::from_digest(bytes(input, 72)?);
    let config = bytes(input, 40)?;
    let matched = if hit_bytes == 0 {
        None
    } else {
        Some(hit(&input[128..128 + hit_bytes], config, rules)?)
    };
    if direct && matched.is_none()
        || !direct
            && matched
                .as_ref()
                .is_some_and(|v| !matches!(v.source, RuleSourceVersion::RulesDocument(_)))
    {
        return Err(ModuleError::InvalidInput);
    }
    let mut event = as_types::dispatch_event::decode(&input[128 + hit_bytes..end])
        .map_err(|_| ModuleError::InvalidInput)?;
    super::dns::interpret(&mut event)?;
    if event.stage()
        != if direct {
            as_types::main_ports::EventStage::ExecutionResult
        } else {
            as_types::main_ports::EventStage::ControlledRequest
        }
    {
        return Err(ModuleError::InvalidInput);
    }
    if matched.as_ref().is_some_and(|hit| {
        matches!(hit.source, RuleSourceVersion::Configuration(_))
            && (event.kind() != as_types::main_ports::EventKind::File
                || hit.group_number.get() != 1)
    }) {
        return Err(ModuleError::InvalidInput);
    }
    Ok((
        Controlled {
            id,
            started: u64_at(input, 8)?,
            deadline: u64_at(input, 16)?,
            generation: u64_at(input, 24)?,
            rules,
            event,
            hit: matched,
        },
        &input[end..],
    ))
}
pub(crate) struct NativeResult {
    pub id: u64,
    pub generation: u64,
    pub rules: RuleVersion,
    pub outcome: ExecutionOutcome,
    pub termination: TerminationReport,
}
fn signal_result(value: i32, requested: bool) -> Result<SignalOutcome, ModuleError> {
    if !requested {
        return if value == -61 {
            Ok(SignalOutcome::NotRequested)
        } else {
            Err(ModuleError::InvalidInput)
        };
    }
    match value {
        -3 => Ok(SignalOutcome::NotRunning),
        0 => Ok(SignalOutcome::SignalSent),
        native_error if native_error < 0 => Ok(SignalOutcome::Failed { native_error }),
        _ => Err(ModuleError::InvalidInput),
    }
}
pub(crate) fn result(input: &[u8]) -> Result<NativeResult, ModuleError> {
    if input.len() != 64 {
        return Err(ModuleError::InvalidInput);
    }
    let code = i32_at(input, 16)?;
    let requested = match u32_at(input, 20)? {
        0 => Termination::None,
        1 => Termination::Actor,
        2 => Termination::ActorAndParent,
        _ => return Err(ModuleError::InvalidInput),
    };
    let actor = signal_result(i32_at(input, 24)?, requested != Termination::None)?;
    let parent = signal_result(i32_at(input, 28)?, requested == Termination::ActorAndParent)?;
    if requested != Termination::None && code != -1 && code != -13 {
        return Err(ModuleError::InvalidInput);
    }
    let outcome = match code {
        0 => ExecutionOutcome::Allowed,
        -1 | -13 => ExecutionOutcome::Denied,
        -125 => ExecutionOutcome::Cancelled,
        native_error if native_error < 0 => ExecutionOutcome::Failed { native_error },
        _ => return Err(ModuleError::InvalidInput),
    };
    Ok(NativeResult {
        id: u64_at(input, 0)?,
        generation: u64_at(input, 8)?,
        rules: RuleVersion::from_digest(bytes(input, 32)?),
        outcome,
        termination: TerminationReport {
            requested,
            actor,
            parent,
        },
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn close_facts_preserve_raw_empty_path_zero_values_and_native_error() {
        use as_types::security_log::FileCloseAuditRecord::FileWriteClose;
        let body = |path: &[u8], error: i32| {
            let mut bytes = vec![0; 32];
            bytes[12..16].copy_from_slice(&error.to_le_bytes());
            bytes[24..28].copy_from_slice(&(path.len() as u32).to_le_bytes());
            bytes.extend_from_slice(path);
            bytes
        };
        for path in [b"".as_slice(), b"/work/\xff".as_slice()] {
            let FileWriteClose {
                reporter_tid,
                path: actual,
                path_error,
                inode,
                ..
            } = file_close(body(path, 0)).expect("actual current close shape");
            assert_eq!(reporter_tid, 0);
            assert_eq!(inode, 0);
            assert_eq!(actual.as_deref(), Some(path));
            assert_eq!(path_error, None);
        }
        let FileWriteClose {
            path, path_error, ..
        } = file_close(body(b"", -36)).expect("real path read failure");
        assert_eq!(path, None);
        assert_eq!(path_error, Some(-36));
        assert!(file_close(body(b"not a returned path", -36)).is_err());
        assert!(file_close(body(b"", 1)).is_err());
        let mut malformed = body(b"", 0);
        malformed[28] = 1;
        assert!(file_close(malformed).is_err());
    }
    #[test]
    fn deny_receipt_keeps_requested_failures_separate_from_unrequested_signals() {
        let mut body = vec![0; 64];
        body[16..20].copy_from_slice(&(-1i32).to_le_bytes());
        body[20..24].copy_from_slice(&2u32.to_le_bytes());
        body[24..28].copy_from_slice(&0i32.to_le_bytes());
        body[28..32].copy_from_slice(&(-61i32).to_le_bytes());
        let parsed = result(&body).expect("a requested LSM ENODATA is a real failure");
        assert_eq!(parsed.outcome, ExecutionOutcome::Denied);
        assert_eq!(parsed.termination.actor, SignalOutcome::SignalSent);
        assert_eq!(
            parsed.termination.parent,
            SignalOutcome::Failed { native_error: -61 }
        );
        body[20..24].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            result(&body).is_err(),
            "unrequested successful signal is malformed"
        );
        body[24..28].copy_from_slice(&(-61i32).to_le_bytes());
        assert_eq!(
            result(&body).expect("plain deny").termination,
            TerminationReport::NONE
        );
        body[20..24].copy_from_slice(&1u32.to_le_bytes());
        body[16..20].copy_from_slice(&0i32.to_le_bytes());
        assert!(result(&body).is_err(), "allow never carries termination");
    }
    #[test]
    fn original_result_preserves_native_errors_without_changing_existing_outcomes() {
        let mut body = vec![0; 64];
        body[24..28].copy_from_slice(&(-61i32).to_le_bytes());
        body[28..32].copy_from_slice(&(-61i32).to_le_bytes());
        for (code, outcome) in [
            (0i32, ExecutionOutcome::Allowed),
            (-1, ExecutionOutcome::Denied),
            (-13, ExecutionOutcome::Denied),
            (-125, ExecutionOutcome::Cancelled),
            (-110, ExecutionOutcome::Failed { native_error: -110 }),
            (-5, ExecutionOutcome::Failed { native_error: -5 }),
        ] {
            body[16..20].copy_from_slice(&code.to_le_bytes());
            let parsed = result(&body).expect("actual native result");
            assert_eq!(parsed.outcome, outcome);
            assert_eq!(parsed.termination, TerminationReport::NONE);
        }
        body[16..20].copy_from_slice(&1i32.to_le_bytes());
        assert!(matches!(result(&body), Err(ModuleError::InvalidInput)));
    }
    pub(crate) fn status_body() -> Vec<u8> {
        let mut body = vec![0; 144];
        body[..16].copy_from_slice(&[7; 16]);
        body[16..20].copy_from_slice(&1u32.to_le_bytes());
        body[20..24].copy_from_slice(&1u32.to_le_bytes());
        body[24..32].copy_from_slice(&1u64.to_le_bytes());
        body[32..40].copy_from_slice(&2u64.to_le_bytes());
        body[40..72].copy_from_slice(&[8; 32]);
        body[72..104].copy_from_slice(&[9; 32]);
        body[104..108].copy_from_slice(&7u32.to_le_bytes());
        body[108..112].copy_from_slice(&2u32.to_le_bytes());
        body[112..116].copy_from_slice(&22u32.to_le_bytes());
        body
    }
    fn hello_body() -> Vec<u8> {
        let mut body = status_body();
        body.extend(16384u32.to_le_bytes());
        body.extend(64u32.to_le_bytes());
        body.extend(MAX_TRANSFER.to_le_bytes());
        body.extend(300000u32.to_le_bytes());
        body.extend(DriverCapabilities::OPERATION_MASK.to_le_bytes());
        body.extend(((1u64 << 25) - 1).to_le_bytes());
        body.extend((DriverFeature::ControlPriority as u32).to_le_bytes());
        body.extend(0u32.to_le_bytes());
        body
    }
    pub(crate) fn info() -> DriverInfo {
        hello(&hello_body(), [6; 16]).expect("actual-shape fixture")
    }
    #[test]
    fn actual_status_does_not_replace_override_or_unavailable_self_protection_with_defaults() {
        let parsed = info();
        let mut missing_priority = hello_body();
        missing_priority[STATUS_BYTES + 32..STATUS_BYTES + 36].fill(0);
        assert!(
            matches!(
                hello(&missing_priority, [6; 16]),
                Err(ModuleError::Unsupported)
            ),
            "current native connection requires actual completion PRI support"
        );
        for operation in [1, 4, 5, 6, 7, 9, 10, 18, 19] {
            assert!(
                parsed
                    .capabilities
                    .supports_controlled(as_types::main_ports::EventKind::File, operation)
            );
        }
        for operation in [2, 3, 8, 11, 12, 13, 14, 15, 16, 17] {
            assert!(
                !parsed
                    .capabilities
                    .supports_controlled(as_types::main_ports::EventKind::File, operation)
            );
        }
        let mut old_source = parsed.capabilities.clone();
        old_source.operation_mask = 127;
        assert!(
            !old_source.supports_controlled(as_types::main_ports::EventKind::File, 9),
            "new interception is never inferred from observation bits"
        );
        assert!(parsed.status.process.startup_default);
        assert_eq!(
            parsed.status.process.runtime_override,
            RuntimeOverride::Disabled
        );
        assert_eq!(
            parsed.status.process.effective_state,
            FeatureState::Disabled
        );
        assert_eq!(parsed.status.self_protection, FeatureState::Unavailable);
        assert!(
            !parsed
                .capabilities
                .supports_feature(DriverFeature::SelfProtection)
        );
        let mut invalid = status_body();
        invalid[112] = 3;
        assert!(status(&invalid).is_err());
        let rejected = 95i32
            .to_le_bytes()
            .into_iter()
            .chain([0; 4])
            .collect::<Vec<_>>();
        assert!(reply(&rejected).is_err());
    }
}

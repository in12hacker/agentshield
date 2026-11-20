//! Export the current native rule representation; the kernel remains the actual installer.
use super::{
    Error, PolicyEngine, PolicyLease,
    compile::{Bytes, CompiledRule, Condition, Number, Text},
    model::*,
};
use as_types::{
    context::TargetPlatform,
    driver_control::{DriverCapabilities, DriverCondition as Field, DriverFeature},
    driver_image::DriverPolicyImage,
    main_ports::{Decision, EventKind, Termination},
};
use std::sync::Arc;

fn too_large(_: impl std::fmt::Debug) -> Error {
    Error::Unsupported("native representation length overflow")
}
fn operation_bits(operations: impl IntoIterator<Item = u32>) -> Result<u32, Error> {
    operations.into_iter().try_fold(0, |bits, operation| {
        let shift = operation
            .checked_sub(1)
            .ok_or(Error::Unsupported("invalid operation"))?;
        Ok(bits
            | 1u32
                .checked_shl(shift)
                .ok_or(Error::Unsupported("invalid native operation"))?)
    })
}
fn encode_backend_routes(
    out: &mut Vec<u8>,
    kind: EventKind,
    rules: &[CompiledRule],
    caps: &DriverCapabilities,
) -> Result<(u32, u32), Error> {
    let mut unfiltered = 0;
    let mut count = 0;
    for rule in rules {
        let operations = operation_bits(rule.operations.iter().copied())?;
        let target = rule.conditions.iter().find(|condition| {
            let field = match (kind, condition) {
                (
                    EventKind::Process,
                    Condition::Bytes(Bytes::Image, Text::Exact(_) | Text::Prefix(_)),
                ) => Field::TargetExecutable,
                (
                    EventKind::File,
                    Condition::Bytes(Bytes::Path, Text::Exact(_) | Text::Prefix(_)),
                ) => Field::FilePath,
                (EventKind::Network, Condition::Port(false, _, _)) => Field::RemotePort,
                _ => return false,
            };
            caps.supports_condition(field)
        });
        let necessary = target.or_else(|| {
            rule.conditions.iter().find(|condition| {
                kind == EventKind::File
                    && caps.supports_condition(Field::AccessMask)
                    && matches!(condition, Condition::Number(Number::Mask, _))
            })
        });
        if let Some(condition) = necessary {
            out.extend((kind as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(operations.to_le_bytes());
            encode_condition(out, condition, caps)?;
            count += 1;
        } else {
            unfiltered |= operations;
        }
    }
    Ok((unfiltered, count))
}

fn termination(value: Termination, caps: &DriverCapabilities) -> Result<u32, Error> {
    match value {
        Termination::None => Ok(0),
        Termination::Actor if caps.supports_feature(DriverFeature::TerminateActor) => Ok(1),
        Termination::ActorAndParent
            if caps.supports_feature(DriverFeature::TerminateActor)
                && caps.supports_feature(DriverFeature::TerminateParent) =>
        {
            Ok(2)
        }
        _ => Err(Error::Unsupported(
            "requested original termination is not supported",
        )),
    }
}
fn validate_actions(doc: &PolicyDocument, caps: &DriverCapabilities) -> Result<(), Error> {
    fn group<T, O: super::compile::OperationCode>(
        g: &Group<T, O>,
        kind: EventKind,
        caps: &DriverCapabilities,
    ) -> Result<(), Error> {
        if !g.enabled {
            return Ok(());
        }
        for rule in g.rules.iter().filter(|r| r.enabled) {
            if matches!(rule.scope, Scope::Containers { .. })
                && !caps.supports_feature(DriverFeature::ContainerBinding)
            {
                return Err(Error::Unsupported(
                    "exact container-instance scope has no active source",
                ));
            }
            for op in &rule.operations {
                if kind == EventKind::File
                    && op.code() == as_types::event_operation::FileOperation::Mount as u32
                    && g.execution.is_driver()
                    && caps.supports_feature(DriverFeature::SelfMountRules)
                {
                    continue;
                }
                if !caps.supports_controlled(kind, op.code()) {
                    return Err(Error::Unsupported(
                        "operation lacks the complete driver/backend control chain",
                    ));
                }
            }
            match &rule.action {
                Action::Deny { terminate } => {
                    termination(*terminate, caps)?;
                }
                Action::Ask {
                    timeout: Decision::Deny { terminate },
                    ..
                } => {
                    termination(*terminate, caps)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    for g in &doc.groups {
        match g {
            RuleGroup::Process(v) => {
                if v.enabled
                    && v.rules
                        .iter()
                        .any(|r| r.enabled && !r.target.arguments.is_empty())
                    && !caps.supports_condition(Field::TargetArguments)
                {
                    return Err(Error::Unsupported(
                        "execution target arguments have no active source",
                    ));
                }
                group(v, EventKind::Process, caps)?;
            }
            RuleGroup::File(v) => {
                if v.enabled
                    && v.rules
                        .iter()
                        .any(|r| r.enabled && r.target.new_path.is_some())
                    && !caps.supports_condition(Field::NewPath)
                {
                    return Err(Error::Unsupported(
                        "file destination path has no active source",
                    ));
                }
                group(v, EventKind::File, caps)?;
            }
            RuleGroup::Network(v) => {
                if v.enabled
                    && v.rules
                        .iter()
                        .any(|r| r.enabled && r.target.domain.is_some())
                    && !caps.supports_feature(DriverFeature::DnsQuerySnapshot)
                {
                    return Err(Error::Unsupported(
                        "DNS query snapshot has no active source",
                    ));
                }
                group(v, EventKind::Network, caps)?;
            }
        }
    }
    Ok(())
}
impl PolicyEngine {
    /// Validate every active rule's source action and export only ordinary driver groups.
    /// The exact saved snapshot stays pinned until native generation retirement.
    pub fn prepare_driver_policy(
        &self,
        caps: &DriverCapabilities,
    ) -> Result<(DriverPolicyImage, PolicyLease), Error> {
        let snapshot = self.active.load_full();
        validate_actions(&snapshot.document, caps)?;
        let plan = &snapshot.driver;
        let mut body = vec![0u8; 32];
        let mut count = 0u32;
        for (kind, pool) in [
            (EventKind::Process, &plan.process),
            (EventKind::File, &plan.file),
            (EventKind::Network, &plan.network),
        ] {
            for rule in pool {
                encode_rule(&mut body, kind, rule, caps)?;
                count = count
                    .checked_add(1)
                    .ok_or(Error::Unsupported("too many native rules"))?;
            }
        }
        let mut backend = [0u32; 3];
        let mut routes = 0u32;
        for (index, (kind, rules)) in [
            (EventKind::Process, &snapshot.process.0.rules),
            (EventKind::File, &snapshot.file.0.rules),
            (EventKind::Network, &snapshot.network.0.rules),
        ]
        .into_iter()
        .enumerate()
        {
            let (unfiltered, count) = encode_backend_routes(&mut body, kind, rules, caps)?;
            backend[index] = unfiltered;
            routes = routes
                .checked_add(count)
                .ok_or(Error::Unsupported("too many backend routes"))?;
        }
        let length = u64::try_from(body.len()).map_err(too_large)?;
        if length > caps.max_transfer_bytes {
            return Err(Error::Unsupported(
                "policy exceeds actual native transfer capacity",
            ));
        }
        body[0..4].copy_from_slice(&32u32.to_le_bytes());
        body[4..8].copy_from_slice(&count.to_le_bytes());
        body[8..16].copy_from_slice(&length.to_le_bytes());
        body[28..32].copy_from_slice(&routes.to_le_bytes());
        for (index, bits) in backend.into_iter().enumerate() {
            body[16 + index * 4..20 + index * 4].copy_from_slice(&bits.to_le_bytes());
        }
        Ok((
            DriverPolicyImage {
                rule_version: snapshot.version,
                platform: TargetPlatform::current(),
                encoding_revision: 1,
                immutable_buffer: Arc::from(body),
            },
            PolicyLease(snapshot),
        ))
    }
}
fn encode_rule(
    out: &mut Vec<u8>,
    kind: EventKind,
    r: &CompiledRule,
    caps: &DriverCapabilities,
) -> Result<(), Error> {
    if !matches!(r.scope, Scope::AllProtected) {
        return Err(Error::Unsupported(
            "exact container-instance native binding is not implemented",
        ));
    }
    let group = r.hit.group_id.as_bytes();
    let id = r.hit.rule_id.as_str().as_bytes();
    let group_len = u16::try_from(group.len()).map_err(too_large)?;
    let id_len = u16::try_from(id.len()).map_err(too_large)?;
    let operations = operation_bits(r.operations.iter().copied())?;
    let (action, terminate) = match r.action {
        Action::Allow => (0u16, 0),
        Action::Deny { terminate } => (1, termination(terminate, caps)?),
        Action::ToUser => (2, 0),
        Action::Ask { .. } => return Err(Error::Unsupported("Ask cannot execute in the driver")),
    };
    let start = out.len();
    out.extend([0u8; 40]);
    out.extend(group);
    out.extend(id);
    for condition in &r.conditions {
        encode_condition(out, condition, caps)?;
    }
    let bytes = u32::try_from(out.len() - start).map_err(too_large)?;
    let count = u32::try_from(r.conditions.len()).map_err(too_large)?;
    out[start..start + 4].copy_from_slice(&bytes.to_le_bytes());
    out[start + 4..start + 6].copy_from_slice(&(kind as u16).to_le_bytes());
    out[start + 6..start + 8].copy_from_slice(&action.to_le_bytes());
    for (at, value) in [
        (8, operations),
        (12, terminate),
        (16, r.hit.group_number.get()),
        (20, r.hit.rule_number.get()),
        (24, u32::from(r.hit.log)),
        (28, count),
        (
            36,
            u32::from(
                matches!(r.action, Action::Deny { .. })
                    && r.hit
                        .deny_message
                        .as_ref()
                        .is_some_and(|text| !text.is_empty()),
            ),
        ),
    ] {
        out[start + at..start + at + 4].copy_from_slice(&value.to_le_bytes());
    }
    out[start + 32..start + 34].copy_from_slice(&group_len.to_le_bytes());
    out[start + 34..start + 36].copy_from_slice(&id_len.to_le_bytes());
    if out.len() as u64 > caps.max_transfer_bytes {
        return Err(Error::Unsupported("native policy exceeds capacity"));
    }
    Ok(())
}
fn text(value: &Text) -> Result<(u8, Vec<u8>), Error> {
    match value {
        Text::Exact(v) => Ok((1, v.clone())),
        Text::Prefix(v) => Ok((2, v.clone())),
        _ => Err(Error::Unsupported(
            "driver text operator must be exact or prefix",
        )),
    }
}
fn encode_condition(
    out: &mut Vec<u8>,
    c: &Condition,
    caps: &DriverCapabilities,
) -> Result<(), Error> {
    let (field, operator, index, value) = match c {
        Condition::MountEffect(effect) => (
            Field::MountEffect,
            3,
            0,
            (*effect as u32).to_le_bytes().to_vec(),
        ),
        Condition::DnsDomain(_) => {
            return Err(Error::Unsupported(
                "DNS question snapshots are matched only by the backend",
            ));
        }
        Condition::Bytes(field, value) => {
            let field = match field {
                Bytes::Name => Field::Name,
                Bytes::Exe => Field::Executable,
                Bytes::Cwd => Field::Cwd,
                Bytes::ParentName => Field::ParentName,
                Bytes::ParentExe => Field::ParentExecutable,
                Bytes::Image => Field::TargetExecutable,
                Bytes::Path => Field::FilePath,
                Bytes::NewPath => Field::NewPath,
            };
            let (op, value) = text(value)?;
            (field, op, 0, value)
        }
        Condition::Arg(target, index, value) => {
            let (op, value) = text(value)?;
            (
                if *target {
                    Field::TargetArguments
                } else {
                    Field::Arguments
                },
                op,
                u32::try_from(*index).map_err(too_large)?,
                value,
            )
        }
        Condition::Number(field, value) => {
            let field = match field {
                Number::Pid => Field::ActorPid,
                Number::Ppid => Field::ParentPid,
                Number::Uid => Field::Uid,
                Number::Gid => Field::Gid,
                Number::Euid => Field::Euid,
                Number::Egid => Field::Egid,
                Number::Inode => Field::Inode,
                Number::Mask => Field::AccessMask,
                Number::Protocol => Field::Protocol,
            };
            if matches!(field, Field::Inode) {
                (field, 4, 0, value.to_le_bytes().to_vec())
            } else {
                (
                    field,
                    if matches!(field, Field::AccessMask) {
                        5
                    } else {
                        3
                    },
                    0,
                    u32::try_from(*value)
                        .map_err(too_large)?
                        .to_le_bytes()
                        .to_vec(),
                )
            }
        }
        Condition::Device((major, minor)) => {
            let mut value = major.to_le_bytes().to_vec();
            value.extend(minor.to_le_bytes());
            (Field::Device, 6, 0, value)
        }
        Condition::Port(local, first, last) => {
            let mut value = first.to_le_bytes().to_vec();
            value.extend(last.to_le_bytes());
            (
                if *local {
                    Field::LocalPort
                } else {
                    Field::RemotePort
                },
                7,
                0,
                value,
            )
        }
        Condition::Ip(local, net) => {
            let mut value = vec![net.prefix_len()];
            match net.network() {
                std::net::IpAddr::V4(ip) => value.extend(ip.octets()),
                std::net::IpAddr::V6(ip) => value.extend(ip.octets()),
            };
            (
                if *local {
                    Field::LocalIp
                } else {
                    Field::RemoteIp
                },
                8,
                0,
                value,
            )
        }
    };
    if !caps.supports_condition(field) {
        return Err(Error::Unsupported(
            "native hook cannot supply a requested rule field",
        ));
    }
    out.extend((field as u16).to_le_bytes());
    out.push(operator);
    out.push(0);
    out.extend(u32::try_from(value.len()).map_err(too_large)?.to_le_bytes());
    out.extend(index.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(value);
    Ok(())
}

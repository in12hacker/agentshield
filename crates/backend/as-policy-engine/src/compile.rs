//! Load-time validation/compilation; matching never parses patterns or locks management state.
use crate::{Error, model::*};
use as_types::driver_control::DriverCapabilities;
use as_types::event_operation::{FileOperation, NetworkOperation, ProcessOperation};
use as_types::rule_execution::{BackendStage, RuleExecution};
use as_types::{
    RuleVersion, dispatch_event::*, main_ports::*, process_source::ProcessFactsPort,
    workload::Workload,
};
use globset::GlobBuilder;
use ipnet::IpNet;
use regex::bytes::Regex;
use std::{collections::HashSet, ffi::OsStr, net::IpAddr, os::unix::ffi::OsStrExt};

pub(crate) enum Text {
    Exact(Vec<u8>),
    Prefix(Vec<u8>),
    Glob(globset::GlobMatcher),
    Regex(Regex),
}
impl Text {
    fn compile(value: &TextMatch, driver: bool) -> Result<Self, Error> {
        Self::compile_case(value, driver, false)
    }
    fn compile_case(
        value: &TextMatch,
        driver: bool,
        case_insensitive: bool,
    ) -> Result<Self, Error> {
        Ok(match value {
            TextMatch::Exact(s) => Self::Exact(s.as_bytes().to_vec()),
            TextMatch::Prefix(s) => Self::Prefix(s.as_bytes().to_vec()),
            TextMatch::Glob(s) if !driver => Self::Glob(
                GlobBuilder::new(s)
                    .case_insensitive(case_insensitive)
                    .literal_separator(true)
                    .backslash_escape(true)
                    .build()
                    .map_err(|e| Error::Invalid(e.to_string()))?
                    .compile_matcher(),
            ),
            TextMatch::Regex(s) if !driver => Self::Regex(
                regex::bytes::RegexBuilder::new(s)
                    .case_insensitive(case_insensitive)
                    .unicode(false)
                    .build()
                    .map_err(|e| Error::Invalid(e.to_string()))?,
            ),
            _ => {
                return Err(Error::Invalid(
                    "driver patterns must be exact or prefix".into(),
                ));
            }
        })
    }
    fn matches(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Exact(v) => v == bytes,
            Self::Prefix(v) => bytes.starts_with(v),
            Self::Glob(v) => v.is_match(std::path::Path::new(OsStr::from_bytes(bytes))),
            Self::Regex(v) => v.is_match(bytes),
        }
    }
    fn matches_dns(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Exact(value) => value.eq_ignore_ascii_case(bytes),
            Self::Prefix(value) => bytes
                .get(..value.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(value)),
            _ => self.matches(bytes),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Bytes {
    Name,
    Exe,
    Cwd,
    ParentName,
    ParentExe,
    Image,
    Path,
    NewPath,
}
#[derive(Clone, Copy)]
pub(crate) enum Number {
    Pid,
    Ppid,
    Uid,
    Gid,
    Euid,
    Egid,
    Inode,
    Mask,
    Protocol,
}
pub(crate) enum Condition {
    MountEffect(as_types::dispatch_event::MountEffect),
    DnsDomain(Text),
    Bytes(Bytes, Text),
    Arg(bool, usize, Text),
    Number(Number, u64),
    Device((u32, u32)),
    Ip(bool, IpNet),
    Port(bool, u16, u16),
}
impl Condition {
    fn matches(&self, event: &EventData) -> Option<bool> {
        let p = &event.process;
        let process = match &event.payload {
            EventPayload::Process(v) => Some(v),
            _ => None,
        };
        let file = match &event.payload {
            EventPayload::File(v) => Some(v),
            _ => None,
        };
        let network = match &event.payload {
            EventPayload::Network(v) => Some(v),
            _ => None,
        };
        Some(match self {
            Self::MountEffect(expected) => file?.mount_effect? == *expected,
            Self::DnsDomain(pattern) => network.is_some_and(|network| {
                network.dns_snapshot.as_ref().is_some_and(|snapshot| {
                    matches!(
                        snapshot.analysis,
                        Some(as_types::dispatch_event::DnsQueryAnalysis::Question { .. })
                    )
                }) && network
                    .target_domain
                    .as_deref()
                    .is_some_and(|name| pattern.matches_dns(name))
            }),
            Self::Bytes(field, pattern) => {
                let v = match field {
                    Bytes::Name => p.name.as_deref(),
                    Bytes::Exe => p.executable.as_deref(),
                    Bytes::Cwd => p.cwd.as_deref(),
                    Bytes::ParentName => p.parent_name.as_deref(),
                    Bytes::ParentExe => p.parent_executable.as_deref(),
                    Bytes::Image => process?.target_executable.as_deref(),
                    Bytes::Path => file?.path.as_deref(),
                    Bytes::NewPath => file?.new_path.as_deref(),
                }?;
                pattern.matches(v)
            }
            Self::Arg(target, index, pattern) => {
                let args = if *target {
                    process?.target_argv.as_ref()?
                } else {
                    p.argv.as_ref()?
                };
                // A captured argv with no such position is a definite mismatch.
                args.get(*index).is_some_and(|v| pattern.matches(v))
            }
            Self::Number(field, expected) => {
                let actual = match field {
                    Number::Pid => u64::from(event.header().actor_tgid?),
                    Number::Ppid => u64::from(p.parent_pid?),
                    Number::Uid => u64::from(p.uid?),
                    Number::Gid => u64::from(p.gid?),
                    Number::Euid => u64::from(p.euid?),
                    Number::Egid => u64::from(p.egid?),
                    Number::Inode => file?.inode?,
                    Number::Mask => u64::from(file?.access_mask?),
                    Number::Protocol => u64::from(network?.protocol?),
                };
                if matches!(field, Number::Mask) {
                    actual & expected == *expected
                } else {
                    actual == *expected
                }
            }
            Self::Device(expected) => file?.device? == *expected,
            Self::Ip(local, expected) => {
                let actual: IpAddr = if *local {
                    network?.local_ip?
                } else {
                    network?.remote_ip?
                };
                expected.contains(&actual)
            }
            Self::Port(local, first, last) => {
                let port = if *local {
                    network?.local_port?
                } else {
                    network?.remote_port?
                };
                (*first..=*last).contains(&port)
            }
        })
    }
}

pub(crate) struct CompiledRule {
    pub hit: RuleHit,
    pub execution: RuleExecution,
    pub action: Action,
    pub(super) scope: Scope,
    pub(super) operations: Vec<u32>,
    pub(super) conditions: Vec<Condition>,
}
pub(crate) enum Match {
    Yes,
    No,
    Missing,
}
impl CompiledRule {
    pub fn matches(
        &self,
        event: &EventData,
        enriched: bool,
        source: Option<&dyn ProcessFactsPort>,
    ) -> Result<Match, ModuleError> {
        if !self.operations.contains(&event.header().operation) {
            return Ok(Match::No);
        }
        let mut missing = false;
        for condition in &self.conditions {
            match condition.matches(event) {
                Some(false) => return Ok(Match::No),
                Some(true) => {}
                None => missing = true,
            }
        }
        match &self.scope {
            Scope::AllProtected => {}
            Scope::Containers { instances } => match &event.process.workload {
                Some(Workload::Container(v)) if instances.contains(v) => {}
                Some(_) => return Ok(Match::No),
                None => {
                    let header = event.header();
                    if let (Some(source), Some(lineage), Some(observed)) = (
                        source,
                        header.actor_cgroup_lineage.as_deref(),
                        header.source_boottime_ns,
                    ) && source.outside_containers(instances, lineage, observed)
                    {
                        return Ok(Match::No);
                    }
                    missing = true;
                }
            },
        }
        if missing && enriched {
            return Err(ModuleError::Unavailable);
        }
        Ok(if missing { Match::Missing } else { Match::Yes })
    }
    pub fn choice(&self, decision: Decision, reason: ChoiceReason) -> RuleChoice {
        RuleChoice {
            decision,
            hit: Some(self.hit.clone()),
            reason,
        }
    }
}

pub(crate) struct Compiler {
    ids: HashSet<String>,
    groups: HashSet<String>,
    positions: HashSet<(u16, RuleExecution, u32)>,
    version: RuleVersion,
}
impl Compiler {
    pub fn new(version: RuleVersion) -> Self {
        Self {
            ids: HashSet::new(),
            groups: HashSet::new(),
            positions: HashSet::new(),
            version,
        }
    }
    pub fn group<T: Target, O: OperationCode>(
        &mut self,
        group: &Group<T, O>,
        kind: u16,
    ) -> Result<Vec<CompiledRule>, Error> {
        if kind == EventKind::File as u16 && group.execution.is_driver() && group.number.get() == 1
        {
            return Err(Error::Invalid(
                "file driver group 1 is reserved for configuration self-protection".into(),
            ));
        }
        if group.id.is_empty()
            || !self.groups.insert(group.id.clone())
            || !self
                .positions
                .insert((kind, group.execution, group.number.get()))
        {
            return Err(Error::Invalid(
                "empty/duplicate group identity or order".into(),
            ));
        }
        let mut positions = HashSet::new();
        let mut result = Vec::new();
        for rule in &group.rules {
            let mount = kind == EventKind::File as u16
                && rule
                    .operations
                    .iter()
                    .any(|op| op.code() == FileOperation::Mount as u32);
            if mount
                && (!group.execution.is_driver()
                    || !matches!(
                        rule.action,
                        Action::Allow
                            | Action::Deny {
                                terminate: Termination::None
                            }
                    ))
            {
                return Err(Error::Unsupported(
                    "mount maintenance requires direct Driver Allow or Deny",
                ));
            }
            if rule.operations.iter().any(|op| {
                !(op.rule_eligible() || mount && op.code() == FileOperation::Mount as u32)
            }) {
                if kind == EventKind::File as u16 {
                    return Err(Error::Unsupported(
                        "file operation lacks the complete control chain",
                    ));
                }
                return Err(Error::Invalid(
                    "observation-only operation cannot carry a protection rule".into(),
                ));
            }
            if rule.id.is_empty()
                || !self.ids.insert(rule.id.clone())
                || !positions.insert(rule.number)
                || rule.operations.is_empty()
            {
                return Err(Error::Invalid(
                    "empty/duplicate rule identity, order or operations".into(),
                ));
            }
            let driver = group.execution.is_driver();
            if !rule.deny_message.is_empty()
                && !matches!(rule.action, Action::Deny { .. } | Action::Ask { .. })
            {
                return Err(Error::Invalid(
                    "nonempty denial message requires Deny or Ask".into(),
                ));
            }
            match (&rule.action, group.execution) {
                (
                    Action::Ask { .. },
                    RuleExecution::Backend {
                        stage: BackendStage::Ask,
                    },
                )
                | (Action::ToUser, RuleExecution::Driver {}) => {}
                (Action::Allow | Action::Deny { .. }, execution) if !execution.is_ask() => {}
                _ => {
                    return Err(Error::Invalid(
                        "action is not valid in this rule stage".into(),
                    ));
                }
            }
            if let Scope::Containers { instances } = &rule.scope {
                let mut unique = HashSet::new();
                if instances.is_empty()
                    || instances.iter().any(|v| {
                        v.runtime.is_empty()
                            || v.id.is_empty()
                            || v.namespace.as_ref().is_some_and(String::is_empty)
                            || !unique.insert(v)
                    })
                {
                    return Err(Error::Invalid(
                        "container scope must contain distinct complete identities".into(),
                    ));
                }
            }
            let mut conditions = Vec::new();
            let p = &rule.source;
            for (field, value) in [
                (Bytes::Name, &p.name),
                (Bytes::Exe, &p.executable),
                (Bytes::Cwd, &p.cwd),
                (Bytes::ParentName, &p.parent_name),
                (Bytes::ParentExe, &p.parent_executable),
            ] {
                add_text(&mut conditions, field, value, driver)?;
            }
            for (field, value) in [
                (Number::Pid, p.pid),
                (Number::Ppid, p.ppid),
                (Number::Uid, p.uid),
                (Number::Gid, p.gid),
                (Number::Euid, p.euid),
                (Number::Egid, p.egid),
            ] {
                if let Some(v) = value {
                    conditions.push(Condition::Number(field, u64::from(v)));
                }
            }
            add_args(&mut conditions, false, &p.arguments, driver)?;
            let operations = rule
                .operations
                .iter()
                .map(OperationCode::code)
                .collect::<Vec<_>>();
            rule.target.compile(&mut conditions, driver, &operations)?;
            if group.enabled && rule.enabled {
                result.push(CompiledRule {
                    hit: RuleHit {
                        group_id: group.id.clone(),
                        group_number: group.number,
                        rule_id: rule.id.clone().into(),
                        rule_number: rule.number,
                        source: RuleSourceVersion::RulesDocument(self.version),
                        log: rule.log,
                        deny_message: Some(rule.deny_message.clone()),
                    },
                    execution: group.execution,
                    action: rule.action.clone(),
                    scope: rule.scope.clone(),
                    operations,
                    conditions,
                });
            }
        }
        Ok(result)
    }
}
fn add_text(
    out: &mut Vec<Condition>,
    field: Bytes,
    value: &Option<TextMatch>,
    driver: bool,
) -> Result<(), Error> {
    if let Some(value) = value {
        out.push(Condition::Bytes(field, Text::compile(value, driver)?));
    }
    Ok(())
}
fn add_args(
    out: &mut Vec<Condition>,
    target: bool,
    args: &[ArgumentMatch],
    driver: bool,
) -> Result<(), Error> {
    let mut indices = HashSet::new();
    for arg in args {
        if !indices.insert(arg.index) {
            return Err(Error::Invalid("duplicate argv position".into()));
        }
        out.push(Condition::Arg(
            target,
            arg.index,
            Text::compile(&arg.pattern, driver)?,
        ));
    }
    Ok(())
}
pub(crate) trait Target {
    fn compile(
        &self,
        out: &mut Vec<Condition>,
        driver: bool,
        operations: &[u32],
    ) -> Result<(), Error>;
}
impl Target for ProcessTarget {
    fn compile(&self, out: &mut Vec<Condition>, driver: bool, _: &[u32]) -> Result<(), Error> {
        add_text(out, Bytes::Image, &self.executable, driver)?;
        add_args(out, true, &self.arguments, driver)
    }
}
impl Target for FileTarget {
    fn compile(
        &self,
        out: &mut Vec<Condition>,
        driver: bool,
        operations: &[u32],
    ) -> Result<(), Error> {
        let mount = operations.contains(&(FileOperation::Mount as u32));
        if mount
            && (!driver
                || operations.len() != 1
                || self.mount_effect != Some(MountEffect::IndependentView))
        {
            return Err(Error::Unsupported(
                "mount rules require the proven independent_view target",
            ));
        }
        if let Some(effect) = self.mount_effect {
            if !mount {
                return Err(Error::Invalid(
                    "mount_effect requires a Mount operation".into(),
                ));
            }
            out.push(Condition::MountEffect(effect));
        }
        if self.new_path.is_some()
            && operations
                .iter()
                .any(|&op| op != FileOperation::Rename as u32)
            || self.access_mask.is_some()
                && operations
                    .iter()
                    .any(|&op| op != FileOperation::Open as u32)
            || self.inode.is_some()
                && operations.iter().any(|&op| {
                    op == FileOperation::CreateDirectory as u32
                        || op == FileOperation::CreateFile as u32
                })
        {
            return Err(Error::Invalid(
                "file target condition has no source for every selected operation".into(),
            ));
        }
        add_text(out, Bytes::Path, &self.path, driver)?;
        add_text(out, Bytes::NewPath, &self.new_path, driver)?;
        if let Some(v) = self.device {
            out.push(Condition::Device(v));
        }
        if let Some(v) = self.inode {
            out.push(Condition::Number(Number::Inode, v));
        }
        if let Some(v) = self.access_mask {
            if v == 0 {
                return Err(Error::Invalid("access mask must be nonzero".into()));
            }
            out.push(Condition::Number(Number::Mask, u64::from(v)));
        }
        Ok(())
    }
}
impl Target for NetworkTarget {
    fn compile(
        &self,
        out: &mut Vec<Condition>,
        driver: bool,
        operations: &[u32],
    ) -> Result<(), Error> {
        if let Some(domain) = &self.domain {
            if driver
                || operations
                    .iter()
                    .any(|&operation| operation != NetworkOperation::Send as u32)
            {
                return Err(Error::Unsupported(
                    "DNS domain rules require backend Send snapshots",
                ));
            }
            if self.protocol.is_some_and(|protocol| protocol != 17)
                || self
                    .remote_port
                    .as_ref()
                    .is_some_and(|port| !(port.first..=port.last).contains(&53))
            {
                return Err(Error::Invalid(
                    "DNS domain rules apply only to UDP destination port 53".into(),
                ));
            }
            // This is a necessary source boundary, not a second DNS matcher in the driver.
            out.push(Condition::Port(false, 53, 53));
            out.push(Condition::Number(Number::Protocol, 17));
            out.push(Condition::DnsDomain(Text::compile_case(
                domain, false, true,
            )?));
        }
        for (local, cidr) in [(true, &self.local_cidr), (false, &self.remote_cidr)] {
            if let Some(v) = cidr {
                out.push(Condition::Ip(
                    local,
                    v.parse()
                        .map_err(|_| Error::Invalid("invalid CIDR".into()))?,
                ));
            }
        }
        for (local, range) in [(true, &self.local_port), (false, &self.remote_port)] {
            if let Some(v) = range {
                if v.first > v.last {
                    return Err(Error::Invalid("reversed port range".into()));
                }
                out.push(Condition::Port(local, v.first, v.last));
            }
        }
        if let Some(v) = self.protocol {
            out.push(Condition::Number(Number::Protocol, u64::from(v)));
        }
        Ok(())
    }
}

pub(crate) trait OperationCode {
    fn code(&self) -> u32;
    fn rule_eligible(&self) -> bool;
}
impl OperationCode for ProcessOperation {
    fn code(&self) -> u32 {
        *self as u32
    }
    fn rule_eligible(&self) -> bool {
        matches!(self, Self::Execute)
    }
}
impl OperationCode for FileOperation {
    fn code(&self) -> u32 {
        *self as u32
    }
    fn rule_eligible(&self) -> bool {
        DriverCapabilities::controlled_operation_bit(EventKind::File, self.code()).is_some()
    }
}
impl OperationCode for NetworkOperation {
    fn code(&self) -> u32 {
        *self as u32
    }
    fn rule_eligible(&self) -> bool {
        true
    }
}

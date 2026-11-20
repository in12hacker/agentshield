//! Canonical persisted maintenance rules, compiled with the same ordinary policy.
use crate::{Error, model::*};
use as_types::{
    context::ConfigPaths,
    event_operation::{FileOperation, NetworkOperation, ProcessOperation},
    rule_execution::RuleExecution,
};
use std::num::NonZeroU32;

const ONE: NonZeroU32 = NonZeroU32::MIN;
const TWO: NonZeroU32 = ONE.saturating_add(1);
const PROCESS_GROUP: &str = "as-maintenance-process";
const FILE_GROUP: &str = "as-maintenance-file";
const NETWORK_GROUP: &str = "as-maintenance-network";

pub(crate) fn is_managed_group(id: &str) -> bool {
    matches!(id, PROCESS_GROUP | FILE_GROUP | NETWORK_GROUP)
}

fn rule<T, O>(
    id: &str,
    number: NonZeroU32,
    operations: Vec<O>,
    source: ProcessMatch,
    target: T,
) -> Rule<T, O> {
    Rule {
        id: id.into(),
        number,
        enabled: true,
        scope: Scope::AllProtected,
        operations,
        source,
        target,
        action: Action::Allow,
        log: false,
        deny_message: String::new(),
    }
}

pub(crate) fn identity(group: &RuleGroup) -> (&str, u16, NonZeroU32, bool) {
    match group {
        RuleGroup::Process(g) => (&g.id, 1, g.number, g.execution.is_driver()),
        RuleGroup::File(g) => (&g.id, 2, g.number, g.execution.is_driver()),
        RuleGroup::Network(g) => (&g.id, 3, g.number, g.execution.is_driver()),
    }
}

pub(crate) fn normalize(
    document: PolicyDocument,
    paths: &ConfigPaths,
) -> Result<PolicyDocument, Error> {
    let root = paths
        .self_protection_root()
        .to_str()
        .ok_or_else(|| Error::Invalid("maintenance root must be valid UTF-8".into()))?;
    let prefix = if root.ends_with('/') {
        root.to_owned()
    } else {
        format!("{root}/")
    };
    let source = || ProcessMatch {
        executable: Some(TextMatch::Prefix(prefix.clone())),
        ..ProcessMatch::default()
    };
    let mut canonical = vec![
        RuleGroup::Process(Group {
            id: PROCESS_GROUP.into(),
            number: ONE,
            enabled: true,
            execution: RuleExecution::Driver {},
            rules: vec![
                rule(
                    "as-maintenance-source-execute",
                    ONE,
                    vec![ProcessOperation::Execute],
                    source(),
                    ProcessTarget::default(),
                ),
                rule(
                    "as-maintenance-target-execute",
                    TWO,
                    vec![ProcessOperation::Execute],
                    ProcessMatch::default(),
                    ProcessTarget {
                        executable: Some(TextMatch::Prefix(prefix.clone())),
                        ..ProcessTarget::default()
                    },
                ),
            ],
        }),
        RuleGroup::File(Group {
            id: FILE_GROUP.into(),
            number: TWO,
            enabled: true,
            execution: RuleExecution::Driver {},
            rules: vec![
                rule(
                    "as-maintenance-source-open",
                    ONE,
                    vec![
                        FileOperation::Open,
                        FileOperation::CreateDirectory,
                        FileOperation::Unlink,
                        FileOperation::RemoveDirectory,
                        FileOperation::Rename,
                        FileOperation::TruncatePath,
                        FileOperation::TruncateOpenFile,
                        FileOperation::MapSharedWritable,
                    ],
                    source(),
                    FileTarget::default(),
                ),
                rule(
                    "as-maintenance-target-open",
                    TWO,
                    vec![FileOperation::Open],
                    ProcessMatch::default(),
                    FileTarget {
                        path: Some(TextMatch::Prefix(prefix.clone())),
                        ..FileTarget::default()
                    },
                ),
            ],
        }),
        RuleGroup::Network(Group {
            id: NETWORK_GROUP.into(),
            number: ONE,
            enabled: true,
            execution: RuleExecution::Driver {},
            rules: vec![rule(
                "as-maintenance-source-network",
                ONE,
                vec![NetworkOperation::Connect, NetworkOperation::Send],
                source(),
                NetworkTarget::default(),
            )],
        }),
    ];
    let mut present = [false; 3];
    let mut ordinary = Vec::new();
    for group in document.groups {
        let (id, kind, number, driver) = identity(&group);
        if let Some(index) = canonical.iter().position(|g| identity(g).0 == id) {
            if present[index] || canonical[index] != group {
                return Err(Error::Invalid(
                    "maintenance groups must retain their canonical content".into(),
                ));
            }
            present[index] = true;
        } else {
            if driver && ((kind == 2 && number == TWO) || (kind != 2 && number == ONE)) {
                return Err(Error::Invalid(
                    "driver group number is reserved for maintenance".into(),
                ));
            }
            ordinary.push(group);
        }
    }
    canonical.extend(ordinary);
    Ok(PolicyDocument { groups: canonical })
}

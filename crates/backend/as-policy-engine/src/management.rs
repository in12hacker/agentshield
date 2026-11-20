//! Single-item edits over the original immutable document and its one existing writer.
use crate::{Error, PolicyEngine, RuleWriteReceipt, maintenance, model::*};
use as_types::RuleVersion;

fn group_id(group: &RuleGroup) -> &str {
    maintenance::identity(group).0
}
fn rule_id(rule: &RuleDefinition) -> &str {
    match rule {
        RuleDefinition::Process(r) => &r.id,
        RuleDefinition::File(r) => &r.id,
        RuleDefinition::Network(r) => &r.id,
    }
}
fn find_rule(document: &PolicyDocument, id: &str) -> Option<(usize, RuleDefinition)> {
    document
        .groups
        .iter()
        .enumerate()
        .find_map(|(index, group)| {
            let rule = match group {
                RuleGroup::Process(g) => g
                    .rules
                    .iter()
                    .find(|r| r.id == id)
                    .cloned()
                    .map(RuleDefinition::Process),
                RuleGroup::File(g) => g
                    .rules
                    .iter()
                    .find(|r| r.id == id)
                    .cloned()
                    .map(RuleDefinition::File),
                RuleGroup::Network(g) => g
                    .rules
                    .iter()
                    .find(|r| r.id == id)
                    .cloned()
                    .map(RuleDefinition::Network),
            };
            rule.map(|rule| (index, rule))
        })
}
fn ordinary_group(group: &RuleGroup) -> Result<(), Error> {
    if maintenance::is_managed_group(group_id(group)) {
        return Err(Error::Invalid(
            "maintenance groups and rules cannot be edited individually".into(),
        ));
    }
    Ok(())
}
fn group_index(document: &PolicyDocument, id: &str) -> Result<usize, Error> {
    document
        .groups
        .iter()
        .position(|g| group_id(g) == id)
        .ok_or(Error::NotFound)
}
fn replace<T, O>(rules: &mut [Rule<T, O>], value: Rule<T, O>) -> Result<(), Error> {
    let original = rules
        .iter_mut()
        .find(|r| r.id == value.id)
        .ok_or(Error::NotFound)?;
    *original = value;
    Ok(())
}
fn apply(document: &mut PolicyDocument, change: RuleChange) -> Result<(), Error> {
    match change {
        RuleChange::CreateGroup { group } => {
            ordinary_group(&group)?;
            if document
                .groups
                .iter()
                .any(|g| group_id(g) == group_id(&group))
            {
                return Err(Error::Invalid("group identity already exists".into()));
            }
            document.groups.push(*group);
        }
        RuleChange::ReplaceGroup {
            group_id: id,
            group,
        } => {
            let index = group_index(document, &id)?;
            let original = &document.groups[index];
            ordinary_group(original)?;
            if group_id(&group) != id
                || std::mem::discriminant(original) != std::mem::discriminant(group.as_ref())
            {
                return Err(Error::Invalid(
                    "replacement group must retain identity and family".into(),
                ));
            }
            document.groups[index] = *group;
        }
        RuleChange::DeleteGroup { group_id: id } => {
            let index = group_index(document, &id)?;
            ordinary_group(&document.groups[index])?;
            document.groups.remove(index);
        }
        RuleChange::CreateRule { group_id: id, rule } => {
            let index = group_index(document, &id)?;
            ordinary_group(&document.groups[index])?;
            if find_rule(document, rule_id(&rule)).is_some() {
                return Err(Error::Invalid("rule identity already exists".into()));
            }
            match (&mut document.groups[index], *rule) {
                (RuleGroup::Process(g), RuleDefinition::Process(r)) => g.rules.push(r),
                (RuleGroup::File(g), RuleDefinition::File(r)) => g.rules.push(r),
                (RuleGroup::Network(g), RuleDefinition::Network(r)) => g.rules.push(r),
                _ => return Err(Error::Invalid("rule family must match its group".into())),
            }
        }
        RuleChange::ReplaceRule { rule_id: id, rule } => {
            let (index, _) = find_rule(document, &id).ok_or(Error::NotFound)?;
            ordinary_group(&document.groups[index])?;
            if rule_id(&rule) != id {
                return Err(Error::Invalid(
                    "replacement rule must retain identity".into(),
                ));
            }
            match (&mut document.groups[index], *rule) {
                (RuleGroup::Process(g), RuleDefinition::Process(r)) => replace(&mut g.rules, r)?,
                (RuleGroup::File(g), RuleDefinition::File(r)) => replace(&mut g.rules, r)?,
                (RuleGroup::Network(g), RuleDefinition::Network(r)) => replace(&mut g.rules, r)?,
                _ => return Err(Error::Invalid("rule family must match its group".into())),
            }
        }
        RuleChange::DeleteRule { rule_id: id } => {
            let (index, _) = find_rule(document, &id).ok_or(Error::NotFound)?;
            ordinary_group(&document.groups[index])?;
            match &mut document.groups[index] {
                RuleGroup::Process(g) => g.rules.retain(|r| r.id != id),
                RuleGroup::File(g) => g.rules.retain(|r| r.id != id),
                RuleGroup::Network(g) => g.rules.retain(|r| r.id != id),
            }
        }
    }
    Ok(())
}

impl PolicyEngine {
    /// Read one original rule and its group from the same immutable current version.
    pub fn get_rule(&self, id: &str) -> Result<RuleSnapshot, Error> {
        let snapshot = self.active.load_full();
        let (index, rule) = find_rule(&snapshot.document, id).ok_or(Error::NotFound)?;
        Ok(RuleSnapshot {
            version: snapshot.version,
            group_id: group_id(&snapshot.document.groups[index]).into(),
            rule,
        })
    }
    /// Edit the same-version document in management context, then use the sole atomic update path.
    /// The update rechecks both active and fixed-source versions and retains started save work.
    pub async fn edit(
        &self,
        expected: RuleVersion,
        change: RuleChange,
    ) -> Result<RuleWriteReceipt, Error> {
        let snapshot = self.active.load_full();
        if snapshot.version != expected {
            return Err(Error::Conflict);
        }
        let mut document = (*snapshot.document).clone();
        apply(&mut document, change)?;
        self.update(expected, document).await
    }
}

use crate::{
    Error, ProcessTree,
    model::*,
    store::{Node, State},
};
use as_types::{
    dispatch_event::EventData,
    driver::DirectHitCount,
    main_ports::{RuleHit, RuleSourceVersion},
    process_tree::*,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    ops::Bound,
};
impl ProcessTree {
    fn parent_view(
        &self,
        s: &State,
        key: ProcessKey,
        f: &as_types::dispatch_event::ProcessInfo,
    ) -> ParentLink {
        if f.parent_pid == Some(0) {
            ParentLink::Root
        } else if let Some(key) = State::parent(key, f) {
            ParentLink::Known {
                key,
                node_present: s.nodes.contains_key(&key),
            }
        } else {
            ParentLink::Unresolved {
                observed_pid: f.parent_pid,
            }
        }
    }
    /// Read an exact lifetime's immutable cached views without OS or driver I/O.
    pub fn get_cached_process(&self, key: ProcessKey) -> Result<ProcessDetails, Error> {
        let s = self.state.read().map_err(|_| Error::Unavailable)?;
        let n = s.nodes.get(&key).ok_or(Error::NotFound)?;
        Ok(ProcessDetails {
            evidence_epoch: s.gap_serial,
            record: n.record.clone(),
            initial_parent: self.parent_view(&s, key, &n.record.initial.process),
            latest_parent: self.parent_view(&s, key, &n.record.latest.process),
            children: s.children.get(&key).map_or(0, |v| v.len()),
        })
    }
    /// Read one existing lifetime, attempting one targeted refresh for missing source-backed fields.
    /// Failed supplementation returns the retained real views, without claiming a successful read.
    /// Only a new exec or an explicit refresh permits another attempt; source gaps do not retry I/O.
    pub async fn get_process(
        self: &std::sync::Arc<Self>,
        key: ProcessKey,
    ) -> Result<ProcessDetails, Error> {
        let needs_details = || {
            let s = self.state.read().map_err(|_| Error::Unavailable)?;
            let n = s.nodes.get(&key).ok_or(Error::NotFound)?;
            Ok::<_, Error>(n.needs_details())
        };
        if !needs_details()? {
            return self.get_cached_process(key);
        }
        let guard = self.scan.clone().lock_owned().await;
        let attempt = {
            let mut s = self.state.write().map_err(|_| Error::Unavailable)?;
            let n = s.nodes.get_mut(&key).ok_or(Error::NotFound)?;
            if n.needs_details() {
                n.detail_attempt = Some(crate::store::DetailAttempt::Pending);
                true
            } else {
                false
            }
        };
        if attempt {
            // The owned refresh survives this reader's cancellation. Its per-node
            // completion never opens another node's failed attempt for a retry.
            let _ = self
                .refresh_locked(RefreshTarget::Process { key }, guard, Some(key))
                .await;
        }
        self.get_cached_process(key)
    }
    /// Return a cache hint, never an operation capability.
    pub fn resolve_current_pid(&self, pid: u32) -> Result<Option<ProcessKey>, Error> {
        Ok(self
            .state
            .read()
            .map_err(|_| Error::Unavailable)?
            .pids
            .get(&pid)
            .copied())
    }
    /// Dispatch bounded queries; only Get may attempt an exact lifetime's missing details.
    pub async fn query(
        self: &std::sync::Arc<Self>,
        query: ProcessQuery,
    ) -> Result<QueryReply, Error> {
        match query {
            ProcessQuery::Status => self.status().map(QueryReply::Status),
            ProcessQuery::Get { key } => self.get_process(key).await.map(QueryReply::Details),
            ProcessQuery::ResolvePid { pid } => self.resolve_current_pid(pid).map(QueryReply::Pid),
            ProcessQuery::List { filter, page } => self
                .page("list", None, filter, page, None)
                .await
                .map(QueryReply::Page),
            ProcessQuery::Children { key, page } => {
                self.get_cached_process(key)?;
                self.page("children", Some(key), ProcessFilter::default(), page, None)
                    .await
                    .map(QueryReply::Page)
            }
            ProcessQuery::Subtree { key, page } => {
                let (descendants, _charge) = self.descendants(key).await?;
                self.page(
                    "subtree",
                    Some(key),
                    ProcessFilter::default(),
                    page,
                    Some(descendants),
                )
                .await
                .map(QueryReply::Page)
            }
            ProcessQuery::Ancestors { key, limit } => {
                if limit.get() > self.settings.query_page_size.get() {
                    return Err(Error::Invalid);
                }
                let mut next = Some(key);
                let mut seen = HashSet::new();
                let mut records = Vec::new();
                let mut incomplete = false;
                while let Some(k) = next {
                    let d = match self.get_cached_process(k) {
                        Ok(d) => d,
                        Err(Error::NotFound) => {
                            incomplete = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    };
                    next = match d.latest_parent {
                        ParentLink::Root => None,
                        ParentLink::Known { key, .. } => Some(key),
                        ParentLink::Unresolved { .. } => {
                            incomplete = true;
                            None
                        }
                    };
                    if k != key {
                        records.push(d.record)
                    }
                    if !seen.insert(k) {
                        return Err(Error::Invalid);
                    }
                    if records.len() >= limit.get() {
                        incomplete |= next.is_some();
                        break;
                    }
                }
                Ok(QueryReply::Ancestors {
                    records,
                    incomplete,
                })
            }
            ProcessQuery::Hits { key, page } => self.hits(key, page),
        }
    }
    async fn descendants(
        &self,
        key: ProcessKey,
    ) -> Result<(BTreeSet<ProcessKey>, std::sync::Arc<crate::model::Charge>), Error> {
        self.get_cached_process(key)?;
        let mut charge = self.budget.take(1024)?;
        std::sync::Arc::get_mut(&mut charge)
            .ok_or(Error::Unavailable)?
            .grow(128)?;
        let mut seen = BTreeSet::from([key]);
        let mut queue = VecDeque::from([key]);
        while let Some(k) = queue.pop_front() {
            let mut after = None;
            loop {
                let batch = {
                    let s = self.state.read().map_err(|_| Error::Unavailable)?;
                    if s.phase == TreePhase::Closed {
                        return Err(Error::Stopped);
                    }
                    s.children
                        .get(&k)
                        .into_iter()
                        .flat_map(|children| {
                            children.range((
                                after.map_or(Bound::Unbounded, Bound::Excluded),
                                Bound::Unbounded,
                            ))
                        })
                        .take(16)
                        .copied()
                        .collect::<Vec<_>>()
                };
                if batch.is_empty() {
                    break;
                }
                after = batch.last().copied();
                for child in batch {
                    if !seen.contains(&child) {
                        std::sync::Arc::get_mut(&mut charge)
                            .ok_or(Error::Unavailable)?
                            .grow(128)?;
                        seen.insert(child);
                        queue.push_back(child);
                    }
                }
                tokio::task::yield_now().await;
            }
            tokio::task::yield_now().await;
        }
        seen.remove(&key);
        Ok((seen, charge))
    }
    fn digest(
        &self,
        tag: &str,
        key: Option<ProcessKey>,
        filter: &ProcessFilter,
    ) -> Result<[u8; 32], Error> {
        Ok(
            Sha256::digest(serde_json::to_vec(&(tag, key, filter)).map_err(|_| Error::Invalid)?)
                .into(),
        )
    }
    fn cursor(&self, page: &PageRequest, digest: [u8; 32]) -> Result<Option<ProcessKey>, Error> {
        if page.limit.get() > self.settings.query_page_size.get() {
            return Err(Error::Invalid);
        }
        let s = self.state.read().map_err(|_| Error::Unavailable)?;
        if s.phase == TreePhase::Closed {
            return Err(Error::Stopped);
        }
        if let Some(c) = &page.cursor {
            if c.instance != self.instance || c.generation != s.generation || c.query != digest {
                return Err(Error::CursorExpired);
            }
            Ok(Some(c.after))
        } else {
            Ok(None)
        }
    }
    async fn page(
        &self,
        tag: &str,
        parent: Option<ProcessKey>,
        filter: ProcessFilter,
        page: PageRequest,
        subtree: Option<BTreeSet<ProcessKey>>,
    ) -> Result<ProcessPage, Error> {
        let digest = self.digest(tag, parent, &filter)?;
        let mut after = self.cursor(&page, digest)?;
        let mut found = Vec::new();
        loop {
            let (batch, checked) = {
                let s = self.state.read().map_err(|_| Error::Unavailable)?;
                let keys = if let Some(keys) = subtree.as_ref() {
                    Some(keys)
                } else if tag == "children" {
                    parent.and_then(|key| s.children.get(&key))
                } else {
                    Some(&s.ordered)
                };
                let checked_keys = keys
                    .into_iter()
                    .flat_map(|keys| {
                        keys.range((
                            after.map_or(Bound::Unbounded, Bound::Excluded),
                            Bound::Unbounded,
                        ))
                    })
                    .take(64)
                    .copied()
                    .collect::<Vec<_>>();
                let checked = checked_keys.last().copied();
                let batch = checked_keys
                    .iter()
                    .filter_map(|k| s.nodes.get(k).map(|n| n.record.clone()))
                    .collect::<Vec<_>>();
                (batch, checked)
            };
            if checked.is_none() {
                break;
            }
            for record in batch {
                let p = &record.latest.process;
                let selected = filter
                    .name
                    .as_ref()
                    .is_none_or(|v| p.name.as_ref() == Some(v))
                    && filter
                        .executable
                        .as_ref()
                        .is_none_or(|v| p.executable.as_ref() == Some(v))
                    && filter.lifecycle.is_none_or(|v| record.lifecycle == v)
                    && filter
                        .kind
                        .is_none_or(|v| record.latest.process.kind == Some(v))
                    && filter
                        .workload
                        .as_ref()
                        .is_none_or(|v| p.workload.as_ref() == Some(v))
                    && (tag != "children" || State::parent(record.key, p) == parent)
                    && subtree
                        .as_ref()
                        .is_none_or(|keys| keys.contains(&record.key));
                if selected {
                    found.push(record);
                    if found.len() > page.limit.get() {
                        break;
                    }
                }
            }
            if found.len() > page.limit.get() {
                break;
            }
            after = checked;
            tokio::task::yield_now().await;
        }
        let more = found.len() > page.limit.get();
        found.truncate(page.limit.get());
        let status = self.status()?;
        let next_cursor = if more {
            found.last().map(|r| ProcessCursor {
                instance: self.instance,
                generation: status.generation,
                query: digest,
                after: r.key,
                hit_after: None,
            })
        } else {
            None
        };
        Ok(ProcessPage {
            records: found,
            next_cursor,
            tree_revision: status.revision,
            status,
        })
    }
    fn hits(&self, key: ProcessKey, page: PageRequest) -> Result<QueryReply, Error> {
        let digest = self.digest("hits", Some(key), &ProcessFilter::default())?;
        self.cursor(&page, digest)?;
        let s = self.state.read().map_err(|_| Error::Unavailable)?;
        let node = s.nodes.get(&key).ok_or(Error::NotFound)?;
        let after = page.cursor.as_ref().and_then(|c| c.hit_after.as_ref());
        let mut items = node
            .hits
            .iter()
            .filter(|(k, _)| after.is_none_or(|a| *k > a))
            .take(page.limit.get() + 1)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        let more = items.len() > page.limit.get();
        items.truncate(page.limit.get());
        let cursor = if more {
            items.last().map(|(id, _)| ProcessCursor {
                instance: self.instance,
                generation: s.generation,
                query: digest,
                after: key,
                hit_after: Some(id.clone()),
            })
        } else {
            None
        };
        Ok(QueryReply::Hits {
            records: items.into_iter().map(|(_, v)| v).collect(),
            next_cursor: cursor,
            incomplete: s.epoch == 0 || s.gap || s.hit_gap,
        })
    }
    pub(crate) fn hit(&self, event: &EventData, hit: &RuleHit) -> Result<(), Error> {
        let h = event.header();
        let (Some(pid), Some(birth)) = (h.actor_tgid, h.actor_birth_boottime_ns) else {
            return Ok(());
        };
        let key = self.key(pid, birth);
        let event_key = EventKey::from(event);
        let (id, bytes) = hit_key(h.kind as u16, hit);
        let mut s = self.state.write().map_err(|_| Error::Unavailable)?;
        let Some(seen) = s.events.get_mut(&event_key) else {
            return Ok(());
        };
        if !seen.insert(id.clone()) {
            return Ok(());
        }
        let Some(node) = s.nodes.get_mut(&key) else {
            return Ok(());
        };
        merge_hit(
            node,
            &self.budget,
            id,
            bytes,
            RuleHitSummary {
                hit: hit.clone(),
                event_kind: h.kind as u16,
                count: 1,
                first: event_key,
                last: event_key,
            },
        )
    }
    pub(crate) fn direct_hits(&self, hits: Vec<DirectHitCount>) {
        let Ok(mut s) = self.state.write() else {
            return;
        };
        for count in hits {
            let applied = (|| {
                let (id, bytes) = hit_key(count.kind as u16, &count.hit);
                let key = self.key(count.host_tgid.get(), count.birth_boottime_ns);
                let node = s.nodes.get_mut(&key).ok_or(Error::NotFound)?;
                merge_hit(
                    node,
                    &self.budget,
                    id,
                    bytes,
                    RuleHitSummary {
                        hit: count.hit,
                        event_kind: count.kind as u16,
                        count: count.count.get(),
                        first: count.first,
                        last: count.last,
                    },
                )
            })();
            s.hit_gap |= applied.is_err();
        }
    }
}

fn hit_key(kind: u16, hit: &RuleHit) -> (String, usize) {
    let (source, version) = match hit.source {
        RuleSourceVersion::Configuration(version) => (1, version.0),
        RuleSourceVersion::RulesDocument(version) => (2, version.digest()),
    };
    let mut key = Sha256::new();
    key.update(kind.to_le_bytes());
    key.update([source]);
    key.update(version);
    key.update(hit.group_number.get().to_le_bytes());
    key.update(hit.rule_number.get().to_le_bytes());
    for id in [hit.group_id.as_str(), hit.rule_id.as_str()] {
        key.update((id.len() as u64).to_le_bytes());
        key.update(id.as_bytes());
    }
    let id = key.finalize().iter().map(|b| format!("{b:02x}")).collect();
    (id, hit.group_id.len() + hit.rule_id.as_str().len())
}

fn merge_hit(
    node: &mut Node,
    budget: &std::sync::Arc<Budget>,
    id: String,
    bytes: usize,
    increment: RuleHitSummary,
) -> Result<(), Error> {
    if let Some(total) = node.hits.get_mut(&id) {
        total.count = total
            .count
            .checked_add(increment.count)
            .ok_or(Error::ResourceLimit)?;
        total.first = total.first.min(increment.first);
        total.last = total.last.max(increment.last);
    } else {
        let charge = budget.take(1024 + bytes)?;
        node.hit_charges.push(charge);
        node.hits.insert(id, increment);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_leaf_subtree_only_charges_its_own_keys_with_a_large_unrelated_tree() {
        use std::sync::atomic::Ordering;
        let tree = ProcessTree::prepare(crate::tests::settings()).expect("tree");
        {
            let mut state = tree.state.write().expect("state");
            for pid in 1..=100 {
                let mut candidate = crate::tests::sample(pid, pid as u64, 10, Default::default());
                candidate.key = tree.key(pid, pid as u64);
                state.put(candidate, &tree.budget).expect("unrelated node");
            }
            state.phase = TreePhase::Ready;
        }
        let used = tree.budget.used.load(Ordering::Acquire);
        let _other = tree
            .budget
            .take(tree.budget.max - used - 2048)
            .expect("leave 2KiB for query");
        let before = tree.budget.used.load(Ordering::Acquire);
        let result = tree
            .query(ProcessQuery::Subtree {
                key: tree.key(1, 1),
                page: PageRequest {
                    limit: std::num::NonZeroUsize::MIN,
                    cursor: None,
                },
            })
            .await
            .expect("leaf needs no whole-system reservation");
        assert!(matches!(result,QueryReply::Page(ref page) if page.records.is_empty()));
        assert_eq!(tree.budget.used.load(Ordering::Acquire), before);
    }

    #[tokio::test]
    async fn subtree_yields_outside_the_read_lock_and_cancellation_releases_query_budget() {
        use std::{future::Future, sync::atomic::Ordering, task::Poll};
        let tree = ProcessTree::prepare(crate::tests::settings()).expect("tree");
        let root = tree.key(1, 1);
        {
            let mut state = tree.state.write().expect("state");
            for pid in 1..=100 {
                let facts = if pid == 1 {
                    Default::default()
                } else {
                    as_types::dispatch_event::ProcessInfo {
                        parent_pid: Some(1),
                        parent_birth_boottime_ns: Some(1),
                        ..Default::default()
                    }
                };
                let mut candidate = crate::tests::sample(pid, pid as u64, 10, facts);
                candidate.key = tree.key(pid, pid as u64);
                state.put(candidate, &tree.budget).expect("node");
            }
            state.phase = TreePhase::Ready;
        }
        let before = tree.budget.used.load(Ordering::Acquire);
        let mut query = Box::pin(tree.query(ProcessQuery::Subtree {
            key: root,
            page: PageRequest {
                limit: std::num::NonZeroUsize::MIN,
                cursor: None,
            },
        }));
        std::future::poll_fn(|cx| {
            assert!(matches!(query.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        assert!(
            tree.state.try_write().is_ok(),
            "cooperative wait must not hold the state read lock"
        );
        assert!(tree.budget.used.load(Ordering::Acquire) > before);
        drop(query);
        assert_eq!(tree.budget.used.load(Ordering::Acquire), before);
    }

    #[tokio::test]
    async fn a_retired_first_batch_does_not_hide_later_live_subtree_keys() {
        let tree = ProcessTree::prepare(crate::tests::settings()).expect("tree");
        let live = tree.key(100, 100);
        {
            let mut state = tree.state.write().expect("state");
            let mut candidate = crate::tests::sample(100, 100, 10, Default::default());
            candidate.key = live;
            state.put(candidate, &tree.budget).expect("live descendant");
            state.phase = TreePhase::Ready;
        }
        let mut keys = (1..=64)
            .map(|pid| tree.key(pid, pid as u64))
            .collect::<BTreeSet<_>>();
        keys.insert(live);
        let page = tree
            .page(
                "subtree",
                Some(tree.key(200, 200)),
                Default::default(),
                PageRequest {
                    limit: std::num::NonZeroUsize::MIN,
                    cursor: None,
                },
                Some(keys),
            )
            .await
            .expect("skip retired keys");
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].key, live);
        tree.close().await.expect("closed");
    }
}

import { selectionAnchor, type ActivityState } from './activity-state';
import { isRuleLog } from './activity-wire';
import type { ContextView } from './context-wire';
import { eventKey, sameEventKey, type EventKey } from './event-wire';
import { wireJson } from './wire-json';

export interface ActivityEvidenceSnapshot {
  schema: 'agentshield.activity-evidence';
  version: 1;
  event_key: EventKey;
  selection: { kind: 'log'; id: string; decision_id: readonly number[] | null }
    | { kind: 'completion'; decision_id: readonly number[] | null };
  view: ContextView;
}

// No stored export copy: borrow only the current successful original view.
export function selectedEvidenceSnapshot(activity: ActivityState, chatContent: boolean): ActivityEvidenceSnapshot | null {
  const { selection, evidence } = activity;
  if (selection?.kind === 'native_observation' || selection?.kind === 'original') return null;
  const anchor = selectionAnchor(selection);
  if (!chatContent || !activity.active || !selection || !anchor
    || evidence.phase !== 'returned' || evidence.pending !== null || evidence.problem !== null
    || evidence.result?.status !== 'completed' || evidence.result.data.type !== 'activity_context') return null;
  const view = evidence.result.data.view;
  if (view.anchor.kind !== 'event' || !sameEventKey(anchor, view.anchor.value)) return null;
  let identity: ActivityEvidenceSnapshot['selection'];
  if (selection.kind === 'log') {
    if (!isRuleLog(selection.row.record)) return null;
    identity = { kind: 'log', id: selection.row.id, decision_id: selection.row.record.decision_id };
  } else identity = { kind: 'completion', decision_id: selection.completion.decision_id };
  return { schema: 'agentshield.activity-evidence', version: 1, event_key: anchor, selection: identity, view };
}

export function evidenceFilename(anchor: EventKey): string {
  const key = eventKey(anchor);
  const source = key.source_instance.map(byte => byte.toString(16).padStart(2, '0')).join('');
  return `agentshield-event-${source}-${key.sequence_domain}-${key.sequence}.json`;
}

export function evidenceBlob(snapshot: ActivityEvidenceSnapshot): Blob {
  return new Blob([wireJson(snapshot)], { type: 'application/json' });
}

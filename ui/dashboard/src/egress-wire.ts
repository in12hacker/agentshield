// Read-only projection of the original ConfigSnapshot, never a second configuration store.
import { arrayOf, boolean, bytes, exact, integer, invalid, numericU64, oneOf, record, string } from './wire-json';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
import { decodeContainerInstance, decodeWorkload, type ContainerInstance, type Workload } from './workload-wire';
import { sameId } from './event-wire';
import { decodeSessionField, type SessionField } from './session-wire';

export interface ProviderRoute {
  readonly id: readonly number[]; readonly origin: string; readonly method: string; readonly path: string;
  readonly session: SessionField | null;
  readonly profile: 'open_ai_responses' | 'open_ai_chat_completions' | 'anthropic_messages';
}
export type CredentialSubject = { kind: 'process'; value: ProcessKey } | { kind: 'container'; value: ContainerInstance };
export interface CredentialBinding {
  readonly id: readonly number[]; readonly secret: readonly number[]; readonly enabled: boolean;
  readonly subjects: readonly CredentialSubject[]; readonly routes: readonly (readonly number[])[];
  readonly slot: 'authorization_bearer' | 'x_api_key';
}
export interface EgressConfig {
  bindings: readonly CredentialBinding[];
  flow: null | { enabled: boolean; mode: 'observe_tls' | 'mediate_tls'; ca: readonly number[] | null;
    routes: readonly ProviderRoute[]; scope: FlowScope };
}
export type FlowScope = { kind: 'all_protected' } | { kind: 'processes'; values: readonly ProcessKey[] }
  | { kind: 'workloads'; values: readonly Workload[] };
export function configuredSubjects(config: EgressConfig): readonly CredentialSubject[] {
  const scope = config.flow?.scope;
  if (scope?.kind === 'processes') return scope.values.map(value => ({ kind: 'process', value }));
  if (scope?.kind === 'workloads') return scope.values.flatMap(value => value.kind === 'container'
    ? [{ kind: 'container' as const, value: value.instance }] : []);
  return [];
}
function binding(value: unknown): CredentialBinding {
  const b = exact(value, ['id', 'secret', 'enabled', 'subjects', 'routes', 'slot']);
  bytes(b.id, 16); bytes(b.secret, 16); boolean(b.enabled);
  oneOf(b.slot, ['authorization_bearer', 'x_api_key'] as const);
  arrayOf(b.routes, item => bytes(item, 16));
  arrayOf(b.subjects, item => {
    const s = exact(item, ['kind', 'value']);
    if (s.kind === 'process') decodeProcessKey(s.value);
    else if (s.kind === 'container') decodeContainerInstance(s.value);
    else invalid();
  });
  return b as unknown as CredentialBinding;
}
function route(value: unknown): ProviderRoute {
  const r = exact(value, ['id', 'origin', 'method', 'path', 'profile', 'session']);
  bytes(r.id, 16); string(r.origin); string(r.method); string(r.path);
  oneOf(r.profile, ['open_ai_responses', 'open_ai_chat_completions', 'anthropic_messages'] as const);
  if (r.session !== null) decodeSessionField(r.session);
  return r as unknown as ProviderRoute;
}
export function readEgressConfig(snapshot: { readonly document: { readonly flow: unknown; readonly credentials: unknown } }): EgressConfig {
  return decodeEgressSettings(snapshot.document.flow, snapshot.document.credentials);
}
// The same wire shapes validate saved snapshots and configuration drafts.
export function decodeEgressSettings(flow: unknown, credentials: unknown): EgressConfig {
  const bindings = arrayOf(exact(credentials, ['bindings']).bindings, binding);
  if (flow === null) return { bindings, flow: null };
  const f = exact(flow, ['enabled', 'scope', 'mode', 'provider_routes', 'capture_content',
    'decode_limits', 'event_queue_bytes', 'egress']);
  const scope = record(f.scope);
  if (scope.kind === 'all_protected') exact(scope, ['kind']);
  else {
    exact(scope, ['kind', 'values']);
    if (scope.kind === 'workloads') arrayOf(scope.values, decodeWorkload);
    else if (scope.kind === 'processes') arrayOf(scope.values, decodeProcessKey);
    else invalid();
  }
  boolean(f.capture_content); numericU64(f.event_queue_bytes);
  const limits = exact(f.decode_limits, ['max_connection_buffer_bytes', 'max_message_bytes', 'max_active_exchanges', 'max_json_depth']);
  numericU64(limits.max_connection_buffer_bytes); numericU64(limits.max_message_bytes);
  integer(limits.max_active_exchanges, 0, 0xffffffff); integer(limits.max_json_depth, 0, 0xffffffff);
  let ca: readonly number[] | null = null;
  if (f.egress !== null) {
    const e = exact(f.egress, ['ca', 'max_connections', 'max_streams_per_connection']);
    ca = bytes(e.ca, 16);
    for (const field of ['max_connections', 'max_streams_per_connection']) {
      integer(e[field], 0, 0xffffffff);
    }
  }
  return { bindings, flow: { enabled: boolean(f.enabled), mode: oneOf(f.mode, ['observe_tls', 'mediate_tls'] as const),
    ca, routes: arrayOf(f.provider_routes, route), scope: f.scope as FlowScope } };
}
export function issuableBinding(config: EgressConfig, selected: CredentialBinding): boolean {
  return config.bindings.includes(selected) && selected.enabled && selected.subjects.length > 0
    && selected.routes.length > 0 && selected.routes.every(id => config.flow?.routes.some(r => sameId(r.id, id)));
}

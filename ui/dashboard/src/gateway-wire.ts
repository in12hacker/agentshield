// Common persisted Gateway provenance; Group and chat borrow the same decoder.
import { arrayOf, boolean, bytes, decimalU64, exact, integer, invalid, string } from './wire-json';
import { decodeProcessKey, type ProcessKey } from './processes-wire';
export interface GatewayReference { id: string; capture: readonly number[]; from_client: boolean; frames: readonly { offset: string; length: string }[] }
export interface GatewayFieldReference { message: GatewayReference; pointer: string }
export interface GatewayConnection { capture: readonly number[]; source: readonly number[]; generation: string; first_syn_ordinal: string;
  client_port: number; server_port: number; client: ProcessKey; server: ProcessKey; client_socket_cookie: string; server_socket_cookie: string }
export function rowId(value: unknown): string {
  const id = decimalU64(value); return id === '0' || BigInt(id) > 9223372036854775807n ? invalid() : id;
}
export function gatewayReference(value: unknown): GatewayReference {
  const o = exact(value, ['id', 'capture', 'from_client', 'frames']); rowId(o.id); bytes(o.capture, 32); boolean(o.from_client);
  const frames = arrayOf(o.frames, value => { const f = exact(value, ['offset', 'length']); decimalU64(f.offset);
    if (decimalU64(f.length) === '0' || BigInt(f.offset as string) + BigInt(f.length as string) > 18446744073709551615n) invalid(); return f; });
  if (!frames.length || frames.some((f, i) => i > 0 && BigInt(f.offset as string) < BigInt(frames[i - 1]!.offset as string) + BigInt(frames[i - 1]!.length as string))) invalid();
  return o as unknown as GatewayReference;
}
export function gatewayField(value: unknown): GatewayFieldReference {
  const o = exact(value, ['message', 'pointer']); gatewayReference(o.message); const p = string(o.pointer);
  if (p !== '' && (!p.startsWith('/') || /~(?![01])/u.test(p))) invalid(); return o as unknown as GatewayFieldReference;
}
export function gatewayConnection(value: unknown): GatewayConnection {
  const o = exact(value, ['capture', 'source', 'generation', 'first_syn_ordinal', 'client_port', 'server_port', 'client', 'server', 'client_socket_cookie', 'server_socket_cookie']);
  bytes(o.capture, 32); bytes(o.source, 16); decimalU64(o.generation); decimalU64(o.first_syn_ordinal);
  integer(o.client_port, 1, 65535); integer(o.server_port, 1, 65535); if (o.client_port === o.server_port) invalid();
  decodeProcessKey(o.client); decodeProcessKey(o.server);
  if (decimalU64(o.client_socket_cookie) === '0' || decimalU64(o.server_socket_cookie) === '0') invalid();
  return o as unknown as GatewayConnection;
}

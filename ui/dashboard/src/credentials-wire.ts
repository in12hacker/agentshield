// Frozen #452 430992c86 transport.rs PrivateCommand/encode and as-types/flow.rs.
import { arrayOf, bytes, decimalU64, exact, integer, invalid, oneOf, record, string } from './wire-json';
import { VERSION, type CommandId } from './request-wire';

export const MAX_SECRET_BYTES = 16 * 1024;
export type PrivateOperation = { type: 'ensure_ca' } | { type: 'public_ca'; ca: readonly number[] }
  | { type: 'import_secret' } | { type: 'replace_secret'; secret: readonly number[]; expected_revision: string }
  | { type: 'issue_phantom'; binding: readonly number[]; expected_config: readonly number[] }
  | { type: 'revoke_phantom'; token: readonly number[] };
export interface CaInfo { id: readonly number[]; certificate_der: readonly number[]; fingerprint_sha256: readonly number[] }
export interface SecretReceipt { secret: readonly number[]; revision: string }
export type PrivateData = { type: 'ca'; info: CaInfo } | { type: 'secret_saved'; receipt: SecretReceipt }
  | { type: 'phantom_issued'; token_id: readonly number[]; binding: readonly number[]; revision: string; value: string }
  | { type: 'phantom_revoked'; revision: string };
export const credentialErrors = ['busy', 'invalid_token', 'revoked', 'scope_denied', 'origin_denied',
  'invalid_header', 'secret_unavailable', 'stopped', 'internal_failure', 'persistence_uncertain'] as const;
export type CredentialError = typeof credentialErrors[number];
export type PrivateResult = { status: 'failed'; code: CredentialError } | { status: 'completed'; data: PrivateData };
export interface PrivateReply { id: CommandId; result: PrivateResult }

export function decodePrivateReply(value: unknown): PrivateReply {
  const reply = exact(value, ['id', 'result']), result = record(reply.result);
  const id = bytes(reply.id, 16);
  if (result.status === 'failed') {
    exact(result, ['status', 'code']);
    return { id, result: { status: 'failed', code: oneOf(result.code, credentialErrors) } };
  }
  exact(result, ['status', 'data']);
  if (result.status !== 'completed') return invalid();
  const d = record(result.data);
  let data: PrivateData;
  if (d.type === 'ca') {
    exact(d, ['type', 'info']);
    const info = exact(d.info, ['id', 'certificate_der', 'fingerprint_sha256']);
    data = { type: 'ca', info: { id: bytes(info.id, 16),
      certificate_der: arrayOf(info.certificate_der, byte => integer(byte, 0, 255)),
      fingerprint_sha256: bytes(info.fingerprint_sha256, 32) } };
  } else if (d.type === 'secret_saved') {
    exact(d, ['type', 'receipt']);
    const r = exact(d.receipt, ['secret', 'revision']);
    data = { type: 'secret_saved', receipt: { secret: bytes(r.secret, 16), revision: decimalU64(r.revision) } };
  } else if (d.type === 'phantom_issued') {
    exact(d, ['type', 'token_id', 'binding', 'revision', 'value']);
    data = { type: 'phantom_issued', token_id: bytes(d.token_id, 16), binding: bytes(d.binding, 16),
      revision: decimalU64(d.revision), value: string(d.value) };
  } else {
    exact(d, ['type', 'revision']);
    if (d.type !== 'phantom_revoked') return invalid();
    data = { type: 'phantom_revoked', revision: decimalU64(d.revision) };
  }
  return { id, result: { status: 'completed', data } };
}

export function validSecret(value: string): boolean {
  return value.length > 0 && !/[\r\n\0]/.test(value) && new TextEncoder().encode(value).length <= MAX_SECRET_BYTES;
}
export function decodePrivateOperation(value: unknown): PrivateOperation {
  const op = record(value);
  switch (op.type) {
    case 'ensure_ca': case 'import_secret': exact(op, ['type']); break;
    case 'public_ca': exact(op, ['type', 'ca']); bytes(op.ca, 16); break;
    case 'replace_secret': exact(op, ['type', 'secret', 'expected_revision']);
      bytes(op.secret, 16); decimalU64(op.expected_revision); break;
    case 'issue_phantom': exact(op, ['type', 'binding', 'expected_config']);
      bytes(op.binding, 16); bytes(op.expected_config, 32); break;
    case 'revoke_phantom': exact(op, ['type', 'token']); bytes(op.token, 16); break;
    default: return invalid();
  }
  return op as unknown as PrivateOperation;
}
// value is only a synchronous encoder argument, never command metadata.
export function privateCommand(id: CommandId, operation: PrivateOperation, value?: string): string {
  bytes(id, 16);
  decodePrivateOperation(operation);
  const secret = operation.type === 'import_secret' || operation.type === 'replace_secret';
  if (secret ? value === undefined || !validSecret(value) : value !== undefined) return invalid();
  return JSON.stringify({ version: VERSION, private: { id, command: secret ? { ...operation, value } : operation } });
}
export function parseReference(value: string): readonly number[] {
  if (!/^[0-9a-fA-F]{32}$/.test(value)) return invalid();
  return Array.from({ length: 16 }, (_, index) => Number.parseInt(value.slice(index * 2, index * 2 + 2), 16));
}

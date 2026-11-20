import { bytes, wireJson } from './wire-json';

export const VERSION = 2;
export type CommandId = readonly number[];

export function managementCommand(id: CommandId, type: 'rules' | 'processes' | 'security_log' | 'configuration' | 'control', command: unknown) {
  return wireJson({ version: VERSION, message: { type, id: bytes(id, 16), command } });
}

export function businessCommand<Query extends { type: string }>(id: CommandId, query: Query) {
  return wireJson({ version: VERSION, business: { id: bytes(id, 16), query } });
}

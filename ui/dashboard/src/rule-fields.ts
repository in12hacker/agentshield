import type { MessageKey } from './messages';
import type { RuleKind } from './rules-wire';

export const ruleCategories = { process: 'category.process', file: 'category.file', network: 'category.network' } as const;
export const ruleStages = { driver: 'rules.stageDriver', fast: 'rules.stageFast', ask: 'rules.stageAsk', remembered: 'rules.stageRemembered' } as const;
export const ruleActions = { allow: 'rules.actionAllow', deny: 'rules.actionDeny', ask: 'rules.stageAsk', to_user: 'rules.actionToUser' } as const;
export const terminationLabels = { none: 'rules.blockOnly', actor: 'choice.terminateActor', actor_and_parent: 'choice.terminateParent' } as const;
export const matchLabels = { exact: 'rules.matchExact', prefix: 'rules.matchPrefix', glob: 'rules.matchGlob', regex: 'rules.matchRegex' } as const;

// Author choices only. Mount has direct Driver rule support; other File entries follow controlled_operation_bit.
// the complete known protocol operations remain in rules-wire.ts for lossless reads.
export const editableRuleOperations = {
  process: ['execute'],
  file: ['open', 'create_file', 'create_directory', 'unlink', 'remove_directory', 'rename',
    'truncate_open_file', 'truncate_path', 'map_shared_writable', 'mount'],
  network: ['connect', 'send'],
} as const satisfies Record<RuleKind, readonly string[]>;

const fileOperations = {
  open: 'ask.open', read: 'ask.readFile', write: 'ask.writeFile', create_file: 'ask.createFile',
  create_directory: 'ask.createDirectory', unlink: 'ask.unlink', remove_directory: 'ask.removeDirectory',
  link: 'ask.link', rename: 'ask.rename', truncate_open_file: 'rules.truncateOpenFile', change_mode: 'ask.changeMode',
  change_owner: 'ask.changeOwner', grant_writable_mapping: 'ask.writableMapping', mount: 'ask.mount',
  unmount: 'ask.unmount', move_mount: 'ask.moveMount', set_metadata: 'ask.setMetadata',
  truncate_path: 'rules.truncateByPath', map_shared_writable: 'ask.mapSharedWritable',
} as const;
const operationLabels: Record<RuleKind, Readonly<Record<string, MessageKey>>> = {
  file: fileOperations, process: { execute: 'ask.execute', create: 'ask.createProcess', exit: 'ask.exitProcess' },
  network: { connect: 'ask.connect', send: 'ask.send' },
};
export function ruleOperation(kind: RuleKind, operation: string): MessageKey {
  return operationLabels[kind][operation]!;
}

export const fileAccessLabels = { 1: 'rules.accessRead', 2: 'rules.accessWrite', 3: 'rules.accessReadWrite' } as const;

export type RuleField = readonly [key: string, label: MessageKey, format: 'match' | 'text' | 'number' | 'u64' | 'access' | 'ports' | 'device' | 'arguments' | 'mount_effect'];
export const actorFields = [
  ['name', 'rules.fieldName', 'match'], ['executable', 'rules.fieldExecutable', 'match'],
  ['arguments', 'rules.fieldArguments', 'arguments'], ['cwd', 'rules.fieldWorkingDirectory', 'match'],
  ['pid', 'rules.fieldPid', 'number'], ['ppid', 'rules.fieldParentPid', 'number'],
  ['parent_name', 'rules.fieldParentName', 'match'], ['parent_executable', 'rules.fieldParentExecutable', 'match'],
  ['uid', 'rules.fieldUid', 'number'], ['gid', 'rules.fieldGid', 'number'],
  ['euid', 'rules.fieldEuid', 'number'], ['egid', 'rules.fieldEgid', 'number'],
] as const satisfies readonly RuleField[];
export const targetFields = {
  process: [actorFields[1], actorFields[2]],
  file: [['path', 'rules.fieldPath', 'match'], ['new_path', 'rules.fieldNewPath', 'match'],
    ['device', 'rules.fieldDevice', 'device'], ['inode', 'rules.fieldInode', 'u64'], ['access_mask', 'rules.fieldAccess', 'access'],
    ['mount_effect', 'rules.fieldMountEffect', 'mount_effect']],
  network: [['domain', 'rules.fieldDomain', 'match'], ['remote_cidr', 'rules.fieldRemoteNetwork', 'text'],
    ['remote_port', 'rules.fieldRemotePort', 'ports'], ['local_cidr', 'rules.fieldLocalNetwork', 'text'],
    ['local_port', 'rules.fieldLocalPort', 'ports'], ['protocol', 'rules.fieldProtocol', 'number']],
} as const satisfies Record<RuleKind, readonly RuleField[]>;

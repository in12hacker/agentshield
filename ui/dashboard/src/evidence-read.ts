import type { Message } from './messages';
import type { CommandId } from './request-wire';
import type { ContextResult } from './reply-wire';
import type { AttributionRequest } from './context-wire';
import { sameJsonValue } from './global/json-value';

// Ask／Activity／聊天共享客户端读取槽；身份校验仍归各自消费者。
export interface EvidenceRead<Result = ContextResult> {
  phase: 'initial' | 'loading' | 'cancelled' | 'returned' | 'failed';
  pending: CommandId | null;
  attributionRequest: AttributionRequest | null;
  result: Result | null;
  problem: Message | null;
}
export const emptyEvidence = <Result = ContextResult>(): EvidenceRead<Result> =>
  ({ phase: 'initial', pending: null, attributionRequest: null, result: null, problem: null });
export const loadingEvidence = <Result extends { status: string } = ContextResult>(pending: CommandId,
  attributionRequest: AttributionRequest | null = null, previous?: EvidenceRead<Result>): EvidenceRead<Result> =>
  ({ phase: 'loading', pending, attributionRequest,
    result: previous?.result?.status === 'completed' && previous.problem === null
      && sameJsonValue(previous.attributionRequest, attributionRequest) ? previous.result : null,
    problem: null });

// 仅放弃本次回执消费，不发送取消 SQL／重读命令。
export function cancelEvidence<Result>(read: EvidenceRead<Result>): EvidenceRead<Result> | null {
  return read.phase === 'loading' && read.pending ? { ...emptyEvidence<Result>(), phase: 'cancelled' } : null;
}

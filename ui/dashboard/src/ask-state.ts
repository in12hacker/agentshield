import type { AskContextResult, BusinessReply } from './reply-wire';
import { sameEventKey, sameId, type Ask, type OperationResult } from './event-wire';
import { message } from './messages';
import { emptyEvidence, type EvidenceRead } from './evidence-read';
import { matchesAttribution } from './context-wire';

export interface Review {
  ask: Ask;
  observedAt: number;
  attempted: boolean;
  phase: 'waiting' | 'sending' | 'sent' | 'send_failed' | 'complete';
  completion: OperationResult | null;
  completionOrder: number | null;
}
export interface AskEvidence extends EvidenceRead<AskContextResult> {
  ask: Ask | null;
}
export const emptyAskEvidence = (ask: Ask | null = null): AskEvidence =>
  ({ ask, ...emptyEvidence<AskContextResult>() });

export function reviewRemaining(review: Review, at: number) {
  return Math.max(0, review.ask.remaining_ms - Math.max(0, at - review.observedAt));
}

export function reviewEligible(review: Review, at: number): boolean {
  return !review.attempted && !review.completion && reviewRemaining(review, at) > 0;
}

// 数组顺序就是实际收取顺序。没有下一待答时保留原项的回执和焦点。
export function selectCurrentReview(reviews: readonly Review[], selected: Ask | null, at: number): Review | undefined {
  const current = reviews.find(review => review.ask === selected);
  return current && reviewEligible(current, at) ? current
    : reviews.find(review => reviewEligible(review, at)) ?? current ?? reviews[0];
}

export function retainRecentReviews(reviews: readonly Review[]): readonly Review[] {
  const finishedLimit = 16;
  let finished = 0;
  let earliest: Review | undefined;
  for (const review of reviews) {
    if (review.completionOrder === null) continue;
    finished++;
    if (!earliest || review.completionOrder < earliest.completionOrder!) earliest = review;
  }
  return finished > finishedLimit ? reviews.filter(review => review !== earliest) : reviews;
}

export function receiveAskContext(read: AskEvidence, { id, result }: BusinessReply): AskEvidence | null {
  const ask = read.ask;
  if (!ask || !read.pending || !sameId(read.pending, id)) return null;
  if (result.status === 'failed') return { ...read, phase: 'returned', pending: null, result };
  if (result.data.type === 'ask_context') {
    const context = result.data.context;
    if (sameId(context.decision_id, ask.decision_id) && sameEventKey(context.event, ask.event.header)
      && context.view.anchor.kind === 'question' && sameId(context.view.anchor.value, ask.decision_id)
      && matchesAttribution(context.attribution, read.attributionRequest, context.event)) {
      return { ...read, phase: 'returned', pending: null, result: { status: 'completed', data: result.data } };
    }
  }
  return { ...emptyAskEvidence(ask), phase: 'failed', problem: message('ask.wrongReply') };
}

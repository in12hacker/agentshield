import { memo } from 'react';
import type { ChatContent, ChatMessage, ChatPart, ChatMessageEntry } from './chat-wire';
import { factsJson } from './wire-json';
import { sameJsonValue } from './global/json-value';
import { RawFacts } from './global/raw-facts';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';

const roleKeys = { system: 'chat.roleSystem', developer: 'chat.roleDeveloper', user: 'chat.roleUser',
  assistant: 'chat.roleAssistant', tool: 'chat.roleTool', other: 'chat.roleOther' } satisfies Record<ChatMessage['role']['kind'], MessageKey>;
const partKeys = { text: 'chat.text', tool_call: 'chat.toolCall', tool_result: 'chat.toolResult',
  opaque: 'chat.opaque', media_ref: 'chat.media' } satisfies Record<ChatPart['type'], MessageKey>;
// Shared verbatim ChatContent rendering for original messages and chain result parts.
export function ChatContentValue({ value }: { value: ChatContent }) {
  return <pre className="chat-body">{value.kind === 'text' ? value.value : factsJson(value.value)}</pre>;
}
function Part({ part }: { part: ChatPart }) {
  const { t } = useI18n();
  return <section className="chat-part stack-tight">
    <h4>{t(partKeys[part.type])} · <code>{part.id}</code></h4>
    {part.type === 'text' && <><p className="muted"><code>{part.kind}</code></p><pre className="chat-body">{part.text}</pre></>}
    {part.type === 'tool_call' && <>
      <p>{t('chat.toolName')} <code>{JSON.stringify(part.name)}</code></p>
      {part.arguments === null ? <p className="amber">{t('chat.argumentsMissing')}</p> : <ChatContentValue value={part.arguments} />}
    </>}
    {part.type === 'tool_result' && <ChatContentValue value={part.content} />}
    {(part.type === 'tool_call' || part.type === 'tool_result') && <RawFacts summaryKey="chat.nativeId" value={part.native_id} />}
    {part.type === 'opaque' && <p><code>{JSON.stringify(part.provider_type)} · {JSON.stringify(part.reason)}</code></p>}
    {part.type === 'media_ref' && <p><code>{part.kind}</code> · {part.safe_label}</p>}
  </section>;
}
export const OriginalChatMessage = memo(function OriginalChatMessage({ message }: { message: ChatMessage }) {
  const { t } = useI18n();
  return <article className="chat-message stack-tight">
    <h3>{t(roleKeys[message.role.kind])}{message.role.kind === 'other' && <> · <code>{JSON.stringify(message.role.value)}</code></>}</h3>
    <p className="overview-note">{t('chat.messageRevision', { sequence: message.id.sequence, revision: message.revision })}</p>
    <dl className="process-fields">
      <div><dt>{t('chat.origin')}</dt><dd><code>{message.origin}</code></dd></div>
      <div><dt>{t('chat.state')}</dt><dd><code>{message.state}</code></dd></div>
      <div><dt>{t('chat.quality')}</dt><dd><code>{factsJson(message.quality)}</code></dd></div>
    </dl>
    {message.observed_parts.length === 0 && <p className="muted">{t('chat.noParts')}</p>}
    {message.observed_parts.map(part => <Part key={part.id} part={part} />)}
    <RawFacts summaryKey="chat.originalMessage" value={message} />
  </article>;
});

export function ChatHistoryMessage({ entry }: { entry: ChatMessageEntry }) {
  const { t } = useI18n();
  if (entry.kind === 'model_http') return <OriginalChatMessage message={entry.message} />;
  const displayed = (observation: (typeof entry.observations)[number]) => ({
    content: observation.content,
    role: observation.role === null ? null : observation.role.kind === 'json'
      ? { kind: 'json', value: observation.role.value } : { kind: 'role', value: observation.role.value },
  });
  const first = entry.observations[0];
  const visible = first && entry.observations.every(observation => sameJsonValue(displayed(first), displayed(observation)))
    ? [first] : entry.observations;
  return <article className="chat-message stack-tight">
    <h3>{t('chat.nativeGateway')}</h3>
    <p className="overview-note">{t(entry.order.kind === 'session_sequence' ? 'chat.sessionOrder' : entry.order.kind === 'history_window' ? 'chat.windowOrder' : 'chat.unresolvedOrder')}
      {entry.order.kind === 'session_sequence' && <> · <code>{entry.order.sequence.value}</code></>}
      {entry.order.kind === 'history_window' && <> · <code>{entry.order.window.message.id}:{entry.order.window.pointer} [{entry.order.index}]</code></>}</p>
    <RawFacts summaryKey="chat.nativeId" value={{ reference: entry.reference, native_message_id: entry.native_message_id, order: entry.order }} />
    {visible.length > 1 && <p className="amber">{t('chat.sourceContentDiffers')}</p>}
    {visible.map(observation => <section className="chat-part stack-tight" key={`${observation.reference.message.id}:${observation.reference.pointer}`}>
      <h4>{observation.role === null ? t('chat.roleMissing') : observation.role.kind === 'json' ? t('chat.roleOther') : t(roleKeys[observation.role.value.kind])}</h4>
      {observation.content === null ? <p className="amber">{t('chat.contentMissing')}</p> : <ChatContentValue value={observation.content} />}
    </section>)}
    <p className="overview-note">{t('chat.observationCount', { count: entry.observations.length })}</p>
    <RawFacts summaryKey="chat.originalMessage" value={entry.observations} />
    {entry.gaps.length > 0 && <RawFacts summaryKey="chat.observationGaps" value={entry.gaps} />}
  </article>;
}

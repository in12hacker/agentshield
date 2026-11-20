import { useState } from 'react';
import { useI18n } from './i18n';
import type { ChatExcerpt } from './intent-conversation';
import type { MessageKey } from './messages';

export function ConversationExcerpt({ message, labelKey, limit = 84 }: { message: ChatExcerpt; labelKey?: MessageKey; limit?: number }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const text = message.parts.join('\n');
  const characters = Array.from(text);
  const truncated = characters.length > limit;
  return <li className="conversation-excerpt">
    <strong>{t(labelKey ?? (message.role === 'user' ? 'ask.chatUser' : 'ask.chatAssistant'))}</strong>
    <p dir="auto">{open || !truncated ? text : characters.slice(0, limit).join('') + '…'}</p>
    {truncated && <button type="button" aria-expanded={open} onClick={() => setOpen(value => !value)}>
      {t(open ? 'ask.collapseMessage' : 'ask.expandMessage')}
    </button>}
  </li>;
}

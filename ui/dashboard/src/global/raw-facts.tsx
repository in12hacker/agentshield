import { useMemo, useState } from 'react';
import { useI18n } from '../i18n';
import type { MessageKey } from '../messages';
import { factsJson } from '../wire-json';

export function RawFacts({ value, summaryKey, format = 'json' }: {
  value: unknown; summaryKey: MessageKey; format?: 'json' | 'text';
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const text = useMemo(() => open ? format === 'text' && typeof value === 'string' ? value : factsJson(value) : null,
    [open, value, format]);
  return <details onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t(summaryKey)}</summary>{open && <pre>{text}</pre>}
  </details>;
}

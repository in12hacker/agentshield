import type { ReactNode } from 'react';
import { useI18n } from '../i18n';
import type { MessageKey } from '../messages';

export function DetailField({ labelKey, children }: { labelKey: MessageKey; children: ReactNode }) {
  const { t } = useI18n();
  return <div><dt>{t(labelKey)}</dt><dd>{children}</dd></div>;
}

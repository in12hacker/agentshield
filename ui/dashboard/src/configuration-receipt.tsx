import type { SettingsState } from './settings-state';
import type { CommandResult } from './reply-wire';
import { useI18n } from './i18n';
import { RawFacts } from './global/raw-facts';

export function ConfigurationFailure({ result }: { result: CommandResult | null }) {
  const { t } = useI18n();
  return result?.status === 'failed' ? <div className="receipt-failure" role="status">
    <p>{t('common.failureCode')}<code>{result.code}</code></p>
    <p>{t('common.originalMessage')}<code>{JSON.stringify(result.message)}</code></p>
  </div> : null;
}
export function ConfigurationReadStatus({ settings }: { settings: SettingsState }) {
  const { t } = useI18n();
  const ready = settings.phase === 'ready' && !settings.pending;
  return <>
    <p role="status" className="overview-note">{t(settings.pending && settings.pending.operation !== 'get' ? 'settings.writing'
      : settings.phase === 'loading' ? settings.readReply?.result.status === 'accepted' ? 'receipt.acceptedNote' : 'settings.reading'
      : ready ? 'settings.readComplete' : settings.phase === 'failed' ? 'settings.readFailed' : 'settings.notRead')}</p>
    {settings.conflict && <p className="amber" role="status">{t('settings.conflict')}</p>}
    {settings.problem && <p className="error" role="status">{t(settings.problem.key, settings.problem.params)}</p>}
    <ConfigurationFailure result={settings.readReply?.result ?? null} />
    {settings.snapshot && !ready && <p className="amber">{t('settings.lastRead')}</p>}
    {settings.readReply && settings.readReply.result.status !== 'completed' && <RawFacts summaryKey="receipt.details" value={settings.readReply} />}
    {settings.diagnostic && <RawFacts summaryKey="receipt.details" value={settings.diagnostic} />}
  </>;
}

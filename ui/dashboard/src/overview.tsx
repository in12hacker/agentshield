import { useLayoutEffect, useRef } from 'react';
import { AlertTriangle, Cpu, FileText, Globe, ShieldCheck, Zap, ChevronRight, type LucideIcon } from 'lucide-react';
import { driverFacts, factQualifier, featureView, flowFacts, flowView,
  overviewCommands, overviewView, receiptLabels, sourceLabels } from './overview-model';
import { idLabel } from './wire';
import { commandFacts, type CommandResult, type OverviewRead } from './reply-wire';
import { useI18n } from './i18n';
import type { MessageKey } from './messages';
import { processesRoute, protectionReceiptsRoute } from './global/navigation';
import { RawFacts } from './global/raw-facts';

function Value({ value }: { value: unknown }) {
  const { t } = useI18n();
  return <>{value === undefined ? t('common.unknownUnsupplied') : value === null ? t('common.null')
    : typeof value === 'string' ? JSON.stringify(value) : String(value)}</>;
}
function Receipt({ label, expected, result }: { label: string; expected: 'driver_status' | 'flow_status'; result: CommandResult }) {
  const { t } = useI18n();
  const data = commandFacts(result);
  const qualifier = factQualifier(result);
  return <article className="overview-panel receipt-card">
    <div className="panel-heading"><h3>{label}</h3><span className={result.status === 'completed' ? 'muted' : 'amber'}>{t(receiptLabels[result.status])}</span></div>
    {result.status === 'failed' && <div className="receipt-failure" role="status"><p>{t('common.failureCode')}<code>{result.code}</code></p><p>{t('common.originalMessage')}<code>{JSON.stringify(result.message)}</code></p></div>}
    {result.status === 'accepted' && <p className="overview-note">{t('receipt.acceptedNote')}</p>}
    {qualifier && data && <p className="overview-note">{t('receipt.qualifier', { qualifier: t(qualifier) })}</p>}
    {data?.type !== expected && <p className="overview-note">{t('receipt.wrongType')}</p>}
    {data?.type === 'driver_status' && expected === 'driver_status' && <dl className="receipt-fields">
      <div><dt>{t('receipt.source')}</dt><dd>{t(sourceLabels[data.snapshot.source_state])}</dd></div>
      <div><dt>{t('receipt.categories')}</dt><dd>{t('category.process')} {t(featureView(data.snapshot.process?.effective_state).label)} · {t('category.file')} {t(featureView(data.snapshot.file?.effective_state).label)} · {t('category.network')} {t(featureView(data.snapshot.network?.effective_state).label)} · {t('category.self')} {t(featureView(data.snapshot.self_protection).label)}</dd></div>
      <div><dt>{t('receipt.pending')}</dt><dd><Value value={data.snapshot.pending} /></dd></div>
      <div><dt>{t('receipt.unread')}</dt><dd><Value value={data.snapshot.pending_receipts} /></dd></div>
      <div><dt>{t('receipt.dropped')}</dt><dd><Value value={data.snapshot.dropped} /></dd></div>
      {data.snapshot.error !== null && <div><dt>{t('receipt.error')}</dt><dd><Value value={data.snapshot.error} /></dd></div>}
    </dl>}
    {data?.type === 'flow_status' && expected === 'flow_status' && <dl className="receipt-fields">
      <div><dt>{t('receipt.intake')}</dt><dd>{t(flowView(data.status.intake_state).label)}</dd></div>
      <div><dt>{t('receipt.ingress')}</dt><dd>{data.ingress === null ? t('common.unknownUnsupplied')
        : t(data.ingress.accepting ? 'flow.ingressOpen' : 'flow.ingressClosed')}</dd></div>
      <div><dt>{t('receipt.exchanges')}</dt><dd>{data.status.active_exchanges ?? t('common.unknownUnsupplied')}</dd></div>
      <div><dt>{t('receipt.lost')}</dt><dd>{data.status.lost_deliveries ?? t('common.unknownUnsupplied')}</dd></div>
      <div><dt>{t('receipt.parseFailures')}</dt><dd>{data.status.parse_failures ?? t('common.unknownUnsupplied')}</dd></div>
    </dl>}
    <RawFacts summaryKey="receipt.details" value={result} />
  </article>;
}

export function OverviewProblem({ read }: { read: OverviewRead }) {
  const { t } = useI18n();
  if (read.phase === 'send_failed' || read.phase === 'failed') return <p className="error" role="alert">{t(read.message.key, read.message.params)}</p>;
  return read.phase === 'returned' && read.result.status === 'failed' && <div className="receipt-failure" role="alert">
    <p>{t('overview.queryFailed')}<code>{read.result.code}</code></p>
    <p>{t('common.originalMessage')}<code>{JSON.stringify(read.result.message)}</code></p>
  </div>;
}

export function ProtectionOverview({ read, refresh }: { read: OverviewRead; refresh: () => void }) {
  const { t } = useI18n();
  const overviewElement = useRef<HTMLDivElement>(null);
  const overviewHeight = useRef(0);
  useLayoutEffect(() => {
    const section = overviewElement.current;
    if (!section || read.phase === 'loading') return;
    // Layout only: the read owner still discards old protection facts immediately.
    const measure = () => { overviewHeight.current = section.getBoundingClientRect().height; };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(section);
    return () => observer.disconnect();
  }, [read]);
  const view = overviewView(read);
  const StatusIcon = view.tone === 'attention' ? AlertTriangle : ShieldCheck;
  const commands = overviewCommands(read);
  const driver = driverFacts(read);
  const flow = flowFacts(read);
  const cards = [
    { name: 'category.network', icon: Globe, ...featureView(driver?.network?.effective_state), result: commands?.driver },
    { name: 'category.process', icon: Cpu, ...featureView(driver?.process?.effective_state), result: commands?.driver },
    { name: 'category.file', icon: FileText, ...featureView(driver?.file?.effective_state), result: commands?.driver },
    { name: 'category.self', icon: ShieldCheck, ...featureView(driver?.self_protection), result: commands?.driver },
    { name: 'category.flow', icon: Zap, ...flowView(flow?.intake_state), result: commands?.flow },
  ] satisfies { name: MessageKey; icon: LucideIcon; label: MessageKey; tone: string; result: CommandResult | undefined }[];
  return <div ref={overviewElement} id="overview" className="overview-content"
    style={{ minHeight: read.phase === 'loading' ? overviewHeight.current : undefined }}>
    <section className="overview-intro">
      <p className="overview-note">{t('overview.subtitle')}</p>
      <div className={`protection-banner ${view.tone}`}>
        <div className="banner-title"><StatusIcon size={20} /><h2 aria-live="polite">{t(view.label)}</h2></div>
        <p className="banner-help">{t('overview.help')}</p>
        <div className="banner-links"><a href={protectionReceiptsRoute}>{t('overview.details')}</a><button type="button" onClick={refresh}>{t(read.phase === 'loading' ? 'overview.retry' : 'common.refresh')}</button></div>
      </div>
    </section>
    <section id="protection-range" aria-labelledby="protection-range-title">
      <h2 className="section-eyebrow" id="protection-range-title" tabIndex={-1}>{t('overview.modules')}</h2>
      <div className="protection-grid">{cards.map(({ name, label, icon: Icon, tone, result }) => {
        const qualifier = factQualifier(result);
        return <a className={`protection-tile ${tone}`} href={protectionReceiptsRoute} key={name}>
          <Icon size={16} /><div><strong>{t(name)}</strong><span>{t(label)}</span>{qualifier && <small>{t(qualifier)}</small>}</div><ChevronRight size={12} />
        </a>;
      })}</div>
      <p className="range-note">{t('overview.range')}</p>
      <div className="banner-links"><a href={processesRoute}>{t('processes.browse')}</a></div>
    </section>
    <section id="protection-receipts" className="receipt-section" aria-labelledby="receipts-title">
      <div className="panel-heading"><h2 id="receipts-title" tabIndex={-1}>{t('overview.receipts')}</h2><button type="button" onClick={refresh}>{t('common.refresh')}</button></div>
      {read.phase !== 'initial' && read.id && <details className="overview-note">
        <summary>{t('overview.commandId')}</summary><code>{idLabel(read.id)}</code>
      </details>}
      {read.phase === 'loading' && <p className="overview-note">{t('overview.waiting')}</p>}
      <OverviewProblem read={read} />
      {commands && <div className="receipt-grid">
        <Receipt label={t('receipt.driver')} expected="driver_status" result={commands.driver} />
        <Receipt label={t('receipt.flow')} expected="flow_status" result={commands.flow} />
      </div>}
    </section>
  </div>;
}

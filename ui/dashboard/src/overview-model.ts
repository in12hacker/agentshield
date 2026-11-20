import { commandFacts, type CommandResult, type FlowStatus, type OverviewRead } from './reply-wire';
import type { DriverStatus, FeatureState } from './driver-wire';
import type { MessageKey } from './messages';

interface StatusView { label: MessageKey; tone: string }
interface OverviewView extends StatusView { shortLabel: MessageKey }

// Pure presentation of the one session-owned response, never a second fact cache.
export function overviewCommands(read: OverviewRead) {
  return read.phase === 'returned' && read.result.status === 'completed' ? read.result.data : null;
}
export function driverFacts(read: OverviewRead): DriverStatus | undefined {
  const data = commandFacts(overviewCommands(read)?.driver);
  return data?.type === 'driver_status' ? data.snapshot : undefined;
}
export function flowFacts(read: OverviewRead): FlowStatus | undefined {
  const data = commandFacts(overviewCommands(read)?.flow);
  return data?.type === 'flow_status' ? data.status : undefined;
}
export function featureView(state: FeatureState | undefined): StatusView {
  switch (state) {
    case 'enabled': return { label: 'feature.enabled', tone: 'enabled' };
    case 'disabled': return { label: 'feature.disabled', tone: 'attention' };
    case 'unavailable': return { label: 'feature.unavailable', tone: 'attention' };
    default: return { label: 'common.unknown', tone: 'unknown' };
  }
}
export function flowView(state: FlowStatus['intake_state'] | undefined): StatusView {
  switch (state) {
    case 'running': return { label: 'flow.running', tone: 'enabled' };
    case 'source_closed': return { label: 'flow.sourceClosed', tone: 'attention' };
    case 'stopped': return { label: 'flow.stopped', tone: 'unknown' };
    default: return { label: 'common.unknown', tone: 'unknown' };
  }
}
export const sourceLabels = { dormant: 'source.dormant', running: 'source.running', quiescing: 'source.quiescing', failed: 'source.failed' } satisfies Record<string, MessageKey>;
export const receiptLabels = { accepted: 'receipt.accepted', completed: 'receipt.completed', failed: 'receipt.failed' } satisfies Record<string, MessageKey>;
export function factQualifier(result: CommandResult | undefined): MessageKey | null {
  return result?.status === 'failed' ? 'receipt.failedFacts' : result?.status === 'accepted' ? 'receipt.acceptedFacts' : null;
}
export function overviewView(read: OverviewRead): OverviewView {
  if (read.phase === 'initial') return { label: 'overview.initial', shortLabel: 'overview.initialShort', tone: 'attention' };
  if (read.phase === 'loading') return { label: 'overview.loading', shortLabel: 'overview.loadingShort', tone: 'unknown' };
  if (read.phase === 'send_failed' || read.phase === 'failed' || read.result.status === 'failed') return { label: 'overview.failed', shortLabel: 'overview.failedShort', tone: 'attention' };
  const { driver, flow } = read.result.data;
  const driverSnapshot = driverFacts(read);
  const flowSnapshot = flowFacts(read);
  if (driver.status === 'completed' && flow.status === 'completed' && driverSnapshot && flowSnapshot) {
    return { label: 'overview.returned', shortLabel: 'overview.returnedShort', tone: 'unknown' };
  }
  if (driverSnapshot || flowSnapshot) return { label: 'overview.partial', shortLabel: 'overview.partialShort', tone: 'attention' };
  if (driver.status === 'accepted' || flow.status === 'accepted') return { label: 'overview.accepted', shortLabel: 'overview.acceptedShort', tone: 'attention' };
  return { label: 'overview.unknown', shortLabel: 'common.unknown', tone: 'attention' };
}

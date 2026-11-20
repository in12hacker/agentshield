import { useSyncExternalStore } from 'react';
import type { MessageKey } from '../messages';

export const navigationGroups = [
  { id: 'protection', titleKey: 'nav.groupProtection' },
  { id: 'activity', titleKey: 'nav.groupActivity' },
  { id: 'management', titleKey: 'nav.groupManagement' },
] as const satisfies readonly { id: string; titleKey: MessageKey }[];

// The only page registry. Internal overview positions are not separate workspaces.
export const dashboardPages = [
  { id: 'overview', href: '#overview', titleKey: 'nav.overview', headingId: 'overview-heading', group: 'protection',
    sections: [{ href: '#protection-range', headingId: 'protection-range-title' },
      { href: '#protection-receipts', headingId: 'receipts-title' }] },
  { id: 'processes', href: '#protection-targets', titleKey: 'processes.title', headingId: 'processes-title', group: 'protection', sections: [] },
  { id: 'rules', href: '#rules', titleKey: 'rules.title', headingId: 'rules-title', group: 'protection', sections: [] },
  { id: 'activity', href: '#activity', titleKey: 'activity.title', headingId: 'activity-heading', group: 'activity', sections: [] },
  { id: 'group', href: '#group', titleKey: 'group.title', headingId: 'group-heading', group: 'activity', sections: [] },
  { id: 'chat', href: '#chat-history', titleKey: 'chat.title', headingId: 'chat-heading', group: 'activity', sections: [] },
  { id: 'settings', href: '#settings', titleKey: 'settings.title', headingId: 'settings-title', group: 'management', sections: [] },
  { id: 'egress', href: '#egress-trust', titleKey: 'egress.title', headingId: 'egress-title', group: 'management', sections: [] },
] as const satisfies readonly {
  id: string; href: `#${string}`; titleKey: MessageKey; headingId: string;
  group: typeof navigationGroups[number]['id'];
  sections: readonly { href: `#${string}`; headingId: string }[];
}[];
export type DashboardPage = typeof dashboardPages[number];
export type DashboardPageId = DashboardPage['id'];
export type DashboardHash = DashboardPage['href'] | DashboardPage['sections'][number]['href'];

export const overviewRoute = dashboardPages[0].href;
export const protectionRangeRoute = dashboardPages[0].sections[0].href;
export const protectionReceiptsRoute = dashboardPages[0].sections[1].href;
export const processesRoute = dashboardPages[1].href;
export const rulesRoute = dashboardPages[2].href;
export const activityRoute = dashboardPages[3].href;
export const groupRoute = dashboardPages[4].href;
export const chatRoute = dashboardPages[5].href;
export const settingsRoute = dashboardPages[6].href;
export const egressRoute = dashboardPages[7].href;

export function resolveDashboardRoute(hash: string) {
  for (const page of dashboardPages) {
    if (page.href === hash) return { hash, page, target: page.href, headingId: page.headingId };
    const section = page.sections.find(section => section.href === hash);
    if (section) return { hash, page, target: section.href, headingId: section.headingId };
  }
  const page = dashboardPages[0];
  return { hash, page, target: page.href, headingId: page.headingId };
}
export type DashboardLocation = ReturnType<typeof resolveDashboardRoute>;

export function navigateDashboard(target: DashboardHash) {
  window.location.hash = target;
}

function readRoute() {
  return window.location.hash;
}

function subscribe(listener: () => void) {
  window.addEventListener('hashchange', listener);
  return () => window.removeEventListener('hashchange', listener);
}

export function useDashboardRoute() {
  return resolveDashboardRoute(useSyncExternalStore(subscribe, readRoute));
}

import { afterEach, describe, expect, it, vi } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { DashboardShell } from '../dashboard';
import { I18nProvider } from '../i18n';
import { dashboardPages, resolveDashboardRoute } from './navigation';

afterEach(() => vi.unstubAllGlobals());

describe('formal Dashboard hash destinations', () => {
  it('resolves direct links and overview positions to the actual workspace and heading', () => {
    for (const page of dashboardPages) {
      const direct = resolveDashboardRoute(page.href);
      expect(direct.page).toBe(page);
      expect(direct.headingId).toBe(page.headingId);
      for (const section of page.sections) {
        const position = resolveDashboardRoute(section.href);
        expect(position.page).toBe(dashboardPages[0]);
        expect(position.target).toBe(section.href);
        expect(position.headingId).toBe(section.headingId);
      }
    }
    expect(new Set(dashboardPages.map(page => page.href)).size).toBe(8);
  });

  it.each(['', '#', '#unknown', '#original-review', '#activity/extra', '#settings?edit=true', '#Overview', '#%72ules'])
    ('shows overview for %j without changing the URL or writing history', hash => {
      const location = Object.freeze({ hash });
      const history = { pushState: vi.fn(), replaceState: vi.fn() };
      vi.stubGlobal('window', { location, history });
      expect(resolveDashboardRoute(hash)).toMatchObject({ page: dashboardPages[0], target: '#overview' });
      expect(location.hash).toBe(hash);
      expect(history.pushState).not.toHaveBeenCalled();
      expect(history.replaceState).not.toHaveBeenCalled();
    });

  it.each([false, true])('renders one labelled navigation and one page title (narrow=%s)', narrow => {
    vi.stubGlobal('document', { documentElement: { classList: { contains: () => false } } });
    vi.stubGlobal('window', { matchMedia: (query: string) => ({ matches: narrow && query.includes('max-width') }) });
    for (const page of dashboardPages) {
      const route = resolveDashboardRoute(page.href);
      const markup = renderToStaticMarkup(createElement(I18nProvider, { children: createElement(DashboardShell, {
        route, askVisible: false, read: { phase: 'initial' }, refresh: () => {}, lock: () => {}, children: null,
      }) }));
      expect(markup.match(/<nav\b/g)).toHaveLength(1);
      expect(markup.match(/<h1\b/g)).toHaveLength(1);
      expect(markup).toContain(`<h1 id="${page.headingId}" tabindex="-1">`);
      expect(markup.match(/aria-current="page"/g)).toHaveLength(1);
      expect(markup).toContain(`href="${page.href}" aria-current="page"`);
      expect(markup).toContain('aria-expanded="false" aria-controls="dashboard-navigation"');
      expect(markup).toContain(narrow ? 'class="dashboard-sidebar" hidden=""' : 'class="dashboard-sidebar"><');
      for (const destination of dashboardPages) expect(markup.match(new RegExp(`href="${destination.href}"`, 'g'))).toHaveLength(1);
      expect(markup).not.toContain('href="#protection-range"');
      expect(markup).not.toContain('href="#protection-receipts"');
    }
  });
});

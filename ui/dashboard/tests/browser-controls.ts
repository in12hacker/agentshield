import { expect, type Locator, type Page } from '@playwright/test';
import { dashboardPages, type DashboardPageId } from '../src/global/navigation';

/** Shared browser-only steps. This module never installs or supplies a transport. */
export async function navigate(page: Page, id: DashboardPageId) {
  const target = dashboardPages.find(p => p.id === id)!;
  const menu = page.locator('.menu-button');
  if (await menu.isVisible() && await menu.getAttribute('aria-expanded') === 'false') await menu.click();
  await page.locator(`nav a[href="${target.href}"]`).click();
  await expect(page.locator(`#${target.headingId}`)).toBeFocused();
}

export async function disclosures(scope: Locator, observation: { local: (action: () => Promise<unknown>) => Promise<void> }) {
  const opened: Locator[] = [];
  for (let i = 0; i < await scope.locator('summary').count(); i++) {
    const summary = scope.locator('summary').nth(i);
    if (!await summary.isVisible() || await summary.evaluate(e => e.parentElement?.hasAttribute('open'))) continue;
    await observation.local(() => summary.click());
    await expect(summary.locator('..')).toHaveAttribute('open', '');
    await expect(summary.locator('..')).not.toHaveText(await summary.textContent() ?? '');
    opened.push(summary);
  }
  for (const summary of opened.reverse()) {
    await observation.local(() => summary.click());
    await expect(summary.locator('..')).not.toHaveAttribute('open', '');
  }
}

export async function installControlObservation(page: Page, locale: string, excludedPages: readonly string[] = []) {
  await page.addInitScript(locale => localStorage.setItem('agentshield.dashboard.language', locale), locale);
  await page.addInitScript(({ pages, excludedPages }) => {
    const records: any[] = [];
    const known = new WeakMap<Element, any>();
    (window as any).__controlInventory = records;
    const selector = 'button,a[href],select,input,textarea,summary';
    const currentPage = () => pages.find(page => page.headingId === document.querySelector('main.dashboard-main')?.getAttribute('aria-labelledby'))?.id ?? 'gate';
    const excluded = () => excludedPages.includes(currentPage());
    const text = (element: Element | null | undefined) => {
      const copy = element?.cloneNode(true) as Element | undefined;
      copy?.querySelectorAll('input,select,textarea,svg,[aria-hidden=true]').forEach(child => child.remove());
      return copy?.textContent?.trim() ?? '';
    };
    const capture = () => {
      if (excluded()) return;
      document.querySelectorAll(selector).forEach(element => {
        const input = element as HTMLInputElement;
        const page = currentPage();
        const owner = element.closest('dialog,.gate,.dashboard-header,nav,.workspace-page > [id]');
        const moduleRoot = element.closest('.workspace-page > [id]');
        const module = owner?.matches('dialog.review-dialog') ? 'ask' : moduleRoot?.id
          ?? (owner?.matches('.gate') ? 'gate' : 'global');
        const scope = owner?.matches('dialog') ? `dialog:${owner.getAttribute('aria-labelledby')}`
          : owner?.matches('.gate') ? 'gate' : owner?.matches('.dashboard-header') ? 'global:header'
            : owner?.matches('nav') ? 'global:nav' : moduleRoot ? `module:${moduleRoot.id}` : 'global:shell';
        const path: string[] = [];
        for (let node: Element | null = element; node && node !== owner; node = node.parentElement) {
          const siblings = node.parentElement ? Array.from(node.parentElement.children).filter(sibling => sibling.tagName === node!.tagName) : [node];
          path.unshift(`${node.tagName.toLowerCase()}:nth-of-type(${siblings.indexOf(node) + 1})`);
        }
        const fieldsets: string[] = [], context: string[] = [];
        for (let parent = element.parentElement; parent && parent !== owner; parent = parent.parentElement) {
          if (parent.matches('fieldset')) fieldsets.unshift(text(parent.querySelector(':scope > legend')));
          if (parent.matches('.rule-input-item,.settings-area,.settings-confirm,.settings-feature,.chain-row,.chat-part,.chat-message,.group-reference,.group-call')) {
            const heading = parent.querySelector(':scope > .panel-heading > strong,:scope > h2,:scope > h3,:scope > h4,:scope > legend');
            if (heading) context.unshift(text(heading));
          }
        }
        const hidden = !element.getClientRects().length || getComputedStyle(element).visibility === 'hidden';
        const reasons: string[] = [];
        if (input.type === 'hidden') reasons.push('hidden_input');
        if (element.closest('[hidden]')) reasons.push('hidden_attribute');
        if (element.closest('dialog:not([open])')) reasons.push('closed_dialog');
        if (hidden && !reasons.length) reasons.push(element.closest('details:not([open])') ? 'closed_details' : 'not_rendered');
        if (element.matches(':disabled')) reasons.push(element.closest('fieldset:disabled') ? 'disabled_fieldset' : 'disabled_control');
        if (input.readOnly === true) reasons.push('read_only');
        const state = {
          page, module, scope, path: path.join('>'),
          region: element.closest('[aria-labelledby]')?.getAttribute('aria-labelledby') ?? element.closest('[id]')?.id ?? null,
          fieldsets, context,
          category: element.closest('[data-rule-category]')?.getAttribute('data-rule-category') ?? null,
          group: element.closest('[data-rule-group]')?.getAttribute('data-rule-group') ?? null,
          tag: element.tagName.toLowerCase(), id: element.id, type: input.type || null,
          name: element.getAttribute('name'), href: element.getAttribute('href'),
          label: element.getAttribute('aria-label') || text(input.labels?.[0]) || text(element),
          readOnly: input.readOnly === true, hidden, disabled: element.matches(':disabled'), reasons,
        };
        let record = known.get(element);
        if (!record) {
          record = { elementInstance: records.length + 1, states: [], current: null };
          records.push(record); known.set(element, record);
        }
        const signature = JSON.stringify(state);
        let observed = record.states.find((observed: any) => observed.signature === signature);
        if (!observed) { observed = { ...state, signature, actions: [], focused: false }; record.states.push(observed); }
        record.current = observed;
      });
    };
    (window as any).__describeControl = (element: Element) => {
      if (excluded()) return null;
      capture();
      const record = known.get(element);
      if (!record?.current) return null;
      const { signature: _signature, actions: _actions, focused: _focused, ...state } = record.current;
      return { elementInstance: record.elementInstance, ...state };
    };
    document.addEventListener('DOMContentLoaded', () => {
      capture(); new MutationObserver(capture).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    });
    for (const type of ['click', 'input', 'change', 'keydown', 'focusin']) document.addEventListener(type, event => {
      if (excluded()) return;
      const element = (event.target as Element)?.closest(selector);
      capture();
      const state = element && known.get(element)?.current;
      if (!state) return;
      if (type === 'focusin') state.focused = true;
      else if (!state.hidden && !state.disabled && (type !== 'keydown' || ['Enter', ' '].includes((event as KeyboardEvent).key))) state.actions.push(type);
    }, true);
  }, { pages: dashboardPages.map(({ id, headingId }) => ({ id, headingId })), excludedPages: [...excludedPages] });
}

export async function controlObservation(page: Page) {
  return page.evaluate(() => ((window as any).__controlInventory ?? []).map((record: any) => ({
    elementInstance: record.elementInstance,
    states: record.states.map(({ signature: _signature, ...state }: any) => state),
  })));
}

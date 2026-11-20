import { test as base, expect, type Page, type WebSocketRoute } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import { factsJson, parseUnique, record } from '../src/wire-json';
import { parseFrame } from '../src/wire';
import { VERSION } from '../src/request-wire';
import { translate, type MessageKey } from '../src/messages';
import type { Locale } from '../src/i18n-contract';
import { installControlObservation, controlObservation } from './browser-controls';
export { navigate, disclosures } from './browser-controls';
import { overviewResult, testToken } from '../test-support/contract-supply';

type Lane = 'business' | 'message' | 'private' | 'stream';
export interface Request { lane: Lane; id: number[]; body: Record<string, any>; socket: WebSocketRoute }

// Transport-only fixture. Tests explicitly supply every business/management/stream reply.
export class Peer {
  requests: Request[] = [];
  events: unknown[] = [];
  sockets: WebSocketRoute[] = [];
  authenticationTokensMatch: boolean[] = [];
  access = 'manage_service';
  content = true;
  authenticate = true;
  constructor(readonly page: Page) {}
  async install() {
    await this.page.routeWebSocket('ws://127.0.0.1:21854/', socket => {
      this.sockets.push(socket);
      socket.onMessage(raw => {
        const frame = record(parseUnique(String(raw)));
        expect(frame.version).toBe(VERSION);
        const lanes = Object.keys(frame).filter(key => key !== 'version');
        expect(lanes).toHaveLength(1);
        expect(['authenticate', 'business', 'private', 'message', 'stream']).toContain(lanes[0]);
        const lane = lanes[0] as Lane | 'authenticate';
        const payload = record(frame[lane]);
        if (lane === 'authenticate') {
          this.authenticationTokensMatch.push(payload.token === testToken);
          // No credential value is kept in interface evidence.
          this.events.push({ direction: 'out', lane: 'authenticate' });
          if (this.authenticate) socket.send(JSON.stringify({ version: VERSION, authenticated: { access: this.access, chat_content: this.content } }));
          return;
        }
        const body = record(lane === 'business' ? payload.query : lane === 'private' || lane === 'stream' ? payload.command : payload);
        if (payload.id) this.requests.push({ lane, id: payload.id as number[], body, socket });
        if (lane === 'private') {
          const { value: _value, ...operation } = body;
          this.events.push({ direction: 'out', lane, id: payload.id, operation });
        } else this.events.push({ direction: 'out', lane, payload });
      });
    });
  }
  async next(action: () => Promise<unknown>, lane: Lane, type: string) {
    const begin = this.requests.length;
    await action();
    await expect.poll(() => this.requests.slice(begin).filter(r => r.lane === lane && r.body.type === type).length).toBe(1);
    return this.requests.slice(begin).find(r => r.lane === lane && r.body.type === type)!;
  }
  reply(request: Request, result: unknown) {
    const payload = { id: request.id, result };
    const frame = { version: VERSION, ...(request.lane === 'business' ? { business_reply: payload }
      : request.lane === 'private' ? { private_reply: payload } : request.lane === 'stream' ? { stream_reply: payload }
        : { message: { type: 'reply', ...payload } }) };
    if (request.lane === 'stream') {
      const decoded = parseFrame(factsJson(frame));
      expect(decoded.kind).toBe('stream_reply');
      if (decoded.kind !== 'stream_reply') throw Error('expected current stream reply');
      const ack = decoded.reply.result;
      if (ack.status === 'subscribed') {
        expect(request.body.type).toBe('subscribe');
        expect(ack.subscription_id).toEqual(request.id);
      } else if (ack.status === 'unsubscribed') {
        expect(request.body.type).toBe('unsubscribe');
        expect(ack.subscription_id).toEqual(request.body.subscription_id);
      }
    }
    this.events.push(request.lane === 'private'
      ? { direction: 'in', lane: 'private_reply', id: request.id, status: (result as any).status, type: (result as any).data?.type }
      : { direction: 'in', lane: request.lane === 'stream' ? 'stream_reply' : request.lane, ...payload });
    request.socket.send(factsJson(frame));
  }
  push(message: unknown) {
    this.events.push({ direction: 'in', lane: 'message', message });
    this.sockets.at(-1)!.send(factsJson({ version: 2, message }));
  }
  diagnostic(diagnostic: unknown) {
    this.events.push({ direction: 'in', lane: 'diagnostic', diagnostic });
    this.sockets.at(-1)!.send(factsJson({ version: 2, diagnostic }));
  }
  async local(action: () => Promise<unknown>) {
    const count = this.events.length;
    await action();
    // Two frames flush React's layout/effect work; local controls must not send anything.
    await this.page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    expect(this.events.length).toBe(count);
  }
}

export const ok = (data: unknown) => ({ status: 'completed', data });
export const failed = (code = 'unavailable', message = '') => ({ status: 'failed', code, message, facts: null });

// Capture the selected SQL read and its tail subscription. The caller supplies every ack/reply.
export async function contextRead(peer: Peer, action: () => Promise<unknown>, query: Record<string, unknown> & { type: 'ask_context' | 'activity_context' }) {
  let read!: Request;
  const subscription = await peer.next(async () => {
    read = await peer.next(action, 'business', query.type);
  }, 'stream', 'subscribe');
  expect(read.body).toEqual(query);
  expect(subscription.body).toEqual({ type: 'subscribe', filter: {}, after: null });
  expect(subscription.id).not.toEqual(read.id);
  return { read, subscription };
}

export async function contextUnsubscribe(peer: Peer, subscription: Request, action: () => Promise<unknown>) {
  const retired = await peer.next(action, 'stream', 'unsubscribe');
  expect(retired.body).toEqual({ type: 'unsubscribe', subscription_id: subscription.id });
  expect(retired.id).not.toEqual(subscription.id);
  return retired;
}

export const test = base.extend<{ peer: Peer; t: (key: MessageKey, params?: Record<string, string | number>) => string }>({
  t: async ({ locale }, use) => {
    await use((key, params) => translate(locale as Locale, key, params));
  },
  peer: async ({ page, locale }, use, info) => {
    const peer = new Peer(page);
    const browserEvents: string[] = [];
    page.on('framenavigated', frame => { if (frame === page.mainFrame()) browserEvents.push(`navigation ${frame.url()}`); });
    page.on('console', message => { if (message.text().startsWith('[vite]')) browserEvents.push(message.text()); });
    await peer.install();
    await installControlObservation(page, locale!);
    await use(peer);
    if (!page.isClosed()) {
      const inventoryPath = info.outputPath('dom-controls.json');
      const controls = await controlObservation(page);
      await writeFile(inventoryPath, JSON.stringify({ project: info.project.name, test: info.title, testResult: info.status, controls }, null, 2));
      await info.attach('dom-controls.json', { path: inventoryPath, contentType: 'application/json' });
      if (info.status !== info.expectedStatus) await info.attach('browser-error-summary.json', {
        body: JSON.stringify((await page.pageErrors()).map(error => error.message)), contentType: 'application/json',
      });
    }
    const eventsPath = info.outputPath('interface-events.json');
    await writeFile(eventsPath, factsJson(peer.events));
    await info.attach('interface-events.json', { path: eventsPath, contentType: 'application/json' });
    await info.attach('browser-lifecycle.json', { body: JSON.stringify(browserEvents, null, 2), contentType: 'application/json' });
  },
});
export { expect };

export async function unlock(page: Page, peer: Peer, route = '') {
  if (page.url() === 'about:blank' || route) await page.goto(`/${route}`);
  const overview = await peer.next(async () => {
    await page.locator('#management-token').fill(testToken);
    await page.locator('button[type=submit]').click();
  }, 'business', 'protection_overview');
  peer.reply(overview, overviewResult());
  expect(peer.authenticationTokensMatch.at(-1)).toBe(true);
  await expect(page.locator('.dashboard-shell')).toBeVisible();
}

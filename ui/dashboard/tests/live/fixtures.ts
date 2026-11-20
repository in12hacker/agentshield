import { test as base, expect, type Page, type TestInfo, type Locator } from '@playwright/test';
import { appendFileSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { preview, type PreviewServer } from 'vite';
import { translate, type MessageKey } from '../../src/messages';
import type { Locale } from '../../src/i18n-contract';
import { parseFrame, idLabel } from '../../src/wire';
import { parseUnique, factsJson, wireJson } from '../../src/wire-json';
import { sameJsonValue } from '../../src/global/json-value';
import { decodePrivateOperation } from '../../src/credentials-wire';
import { installControlObservation, controlObservation, navigate } from '../browser-controls';

export { expect, navigate };
const execute = promisify(execFile);
const repo = fileURLToPath(new URL('../../../../', import.meta.url));
const fixture = process.env.AS_LIVE_FIXTURE!;
export const same = (a: unknown, b: unknown) => a === undefined || b === undefined ? a === b : sameJsonValue(a, b);

export class LiveFailure extends Error {
  constructor(readonly code: string) { super(code); }
}
export function requireLive(value: unknown, code: string): asserts value {
  if (!value) throw new LiveFailure(code);
}
export interface WireObservation {
  line: number; connection: number; direction: 'out' | 'in'; frame: any;
  raw_ref: string | null; decoder_ok?: boolean;
  metadata_ref?: string | null;
}
export interface Exchange { request: WireObservation; reply: WireObservation }

/** Real browser socket observation only. It has no send/reply/mock capability. */
export class LiveObservation {
  readonly frames: WireObservation[] = [];
  readonly steps: any[] = [];
  private readonly privateReplies = new Map<string, any>();
  private readonly endedConnections = new Set<number>();
  private consumerFailure: string | null = null;
  private collectorFailures = 0;
  private connection = 0;
  private line = 0;
  private currentStage = 'initialization';
  private decoderFailures = 0;
  private pageErrors = 0;
  private readonly directory: string;
  private locale: string;
  private width: number;
  private originalSource: Record<string, unknown> | null = null;
  constructor(readonly page: Page, readonly info: TestInfo, private readonly server: PreviewServer) {
    this.locale = info.project.use.locale!;
    this.width = info.project.use.viewport!.width;
    const output = info.outputPath('live');
    mkdirSync(output, { mode: 0o700 });
    this.directory = join(output, 'private-evidence');
    mkdirSync(this.directory, { mode: 0o700 });
    page.on('pageerror', () => { this.pageErrors++; this.consumerFailure = 'PAGE_CONSUMER_ERROR'; });
    page.on('crash', () => { this.consumerFailure = 'PAGE_CRASHED'; });
    page.on('websocket', socket => {
      const connection = ++this.connection;
      this.save('connections.jsonl', { connection, url: socket.url(), event: 'open' });
      const capture = (direction: 'out' | 'in', payload: string | Buffer) => {
        try { this.observe(connection, direction, payload.toString()); }
        catch { this.collectorFailures++; } // Observation never throws into the live socket pump.
      };
      socket.on('framesent', ({ payload }) => capture('out', payload));
      socket.on('framereceived', ({ payload }) => capture('in', payload));
      socket.on('socketerror', () => { this.endedConnections.add(connection); });
      socket.on('close', () => {
        this.endedConnections.add(connection);
        this.save('connections.jsonl', { connection, event: 'closed' });
      });
    });
  }
  // Business originals may themselves contain restricted material. Retain all
  // acquired business bytes in the same private evidence boundary, never decide
  // admission from their text. Authentication/private values stay memory-only.
  private save(file: string, value: unknown) {
    try { appendFileSync(join(this.directory, file), wireJson(value) + '\n', { mode: 0o600 }); return true; }
    catch (error) {
      this.collectorFailures++;
      try { appendFileSync(join(this.directory, 'collector-errors.jsonl'), JSON.stringify({ artifact: file,
        error_type: error instanceof Error ? error.name : 'unknown' }) + '\n', { mode: 0o600 }); } catch {}
      return false; // Optional collection cannot interrupt an Answer.
    }
  }
  private observe(connection: number, direction: 'out' | 'in', wire: string) {
    let frame: any;
    try { frame = parseUnique(wire); } catch {
      this.decoderFailures++;
      this.save('frames.jsonl', { line: ++this.line, connection, direction, kind: 'invalid_json', raw_retained: false }); return;
    }
    const line = ++this.line;
    if (!frame || typeof frame !== 'object' || Array.isArray(frame)) {
      this.decoderFailures++;
      this.save('frames.jsonl', { line, connection, direction, kind: 'unclassified', raw_retained: false }); return;
    }
    if (Object.hasOwn(frame, 'authenticate')) {
      this.save('frames.jsonl', { line, connection, direction, kind: 'authenticate', raw_retained: false }); return;
    }
    if (Object.hasOwn(frame, 'private')) {
      const { value: _value, ...rawOperation } = frame.private.command;
      const operation = decodePrivateOperation(rawOperation);
      const retained = this.save('frames.jsonl', { line, connection, direction, kind: 'private', id: frame.private.id, operation, raw_retained: false });
      this.frames.push({ line, connection, direction, frame: { private: { id: frame.private.id, command: operation } }, raw_ref: null,
        metadata_ref: retained ? `frames.jsonl:${line}` : null }); return;
    }
    if (frame.private_reply) {
      const reply = frame.private_reply;
      let decoder_ok = true;
      try { parseFrame(wire); } catch { decoder_ok = false; this.decoderFailures++; }
      this.privateReplies.set(`${connection}:${idLabel(reply.id)}`, reply.result);
      const retained = this.save('frames.jsonl', { line, connection, direction, kind: 'private_reply', id: reply.id,
        status: reply.result?.status, type: reply.result?.data?.type ?? null, decoder_ok, raw_retained: false });
      this.frames.push({ line, connection, direction, frame: { private_reply: { id: reply.id, status: reply.result?.status, type: reply.result?.data?.type ?? null } }, decoder_ok, raw_ref: null,
        metadata_ref: retained ? `frames.jsonl:${line}` : null }); return;
    }
    let decoder_ok = true;
    if (direction === 'in') {
      try { parseFrame(wire); } catch { decoder_ok = false; this.decoderFailures++; }
    }
    const retained = this.save('frames.jsonl', { line, connection, direction, decoder_ok, wire,
      sha256: createHash('sha256').update(wire).digest('hex'), observed_mono_ns: process.hrtime.bigint().toString() });
    const raw_ref = retained ? `frames.jsonl:${line}` : null;
    this.frames.push({ line, connection, direction, frame, raw_ref, decoder_ok });
  }
  record(name: string, status: 'PASS' | 'FAIL' | 'LIVE_BLOCKED' | 'NOT_OBSERVED' | 'OBSERVER_FAILED', data: unknown = {}) {
    const row = { name, status, project: this.info.project.name, observed_project: `${this.locale}-${this.width}`, stage: this.currentStage, data };
    this.steps.push(row); this.save('steps.jsonl', row);
  }
  stage(name: string) { this.currentStage = name; }
  async guest(message: Record<string, unknown>) {
    const process = await execute('python3', ['-B', join(fixture, 'call-vm.py'), factsJson(message)], { maxBuffer: 8 * 1024 * 1024 });
    const reply = parseUnique(process.stdout) as any;
    if (message.op === 'owner_status') return reply;
    if (!reply.ok) {
      this.save('guest-errors.jsonl', { op: message.op, error: reply.error });
      throw new LiveFailure('OWNED_GUEST_OPERATION_FAILED');
    }
    return reply.result;
  }
  async initialize(locale: string) {
    const delivery = parseUnique(readFileSync(join(fixture, 'selected-delivery.json'), 'utf8')) as any;
    const serviceSha = delivery.joint_assets?.['as/bin/as-service'];
    requireLive(typeof serviceSha === 'string' && /^[a-f0-9]{64}$/.test(serviceSha), 'SELECTED_SERVICE_BINDING_INVALID');
    const status = await this.guest({ op: 'owner_status' });
    this.save('environment.jsonl', { state: status.state, kernel_sha256: status.kernel_sha256, qemu_pid: status.qemu_pid,
      selected_delivery: status.selected_delivery, service_expected_sha256: serviceSha });
    // Gateway startup is independent of the already-started AS management WS.
    requireLive(status.qemu_exit === undefined || status.qemu_exit === null, 'OWNED_VM_ACTUALLY_EXITED');
    requireLive(same(status.selected_delivery, delivery), 'OWNER_SELECTED_DELIVERY_DIFFERS');
    requireLive(status.kernel_sha256 === delivery.kernel_sha256, 'OWNER_KERNEL_BINDING_MISMATCH');
    const hashes: Record<string, string> = {};
    const dashboard = resolve(repo, 'ui/dashboard');
    const hash = (file: string) => { hashes[file] = createHash('sha256').update(readFileSync(join(dashboard, file))).digest('hex'); };
    const source = (directory: string) => {
      for (const entry of readdirSync(join(dashboard, directory), { withFileTypes: true })) {
        if (entry.name.startsWith('.env')) continue;
        const file = join(directory, entry.name);
        if (entry.isDirectory()) source(file);
        else if (entry.isFile()) hash(file);
      }
    };
    source('src'); source('tests/live'); hash('index.html'); hash('vite.config.mjs'); hash('playwright.live.config.ts');
    const git = await execute('git', ['rev-parse', 'HEAD'], { cwd: repo });
    this.save('source-manifest.jsonl', { git_head: git.stdout.trim(), files: hashes });
    const assets = resolve(repo, 'ui/dashboard/dist/assets');
    this.save('compiled-assets.jsonl', Object.fromEntries(readdirSync(assets).map(file => [file, createHash('sha256').update(readFileSync(join(assets, file))).digest('hex')])));
    const cdp = await this.page.context().newCDPSession(this.page);
    await cdp.send('Network.enable');
    cdp.on('Network.webSocketWillSendHandshakeRequest', value => {
      this.save('handshakes.jsonl', { event: 'request', id: value.requestId, origin: value.request.headers.Origin ?? value.request.headers.origin ?? null });
    });
    cdp.on('Network.webSocketHandshakeResponseReceived', value => {
      this.save('handshakes.jsonl', { event: 'response', id: value.requestId, status: value.response.status });
    });
    await installControlObservation(this.page, locale, ['gate', 'egress']);
  }
  async unlock(expectedChatContent = true) {
    this.stage('real_authentication');
    const from = this.frames.length;
    await this.page.goto('/#overview');
    const accessPath = join(fixture, 'vm-access.json');
    requireLive((statSync(accessPath).mode & 0o777) === 0o600, 'ACCESS_FILE_NOT_0600');
    const access = JSON.parse(readFileSync(accessPath, 'utf8')) as { dashboard_token: string; gateway_token: string };
    let token = access.dashboard_token;
    requireLive(/^[a-f0-9]{64}$/i.test(token), 'ACCESS_TOKEN_FORMAT_INVALID');
    try {
      await this.page.locator('#management-token').fill(token);
      await Promise.all([
        this.page.waitForEvent('websocket', { timeout: 0 }),
        this.page.locator('.gate button[type=submit]').click(),
      ]);
    } catch { throw new LiveFailure('AUTH_INPUT_OR_SUBMIT_FAILED'); }
    finally { token = ''; }
    const connection = this.connection;
    const auth = await this.until(() => this.frames.slice(from).find(o => o.connection === connection && o.direction === 'in' && o.frame.authenticated),
      'AUTHENTICATED_FRAME_NOT_RECEIVED', () => this.checkConsumer(connection));
    await expect(this.page.locator('.dashboard-shell')).toBeVisible();
    requireLive(auth.decoder_ok && auth.frame.authenticated.access === 'manage_service' && auth.frame.authenticated.chat_content === expectedChatContent, 'ACTUAL_AUTHORIZATION_DIFFERS');
    this.record('real_authentication', 'PASS', { raw_reply_ref: auth.raw_ref, access: auth.frame.authenticated.access, chat_content: auth.frame.authenticated.chat_content,
      origin: new URL(this.page.url()).origin });
  }
  checkConsumer(connection = this.connection) {
    requireLive(this.server.httpServer.listening, 'OWNED_HTTP_SERVER_ENDED');
    requireLive(!this.page.isClosed(), 'BROWSER_PAGE_CLOSED');
    requireLive(!this.consumerFailure, this.consumerFailure ?? 'PAGE_CONSUMER_FAILED');
    requireLive(!this.endedConnections.has(connection), 'ORIGINAL_SOCKET_ENDED');
  }
  connectionEnded(connection: number) { return this.endedConnections.has(connection); }
  async until<T>(predicate: () => T | undefined | false | null, code: string, owner?: () => void): Promise<T> {
    for (;;) {
      const value = predicate();
      if (value) return value;
      try { this.checkConsumer(); owner?.(); }
      catch (error) { this.record(code, 'OBSERVER_FAILED', { waiting_for: code }); throw error; }
      await sleep(20);
    }
  }
  async exchange(action: () => Promise<unknown>, lane: 'business' | 'message', type: string): Promise<Exchange> {
    const from = this.frames.length;
    await action();
    return this.exchangeAfter(from, lane, type);
  }
  async exchangeAfter(from: number, lane: 'business' | 'message', type: string): Promise<Exchange> {
    const request = await this.until(() => this.frames.slice(from).find(o => o.direction === 'out' && o.frame[lane]?.type === type
      || o.direction === 'out' && lane === 'business' && o.frame.business?.query?.type === type), 'ORIGINAL_UI_REQUEST_NOT_OBSERVED');
    const id = request.frame[lane].id;
    const reply = await this.until(() => this.frames.find(o => o.connection === request.connection && o.direction === 'in'
      && (same(o.frame.diagnostic?.id, id) || same(lane === 'business' ? o.frame.business_reply?.id : o.frame.message?.id, id)
      && (lane === 'business' ? o.frame.business_reply?.result.status : o.frame.message?.result?.status) !== 'accepted')),
    'ORIGINAL_UI_REPLY_NOT_RECEIVED', () => this.checkConsumer(request.connection));
    requireLive(!reply.frame.diagnostic, 'ORIGINAL_UI_REPLY_DIAGNOSTIC');
    requireLive(reply.decoder_ok, 'REAL_REPLY_REJECTED_BY_PRODUCTION_DECODER');
    return { request, reply };
  }
  result(exchange: Exchange) { return exchange.reply.frame.business_reply?.result ?? exchange.reply.frame.message.result; }
  refs(exchange: Exchange) { return { request_ref: exchange.request.raw_ref, reply_ref: exchange.reply.raw_ref, id: exchange.request.frame.business?.id ?? exchange.request.frame.message?.id }; }
  source(exchange: Exchange) { this.originalSource = this.refs(exchange); }
  async describe(locator: Locator) { return locator.evaluate(element => (window as any).__describeControl?.(element) ?? null); }
  async privateInput(locator: Locator, value: string) {
    try { await locator.fill(value); } catch { throw new LiveFailure('PRIVATE_INPUT_FAILED_NO_PARAMETERS_RETAINED'); }
  }
  async privateLocal(name: string, locator: Locator, action: () => Promise<unknown>) {
    const control = await locator.evaluate(element => {
      const input = element as HTMLInputElement;
      const owner = element.closest('#egress-trust');
      const path: string[] = [];
      for (let node: Element | null = element; node && node !== owner; node = node.parentElement) {
        const siblings = Array.from(node.parentElement?.children ?? []).filter(sibling => sibling.tagName === node!.tagName);
        path.unshift(`${node.tagName.toLowerCase()}:nth-of-type(${siblings.indexOf(node) + 1})`);
      }
      return { owner: 'egress-trust', path: path.join('>'), tag: element.tagName.toLowerCase(), id: element.id,
        role: element.getAttribute('role'), ...('type' in element ? { type: input.type } : {}), readOnly: input.readOnly === true,
        disabled: element.matches(':disabled'), hidden: !element.getClientRects().length };
    });
    await this.local(action);
    this.record(name, 'PASS', { local: true, control, values_retained: false,
      assertion_scope: 'local interaction and no outgoing frame; value/effect comparisons belong to the scenario' });
  }
  async privateCommand(action: () => Promise<unknown>, type: string) {
    const from = this.frames.length;
    await action();
    const request = await this.until(() => this.frames.slice(from).find(o => o.direction === 'out' && o.frame.private?.command.type === type), 'ORIGINAL_UI_PRIVATE_REQUEST_NOT_OBSERVED');
    const id = request.frame.private.id;
    const reply = await this.until(() => this.frames.find(o => o.direction === 'in' && o.connection === request.connection
      && (same(o.frame.private_reply?.id, id) || same(o.frame.diagnostic?.id, id) || o.frame.diagnostic?.type === 'private_not_authorized')),
    'ORIGINAL_UI_PRIVATE_REPLY_NOT_OBSERVED', () => this.checkConsumer(request.connection));
    requireLive(!reply.frame.diagnostic, 'ORIGINAL_PRIVATE_REPLY_DIAGNOSTIC');
    requireLive(reply.decoder_ok, 'PRIVATE_REPLY_REJECTED_BY_PRODUCTION_DECODER');
    const key = `${request.connection}:${idLabel(id)}`;
    const result = this.privateReplies.get(key);
    this.privateReplies.delete(key);
    requireLive(result, 'PRIVATE_ORIGINAL_RESULT_ALREADY_RELEASED');
    return { result, command: request.frame.private.command, metadata: { id, operation: type, request_metadata_ref: request.metadata_ref ?? null,
      reply_metadata_ref: reply.metadata_ref ?? null, raw_private_retained: false } };
  }
  preserve(name: string, value: unknown) {
    requireLive(/^[a-z0-9-]+$/.test(name), 'INVALID_EVIDENCE_NAME');
    this.save(name + '.jsonl', value);
  }
  async language(locale: Locale) {
    if (await this.page.locator('html').getAttribute('lang') === locale) {
      this.locale = locale;
      this.record('interface_language_observed', 'PASS', { locale, action: 'state_observation_only' });
      return;
    }
    await this.local(() => this.page.locator('.dashboard-header select').selectOption(locale));
    await expect(this.page.locator('html')).toHaveAttribute('lang', locale);
    this.locale = locale;
    this.record('interface_language', 'PASS', { locale, local: true });
  }
  async viewport(width: number) {
    await this.page.setViewportSize({ width, height: 900 });
    this.width = width;
  }
  t(key: MessageKey, params?: Record<string, string | number>) { return translate(this.locale as Locale, key, params); }
  async local(action: () => Promise<unknown>) {
    const count = this.frames.filter(o => o.direction === 'out').length;
    await action();
    await this.page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    requireLive(count === this.frames.filter(o => o.direction === 'out').length, 'LOCAL_CONTROL_EMITTED_REQUEST');
  }
  async openSummary(summary: Locator, name: string, expected?: unknown) {
    const verifyValue = arguments.length >= 3;
    const control = await this.describe(summary);
    await this.local(() => summary.click());
    requireLive(await summary.locator('..').getAttribute('open') !== null, 'ORIGINAL_DISCLOSURE_DID_NOT_OPEN');
    const originalTextPresent = (await summary.locator('..').innerText()).length > (await summary.innerText()).length;
    requireLive(originalTextPresent, 'ORIGINAL_DISCLOSURE_EMPTY');
    if (verifyValue) {
      const shown = parseUnique(await summary.locator('..').locator(':scope > pre').innerText());
      requireLive(same(shown, expected), 'ORIGINAL_DISCLOSURE_VALUE_DIFFERS_FROM_SOURCE');
    }
    this.record(name, 'PASS', { local: true, control, original_source: this.originalSource, original_text_present: true, original_value_compared: verifyValue });
    await this.local(() => summary.click());
    requireLive(await summary.locator('..').getAttribute('open') === null, 'ORIGINAL_DISCLOSURE_DID_NOT_CLOSE');
  }
  async safe(name: string, action: () => Promise<void>) {
    this.stage(name);
    try { await action(); }
    catch (error) {
      const code = error instanceof LiveFailure ? error.code : 'BROWSER_OR_ASSERTION_FAILURE';
      this.record(name, 'FAIL', { code, error_type: error instanceof Error ? error.name : 'unknown',
        verdict_scope: 'live check failed; product failure requires original receipt/effect evidence' });
      throw new LiveFailure(code);
    }
  }
  async finish() {
    try {
      if (!this.page.isClosed()) {
        try { this.save('dom-controls.jsonl', { project: this.info.project.name, controls: await controlObservation(this.page), excluded_pages: ['gate', 'egress'] }); }
        catch { this.collectorFailures++; }
        const lock = this.page.getByRole('button', { name: this.t('header.lock'), exact: true });
        if (await lock.isVisible()) await lock.click();
      }
    } finally {
      if (!this.page.isClosed()) await this.page.close();
      this.privateReplies.clear();
      try {
        writeFileSync(join(this.directory, 'end.json'), JSON.stringify({ browser_context_owner: 'Playwright fixture',
          session_cleanup_attempted: true, decoder_failures: this.decoderFailures, page_errors: this.pageErrors,
          page_closed: this.page.isClosed(), collector_failures: this.collectorFailures,
          evidence_scope: 'private directory 0700 / files 0600; business originals may contain restricted material',
          vm_retirement_owner: 'root original run-vm/retire.py', service_stop_requested: false }) + '\n', { mode: 0o600 });
      } catch { this.collectorFailures++; }
      this.info.annotations.push({ type: 'live_observer_failures', description: String(this.collectorFailures) });
    }
  }
}

export const test = base.extend<{ live: LiveObservation; t: (key: MessageKey, params?: Record<string, string | number>) => string }, { liveServer: PreviewServer }>({
  // Playwright 1.60's webServer timeout:0 silently selects its 60s default.
  // Keep the original build, then use Vite's actual listen/close promises.
  liveServer: [async ({}, use) => {
    const url = new URL(process.env.AS_LIVE_HTTP_URL!);
    requireLive(url.protocol === 'http:' && url.hostname === '127.0.0.1' && !!url.port && url.pathname === '/', 'OWNED_HTTP_ENDPOINT_INVALID');
    await execute('npm', ['run', 'build'], { cwd: resolve(repo, 'ui/dashboard'),
      env: { ...process.env, VITE_AS_WS_URL: process.env.AS_LIVE_WS_URL }, maxBuffer: 8 * 1024 * 1024 });
    const server = await preview({ root: resolve(repo, 'ui/dashboard'), preview: { host: url.hostname, port: Number(url.port), strictPort: true } });
    try { await use(server); }
    finally { await server.close(); }
  }, { scope: 'worker' }],
  t: async ({ locale }, use) => { await use((key, params) => translate(locale as Locale, key, params)); },
  live: async ({ page, locale, liveServer }, use, info) => {
    const live = new LiveObservation(page, info, liveServer);
    try { await live.initialize(locale!); await use(live); }
    finally { await live.finish(); }
  },
});

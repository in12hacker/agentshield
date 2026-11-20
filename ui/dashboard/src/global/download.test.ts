import { afterEach, describe, expect, it, vi } from 'vitest';
import { downloadBlob } from './download';

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

function browser(failure?: 'create' | 'append' | 'click') {
  const fail = () => { throw new Error('simulated_browser_failure'); };
  const revokeObjectURL = vi.fn();
  const link = { href: '', download: '', remove: vi.fn(), click: vi.fn(failure === 'click' ? fail : () => {
    expect(revokeObjectURL).not.toHaveBeenCalled();
  }) };
  const append = vi.fn(failure === 'append' ? fail : () => {});
  const createObjectURL = vi.fn(failure === 'create' ? fail : () => 'blob:simulated');
  vi.stubGlobal('document', { createElement: vi.fn(() => link), body: { append } });
  vi.stubGlobal('URL', { createObjectURL, revokeObjectURL });
  return { link, append, createObjectURL, revokeObjectURL };
}

describe('shared Blob download browser operation', () => {
  it.each(['application/json', 'application/pkix-cert'])('keeps %s bytes and URL live through click, then removes and releases once', async type => {
    vi.useFakeTimers();
    const b = browser(), blob = new Blob([new Uint8Array([0, 255, 48, 128])], { type });
    const filename = type === 'application/json' ? 'agentshield-event.json' : 'agentshield-ca.der';
    downloadBlob(blob, filename);
    expect(b.createObjectURL).toHaveBeenCalledExactlyOnceWith(blob);
    expect(b.append).toHaveBeenCalledExactlyOnceWith(b.link);
    expect(b.link.href).toBe('blob:simulated'); expect(b.link.download).toBe(filename);
    expect(b.link.click).toHaveBeenCalledTimes(1); expect(b.link.remove).toHaveBeenCalledTimes(1);
    expect(b.revokeObjectURL).not.toHaveBeenCalled();
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(new Uint8Array([0, 255, 48, 128]));
    vi.runAllTimers(); expect(b.revokeObjectURL).toHaveBeenCalledExactlyOnceWith('blob:simulated');
  });

  it.each(['create', 'append', 'click'] as const)('propagates %s failure, removes the link and releases any allocated URL', failure => {
    vi.useFakeTimers();
    const b = browser(failure);
    expect(() => downloadBlob(new Blob(['{}'], { type: 'application/json' }), 'agentshield-event.json')).toThrow('simulated_browser_failure');
    expect(b.link.remove).toHaveBeenCalledTimes(1);
    expect(b.revokeObjectURL).toHaveBeenCalledTimes(failure === 'create' ? 0 : 1);
    vi.runAllTimers(); expect(b.revokeObjectURL).toHaveBeenCalledTimes(failure === 'create' ? 0 : 1);
  });
});

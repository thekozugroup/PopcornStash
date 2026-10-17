import {describe, expect, it} from 'vitest';
import {BrowserIdle, BROWSER_IDLE_MS} from './browserIdle';

describe('browser inactivity', () => {
  it('locks at fifteen minutes even when background checks continue', () => {
    const idle = new BrowserIdle(0);
    for (let now = 60_000; now < BROWSER_IDLE_MS; now += 60_000) expect(idle.check(now)).toBe(false);
    expect(idle.check(BROWSER_IDLE_MS)).toBe(true);
    expect(idle.input(BROWSER_IDLE_MS)).toBe(false);
  });
  it('uses the server deadline after reload and cannot revive from delayed replies', () => {
    const idle = new BrowserIdle(0, {server_time: 1000, idle_expires_at: 1005});
    expect(idle.remaining(0)).toBe(5000);
    expect(idle.check(5000)).toBe(true);
    idle.acknowledge(5001, {server_time: 1005, idle_expires_at: 1905});
    expect(idle.remaining(5001)).toBe(0);
    expect(idle.input(5002)).toBe(false);
  });
  it('throttles input reports and shares acknowledged activity without extending on checks', () => {
    const idle = new BrowserIdle(0);
    expect(idle.input(1000)).toBe(true);
    expect(idle.input(1001)).toBe(false);
    idle.acknowledge(1100, {server_time: 1, idle_expires_at: 901});
    expect(idle.remaining(1100)).toBe(BROWSER_IDLE_MS);
    expect(idle.input(11_000)).toBe(true);
  });
});

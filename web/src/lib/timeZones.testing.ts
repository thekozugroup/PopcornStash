/**
 * Test helper: run code as a viewer in another time zone.
 *
 * Node applies a new `process.env.TZ` to Date and Intl at once, so a test can
 * walk the zones below in one run. Each zone is checked before use (Los Angeles
 * really is behind UTC on June 1), so a runtime that ignored the change would
 * fail the test instead of passing it by accident.
 */
import {expect} from 'vitest';

/** The test runner's environment (Node); the app's own types know no `process`. */
const env = (globalThis as unknown as {process: {env: Record<string, string | undefined>}}).process.env;

/** West of UTC, east of it, both extremes, a half hour, and UTC itself. */
export const VIEWER_ZONES = [
  'UTC',
  'America/Los_Angeles',
  'America/New_York',
  'Pacific/Pago_Pago',
  'Pacific/Auckland',
  'Pacific/Kiritimati',
  'Asia/Kolkata',
  'Europe/London',
] as const;

/** Minutes the zone is behind UTC on 2024-06-01, as Date.getTimezoneOffset reports it. */
const JUNE_OFFSETS: Record<(typeof VIEWER_ZONES)[number], number> = {
  UTC: 0,
  'America/Los_Angeles': 420,
  'America/New_York': 240,
  'Pacific/Pago_Pago': 660,
  'Pacific/Auckland': -720,
  'Pacific/Kiritimati': -840,
  'Asia/Kolkata': -330,
  'Europe/London': -60,
};

/** Runs `run` with the process in `zone`, then puts the zone back. */
export function inZone<T>(zone: (typeof VIEWER_ZONES)[number], run: () => T): T {
  const before = env.TZ;
  env.TZ = zone;
  try {
    expect(new Date(Date.UTC(2024, 5, 1)).getTimezoneOffset(), `the process is in ${zone}`).toBe(JUNE_OFFSETS[zone]);
    return run();
  } finally {
    if (before === undefined) delete env.TZ;
    else env.TZ = before;
  }
}

/** The same result in every zone: computed in UTC, then compared in each of the others. */
export function sameInEveryZone<T>(run: () => T): T {
  const reference = inZone('UTC', run);
  for (const zone of VIEWER_ZONES) {
    expect(inZone(zone, run), `in ${zone}`).toEqual(reference);
  }
  return reference;
}

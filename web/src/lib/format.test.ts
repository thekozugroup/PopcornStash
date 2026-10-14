/**
 * Capture times on the camera's clock (docs/architecture.md section 4, Rules for
 * clients): a month heading and a photo's time read the same for every viewer,
 * wherever the browser is.
 */
import {describe, expect, it} from 'vitest';
import {
  clockDate,
  formatMonth,
  formatTakenDate,
  formatTakenDateTime,
  formatTakenTime,
  formatUtcOffset,
  takenSortKey,
} from './format';
import {inZone, sameInEveryZone} from './timeZones.testing';

const english = new Intl.DateTimeFormat().resolvedOptions().locale.startsWith('en-US');

describe('month headings', () => {
  it('name the month as written, never the one before it west of UTC', () => {
    const months = sameInEveryZone(() => ['2024-06', '2024-07', '2024-01', '2023-12'].map(formatMonth));
    // The month a viewer anywhere would agree on: mid-month at noon, formatted in UTC.
    const midMonth = (year: number, month: number) =>
      new Intl.DateTimeFormat(undefined, {month: 'long', year: 'numeric', timeZone: 'UTC'}).format(
        Date.UTC(year, month - 1, 15, 12),
      );
    expect(months).toEqual([midMonth(2024, 6), midMonth(2024, 7), midMonth(2024, 1), midMonth(2023, 12)]);
    if (english) expect(months).toEqual(['June 2024', 'July 2024', 'January 2024', 'December 2023']);
  });

  it('would have said May in Los Angeles the old way, which is what this guards', () => {
    inZone('America/Los_Angeles', () => {
      const old = new Intl.DateTimeFormat(undefined, {month: 'long', year: 'numeric'}).format(new Date(Date.UTC(2024, 5, 1)));
      expect(old).not.toBe(formatMonth('2024-06'));
    });
  });

  it('leaves text that is not a month alone', () => {
    expect(formatMonth('Undated')).toBe('Undated');
    expect(formatMonth('2024-13')).toBe('2024-13');
    expect(formatMonth('2024-6')).toBe('2024-6');
  });
});

describe("the camera's clock", () => {
  // A Fujifilm with no offset, just after midnight on July 1 (filed under 2024/07).
  const midnight = '2024-07-01T00:20:00';
  // A Pixel at 8:18 PM in London, with its +01:00 offset.
  const evening = '2024-06-14T20:18:33';

  it('shows the date and time as written in every time zone', () => {
    const shown = sameInEveryZone(() => [
      formatTakenDate(midnight),
      formatTakenDateTime(midnight),
      formatTakenTime(midnight),
      formatTakenDate(evening),
      formatTakenDateTime(evening),
      formatTakenTime(evening),
    ]);
    if (english) {
      expect(shown).toEqual([
        'Jul 1, 2024',
        'Jul 1, 2024, 12:20 AM',
        '12:20 AM',
        'Jun 14, 2024',
        'Jun 14, 2024, 8:18 PM',
        '8:18 PM',
      ]);
    }
  });

  it('reads the fields as given, with no conversion', () => {
    const date = clockDate(midnight);
    expect(date?.getUTCFullYear()).toBe(2024);
    expect(date?.getUTCMonth()).toBe(6);
    expect(date?.getUTCDate()).toBe(1);
    expect(date?.getUTCHours()).toBe(0);
    expect(date?.getUTCMinutes()).toBe(20);
    // A clock without seconds, a date alone and a month.
    expect(clockDate('2024-06-14T20:18')?.toISOString()).toBe('2024-06-14T20:18:00.000Z');
    expect(clockDate('2024-06-14')?.toISOString()).toBe('2024-06-14T00:00:00.000Z');
    expect(clockDate('2024-06')?.toISOString()).toBe('2024-06-01T00:00:00.000Z');
    // Old years stay old (Date.UTC would read 0099 as 1999).
    expect(clockDate('0099-01-01T00:00:00')?.getUTCFullYear()).toBe(99);
  });

  it('refuses what is not a clock instead of showing another date', () => {
    for (const bad of ['', 'yesterday', '2024-06-31T10:00:00', '2024-02-30', '2024-06-14T24:00:00', '2024-06-14T20:61:00', '1718392713']) {
      expect(clockDate(bad), bad).toBeNull();
      expect(formatTakenDateTime(bad), bad).toBe('');
    }
    expect(formatTakenDate(null)).toBe('');
    expect(formatTakenTime(undefined)).toBe('');
  });
});

describe('the offset a camera wrote', () => {
  it('reads as UTC plus or minus hours and minutes', () => {
    expect(formatUtcOffset(60)).toBe('UTC+01:00');
    expect(formatUtcOffset(0)).toBe('UTC+00:00');
    expect(formatUtcOffset(-420)).toBe('UTC-07:00');
    expect(formatUtcOffset(330)).toBe('UTC+05:30');
    expect(formatUtcOffset(-570)).toBe('UTC-09:30');
    expect(formatUtcOffset(null)).toBe('');
  });
});

describe('the sort=taken key', () => {
  it("is the camera's clock, else when the item was added read in UTC", () => {
    const keys = sameInEveryZone(() => [
      takenSortKey({taken_local: '2024-07-01T00:20:00', added_at: 1_790_000_000}),
      // 1719789600 is 2024-06-30 23:20:00 UTC.
      takenSortKey({taken_local: null, added_at: 1_719_789_600}),
    ]);
    expect(keys).toEqual(['2024-07-01T00:20:00', '2024-06-30T23:20:00']);
  });
});

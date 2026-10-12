/**
 * Formatting helpers for sizes, rates, durations and dates.
 *
 * Plain words, sentence case, the browser's locale for numbers and dates.
 * Sizes use decimal units (1 GB = 1000 MB), matching what disks and most
 * operating systems report.
 */

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const;

function numberFormat(maximumFractionDigits: number): Intl.NumberFormat {
  return new Intl.NumberFormat(undefined, {maximumFractionDigits, minimumFractionDigits: 0});
}

/** 0 B, 812 KB, 4.7 GB, 1.21 TB */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return 'Unknown size';
  if (bytes < 1000) return `${Math.round(bytes)} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${numberFormat(digits).format(value)} ${BYTE_UNITS[unit]}`;
}

/** "3.4 GB of 7.9 GB" */
export function formatBytesOf(done: number, total: number | null | undefined): string {
  if (total === null || total === undefined || total <= 0) return formatBytes(done);
  return `${formatBytes(done)} of ${formatBytes(total)}`;
}

/** 4.2 MB/s. Zero and missing rates read as "Idle". */
export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  if (!bytesPerSecond || bytesPerSecond <= 0 || !Number.isFinite(bytesPerSecond)) return 'Idle';
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Media runtime in words: "1 h 42 min", "42 min", "38 s". */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '';
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (hours > 0) return minutes > 0 ? `${hours} h ${minutes} min` : `${hours} h`;
  if (minutes > 0) return `${minutes} min`;
  return `${total} s`;
}

/** Player clock: "1:42:05", "42:05", "0:38". */
export function formatClock(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '0:00';
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const ss = String(secs).padStart(2, '0');
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, '0')}:${ss}`;
  return `${minutes}:${ss}`;
}

/** Remaining time for downloads: "About 3 min left", "Less than a minute left". */
export function formatEta(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '';
  if (seconds < 60) return 'Less than a minute left';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `About ${minutes} min left`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours < 48) return rest > 0 ? `About ${hours} h ${rest} min left` : `About ${hours} h left`;
  return `About ${Math.round(hours / 24)} days left`;
}

function toDate(unixSeconds: number): Date {
  return new Date(unixSeconds * 1000);
}

/** "12 Oct 2026" (locale order). */
export function formatDate(unixSeconds: number | null | undefined): string {
  if (unixSeconds === null || unixSeconds === undefined) return '';
  return new Intl.DateTimeFormat(undefined, {dateStyle: 'medium'}).format(toDate(unixSeconds));
}

/** "12 Oct 2026, 14:05" (locale order). */
export function formatDateTime(unixSeconds: number | null | undefined): string {
  if (unixSeconds === null || unixSeconds === undefined) return '';
  return new Intl.DateTimeFormat(undefined, {dateStyle: 'medium', timeStyle: 'short'}).format(
    toDate(unixSeconds),
  );
}

const RELATIVE_STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['second', 60],
  ['minute', 60],
  ['hour', 24],
  ['day', 7],
  ['week', 4.34524],
  ['month', 12],
  ['year', Number.POSITIVE_INFINITY],
];

/** "Just now", "5 minutes ago", "Yesterday", "In 2 days". */
export function formatRelative(unixSeconds: number | null | undefined, now: number = Date.now()): string {
  if (unixSeconds === null || unixSeconds === undefined) return '';
  let delta = unixSeconds - now / 1000;
  if (Math.abs(delta) < 45) return 'Just now';
  const rtf = new Intl.RelativeTimeFormat(undefined, {numeric: 'auto'});
  for (const [unit, size] of RELATIVE_STEPS) {
    if (Math.abs(delta) < size) {
      const text = rtf.format(Math.round(delta), unit);
      return text.charAt(0).toLocaleUpperCase() + text.slice(1);
    }
    delta /= size;
  }
  return formatDate(unixSeconds);
}

/** "2024-06" -> "June 2024". */
export function formatMonth(month: string): string {
  const [year, monthIndex] = month.split('-').map(Number);
  if (!year || !monthIndex) return month;
  return new Intl.DateTimeFormat(undefined, {month: 'long', year: 'numeric'}).format(
    new Date(Date.UTC(year, monthIndex - 1, 1)),
  );
}

/** 0.423 -> "42%". */
export function formatPercent(fraction: number | null | undefined): string {
  if (fraction === null || fraction === undefined || !Number.isFinite(fraction)) return '';
  const clamped = Math.min(1, Math.max(0, fraction));
  return new Intl.NumberFormat(undefined, {style: 'percent', maximumFractionDigits: 0}).format(clamped);
}

/** formatCount(1, 'movie') -> "1 movie"; formatCount(3, 'movie') -> "3 movies". */
export function formatCount(count: number, singular: string, plural = `${singular}s`): string {
  return `${new Intl.NumberFormat().format(count)} ${count === 1 ? singular : plural}`;
}

/** Season and episode code: "S01E02". */
export function formatEpisodeCode(season: number | null | undefined, episode: number | null | undefined): string {
  if (season === null || season === undefined) return '';
  const s = `S${String(season).padStart(2, '0')}`;
  if (episode === null || episode === undefined) return s;
  return `${s}E${String(episode).padStart(2, '0')}`;
}

/** Watched fraction for a progress hairline, or null when there is nothing to show. */
export function watchedFraction(
  progress: {position_s: number; duration_s: number; finished: boolean} | null | undefined,
): number | null {
  if (!progress || progress.finished || progress.duration_s <= 0) return null;
  const fraction = progress.position_s / progress.duration_s;
  if (fraction <= 0.01) return null;
  return Math.min(1, fraction);
}

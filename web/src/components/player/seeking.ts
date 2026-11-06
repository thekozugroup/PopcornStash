/** The server samples at most 200 positions per video, at least two seconds apart. */
export function previewSecond(position: number, duration: number): number {
  if (!Number.isFinite(position) || !Number.isFinite(duration) || duration <= 0) return 0;
  const interval = Math.max(2, Math.ceil(duration / 200));
  return Math.floor(Math.max(0, Math.min(position, Math.max(0, duration - 0.1))) / interval) * interval;
}

export function positionAt(clientX: number, left: number, width: number, duration: number, rtl: boolean): number {
  const fraction = Math.max(0, Math.min(1, (clientX - left - 10) / Math.max(1, width - 20)));
  return Math.round((rtl ? 1 - fraction : fraction) * Math.max(0, duration));
}

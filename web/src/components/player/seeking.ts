/** The server samples at most 200 positions per video, at least two seconds apart. */
export function previewSecond(position: number, duration: number): number {
  if (!Number.isFinite(position) || !Number.isFinite(duration) || duration <= 0) return 0;
  const interval = Math.max(2, Math.ceil(duration / 200));
  return Math.floor(Math.max(0, Math.min(position, Math.max(0, duration - 0.1))) / interval) * interval;
}

// Astryx insets each end of the thumb's travel by half its 20px diameter.
const THUMB_INSET = 10;

export function positionAt(clientX: number, left: number, width: number, duration: number, rtl: boolean): number {
  if (!Number.isFinite(duration) || duration <= 0 || width <= 0) return 0;
  const inset = width > THUMB_INSET * 2 ? THUMB_INSET : 0;
  const offset = rtl ? left + width - clientX : clientX - left;
  const fraction = Math.max(0, Math.min(1, (offset - inset) / (width - 2 * inset)));
  return fraction * duration;
}

export interface SeekTrack {
  left: number; // Track offset from the physical left edge of the control strip.
  width: number;
  available: number;
  rtl: boolean;
}

export function previewPlacement(position: number, duration: number, track: SeekTrack) {
  const width = Math.min(track.available, track.available < 330 ? 96 : track.available < 600 ? 128 : 160);
  const fraction = duration > 0 ? Math.max(0, Math.min(1, position / duration)) : 0;
  const inset = track.width > THUMB_INSET * 2 ? THUMB_INSET : 0;
  const center = track.left + inset + (track.rtl ? 1 - fraction : fraction) * (track.width - 2 * inset);
  return {width, left: Math.max(0, Math.min(track.available - width, center - width / 2))};
}

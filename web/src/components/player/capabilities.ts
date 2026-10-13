/**
 * What this browser can decode, in the vocabulary of `POST
 * /api/items/:id/playback` (architecture section 5, playback decision):
 * video h264, hevc, av1, vp9; audio aac, mp3, opus, flac, ac3, eac3; and
 * whether the screen shows HDR. The server plays the original file directly
 * when it fits and transcodes to HLS when it does not.
 */

export interface PlaybackCaps {
  video_codecs: string[];
  audio_codecs: string[];
  hdr: boolean;
}

const VIDEO_PROBES: Record<string, string[]> = {
  h264: ['video/mp4; codecs="avc1.640028"', 'video/mp4; codecs="avc1.42E01E"'],
  hevc: ['video/mp4; codecs="hvc1.2.4.L153.B0"', 'video/mp4; codecs="hev1.2.4.L153.B0"', 'video/mp4; codecs="hvc1.1.6.L120.90"'],
  av1: ['video/mp4; codecs="av01.0.08M.08"', 'video/webm; codecs="av01.0.08M.08"'],
  vp9: ['video/webm; codecs="vp9"', 'video/webm; codecs="vp09.00.40.08"', 'video/mp4; codecs="vp09.00.40.08"'],
};

const AUDIO_PROBES: Record<string, string[]> = {
  aac: ['audio/mp4; codecs="mp4a.40.2"'],
  mp3: ['audio/mpeg', 'audio/mp4; codecs="mp3"'],
  opus: ['audio/webm; codecs="opus"', 'audio/mp4; codecs="opus"', 'audio/ogg; codecs="opus"'],
  flac: ['audio/flac', 'audio/mp4; codecs="flac"'],
  ac3: ['audio/mp4; codecs="ac-3"'],
  eac3: ['audio/mp4; codecs="ec-3"'],
};

type MediaSourceLike = {isTypeSupported?: (type: string) => boolean};

function mediaSourceSupports(type: string): boolean {
  const scope = globalThis as unknown as {MediaSource?: MediaSourceLike; ManagedMediaSource?: MediaSourceLike};
  for (const source of [scope.MediaSource, scope.ManagedMediaSource]) {
    try {
      if (source?.isTypeSupported?.(type)) return true;
    } catch {
      // Some browsers throw on unknown codec strings.
    }
  }
  return false;
}

function supports(element: HTMLMediaElement, types: string[]): boolean {
  return types.some(type => {
    if (mediaSourceSupports(type)) return true;
    try {
      return element.canPlayType(type) !== '';
    } catch {
      return false;
    }
  });
}

function screenIsHdr(): boolean {
  try {
    return window.matchMedia('(dynamic-range: high)').matches;
  } catch {
    return false;
  }
}

/** Detects decodable codecs once per element. H.264 is always included, per the contract. */
export function detectPlaybackCaps(element?: HTMLMediaElement | null): PlaybackCaps {
  const probe = element ?? document.createElement('video');
  const video = Object.keys(VIDEO_PROBES).filter(codec => codec === 'h264' || supports(probe, VIDEO_PROBES[codec] ?? []));
  const audio = Object.keys(AUDIO_PROBES).filter(codec => supports(probe, AUDIO_PROBES[codec] ?? []));
  return {video_codecs: video, audio_codecs: audio.length ? audio : ['aac'], hdr: screenIsHdr()};
}

/** The most widely supported pair, used to retry when direct play fails. */
export const SAFE_CAPS: PlaybackCaps = {video_codecs: ['h264'], audio_codecs: ['aac'], hdr: false};

/** True when the browser plays HLS without hls.js (Safari, iOS, some Chrome builds). */
export function hasNativeHls(element: HTMLMediaElement): boolean {
  try {
    return element.canPlayType('application/vnd.apple.mpegurl') !== '';
  } catch {
    return false;
  }
}

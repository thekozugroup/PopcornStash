/**
 * Plain-word labels for library items: where a category lives, the one
 * metadata line under a tile, codec summaries in metadata style, and the
 * folder an item would move to when organized (architecture section 3).
 */
import {items as itemsApi} from '../../api/client';
import type {
  Item,
  ItemCategory,
  ItemKind,
  MediaAudioStream,
  MediaSubtitleStream,
  MediaSummary,
  MediaVideoStream,
} from '../../api/types';
import {formatBytes, formatCount, formatDate, formatDuration, formatEpisodeCode} from '../../lib/format';

export const CATEGORY_PATH: Record<ItemCategory, string> = {
  movie: '/movies',
  episode: '/shows',
  home_video: '/home-videos',
  photo: '/photos',
  other: '/other',
};

export const CATEGORY_LABEL: Record<ItemCategory, string> = {
  movie: 'Movies',
  episode: 'Shows',
  home_video: 'Home Videos',
  photo: 'Photos',
  other: 'Other',
};

/** Top-level folders of the library (architecture section 3). */
export const CATEGORY_FOLDER: Record<ItemCategory, string> = {
  movie: 'Movies',
  episode: 'Shows',
  home_video: 'Home Videos',
  photo: 'Photos',
  other: 'Other',
};

const LIBRARY_FOLDERS = new Set(Object.values(CATEGORY_FOLDER));

/** True when the file already sits in one of the five library folders. */
export function isInLibraryFolder(relPath: string): boolean {
  return LIBRARY_FOLDERS.has(relPath.split('/')[0] ?? '');
}

/** "2 seasons · 14 episodes", or just the episodes for a single season. */
export function showMeta(show: {season_count: number; item_count: number}): string {
  const episodes = formatCount(show.item_count, 'episode');
  return show.season_count > 1 ? `${formatCount(show.season_count, 'season')} · ${episodes}` : episodes;
}

export function itemHref(item: Pick<Item, 'id'>): string {
  return `/item/${encodeURIComponent(item.id)}`;
}

export function showHref(showName: string): string {
  return `/shows/${encodeURIComponent(showName)}`;
}

/** Where "back" goes from an item: its show, or its category page. */
export function itemParent(item: Pick<Item, 'category' | 'show_name'>): {href: string; label: string} {
  if (item.category === 'episode' && item.show_name) {
    return {href: showHref(item.show_name), label: item.show_name};
  }
  return {href: CATEGORY_PATH[item.category], label: CATEGORY_LABEL[item.category]};
}

/**
 * True when a photo's title is still its camera file name ("PXL_20240613_…"),
 * which means nothing to the family; the date it was taken says more.
 */
export function hasFileNameTitle(item: Pick<Item, 'title' | 'rel_path' | 'category' | 'taken_at'>): boolean {
  if (item.category !== 'photo' || !item.taken_at) return false;
  const name = fileName(item.rel_path);
  const dot = name.lastIndexOf('.');
  return item.title === (dot > 0 ? name.slice(0, dot) : name);
}

/** The name people know an item by: episode titles for episodes, the date for camera-named photos. */
export function displayTitle(
  item: Pick<Item, 'title' | 'episode_title' | 'category' | 'rel_path' | 'taken_at'>,
): string {
  if (item.category === 'episode' && item.episode_title) return item.episode_title;
  if (hasFileNameTitle(item)) return `Photo from ${formatDate(item.taken_at)}`;
  return item.title;
}

/**
 * An episode's name inside its show, where the show name and season are
 * already on screen: its own title, or "Episode 3" ("Special 1" for season 0)
 * rather than "The Lantern Keepers S01E03" again.
 */
export function episodeName(item: Pick<Item, 'title' | 'episode_title' | 'season' | 'episode'>): string {
  if (item.episode_title) return item.episode_title;
  if (item.episode === null) return item.title;
  return item.season === 0 ? `Special ${item.episode}` : `Episode ${item.episode}`;
}

/** "3:28 PM" in the viewer's own clock style. */
export function formatTime(unixSeconds: number): string {
  return new Intl.DateTimeFormat(undefined, {timeStyle: 'short'}).format(new Date(unixSeconds * 1000));
}

/** File extension in capitals ("PDF", "MP3"), or null. */
export function fileType(relPath: string): string | null {
  const name = relPath.split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toUpperCase();
}

export function fileName(relPath: string): string {
  return relPath.split('/').pop() ?? relPath;
}

/** One metadata line for a tile: year and runtime, date, or type and size. */
export function itemMeta(item: Item): string {
  switch (item.category) {
    case 'movie':
      return [item.year ? String(item.year) : null, formatDuration(item.duration_s)].filter(Boolean).join(' · ');
    case 'episode':
      return [formatEpisodeCode(item.season, item.episode), formatDuration(item.duration_s)].filter(Boolean).join(' · ');
    case 'home_video':
      return [formatDate(item.taken_at ?? item.added_at), formatDuration(item.duration_s)].filter(Boolean).join(' · ');
    case 'photo':
      // A camera-named photo already shows its date as the title.
      return hasFileNameTitle(item) ? (item.camera ?? '') : formatDate(item.taken_at ?? item.added_at);
    case 'other':
    default: {
      const type = fileType(item.rel_path);
      const amount = item.duration_s ? formatDuration(item.duration_s) : formatBytes(item.size);
      return [type, amount].filter(Boolean).join(' · ');
    }
  }
}

/**
 * Where to pick up: a partly watched item. "Almost over" is the server's
 * `finished` (90% watched), the same rule that keeps an item off "Continue
 * watching", so every item on that shelf resumes, short clips included.
 */
export function resumePosition(item: Pick<Item, 'progress' | 'duration_s'>): number {
  const progress = item.progress;
  if (!progress || progress.finished) return 0;
  const duration = progress.duration_s || item.duration_s || 0;
  if (progress.position_s < 15 || (duration && progress.position_s > duration - 2)) return 0;
  return progress.position_s;
}

/** Time left in words for a partly watched item: "24 min left". */
export function timeLeft(item: Pick<Item, 'progress' | 'duration_s'>): string {
  const duration = item.progress?.duration_s || item.duration_s || 0;
  const position = item.progress?.position_s ?? 0;
  const left = duration - position;
  if (!duration || left <= 0) return '';
  if (left < 60) return 'Less than a minute left';
  return `${formatDuration(left)} left`;
}

export interface ThumbSources {
  src: string;
  srcSet: string;
}

/**
 * Small and large thumbnail URLs for `srcset`, or null while the thumbnail
 * is not ready. `version` busts the browser cache after a live update.
 */
export function thumbSources(item: Pick<Item, 'id' | 'thumb'>, version?: number): ThumbSources | null {
  return item.thumb ? sourcesFor(item.id, version) : null;
}

function sourcesFor(id: string, version?: number): ThumbSources {
  const v = version ? `&v=${version}` : '';
  const sm = `${itemsApi.thumbUrl(id, 'sm')}${v}`;
  const lg = `${itemsApi.thumbUrl(id, 'lg')}${v}`;
  return {src: sm, srcSet: `${sm} 400w, ${lg} 1280w`};
}

/**
 * A show's poster from the server's choice (`poster_item_id` in the shows list,
 * the show's page and Home's `show_posters`), so a show has one picture
 * everywhere. A poster without a thumbnail yet fails to load and the tile
 * falls back to its placeholder.
 */
export function posterSources(posterItemId: string | null | undefined, version?: number): ThumbSources | null {
  return posterItemId ? sourcesFor(posterItemId, version) : null;
}

/** The large thumbnail alone (player poster, show hero). */
export function largeThumb(item: Pick<Item, 'id' | 'thumb'>, version?: number): string | null {
  if (!item.thumb) return null;
  return `${itemsApi.thumbUrl(item.id, 'lg')}${version ? `&v=${version}` : ''}`;
}

// ---------------------------------------------------------------------------
// Codec summaries
// ---------------------------------------------------------------------------

const VIDEO_CODECS: Record<string, string> = {
  h264: 'H.264',
  hevc: 'HEVC',
  h265: 'HEVC',
  av1: 'AV1',
  vp9: 'VP9',
  vp8: 'VP8',
  mpeg4: 'MPEG-4',
  mpeg2video: 'MPEG-2',
  prores: 'ProRes',
};

const AUDIO_CODECS: Record<string, string> = {
  aac: 'AAC',
  mp3: 'MP3',
  opus: 'Opus',
  flac: 'FLAC',
  vorbis: 'Vorbis',
  ac3: 'Dolby Digital',
  eac3: 'Dolby Digital Plus',
  truehd: 'Dolby TrueHD',
  dts: 'DTS',
  pcm_s16le: 'PCM',
  pcm_s24le: 'PCM',
  alac: 'Apple Lossless',
};

const SUBTITLE_CODECS: Record<string, string> = {
  subrip: 'SRT',
  ass: 'ASS',
  ssa: 'SSA',
  webvtt: 'WebVTT',
  mov_text: 'Text',
  hdmv_pgs_subtitle: 'PGS',
  dvd_subtitle: 'VobSub',
};

const HDR_NAMES: Record<string, string> = {
  hdr10: 'HDR10',
  pq: 'HDR10',
  'hdr10+': 'HDR10+',
  hlg: 'HLG',
  dolby_vision: 'Dolby Vision',
  dovi: 'Dolby Vision',
};

const ISO_639_2: Record<string, string> = {
  eng: 'en', spa: 'es', fra: 'fr', fre: 'fr', deu: 'de', ger: 'de', ita: 'it', jpn: 'ja', por: 'pt',
  nld: 'nl', dut: 'nl', swe: 'sv', nor: 'no', nob: 'nb', dan: 'da', fin: 'fi', pol: 'pl', rus: 'ru',
  kor: 'ko', zho: 'zh', chi: 'zh', ara: 'ar', hin: 'hi', tur: 'tr', ces: 'cs', cze: 'cs', ell: 'el',
  gre: 'el', heb: 'he', hun: 'hu', ukr: 'uk', ron: 'ro', rum: 'ro', tha: 'th', vie: 'vi', ind: 'id',
};

let languageNames: Intl.DisplayNames | null | undefined;

/** "eng" -> "English"; null for unknown or undetermined. */
export function languageName(code: string | null | undefined): string | null {
  if (!code || code === 'und' || code === 'mul' || code === 'zxx') return null;
  const tag = ISO_639_2[code.toLowerCase()] ?? code;
  if (languageNames === undefined) {
    try {
      languageNames = new Intl.DisplayNames(undefined, {type: 'language'});
    } catch {
      languageNames = null;
    }
  }
  try {
    const name = languageNames?.of(tag);
    return name && name !== tag ? name : code.toUpperCase();
  } catch {
    return code.toUpperCase();
  }
}

export function videoCodecName(codec: string): string {
  return VIDEO_CODECS[codec.toLowerCase()] ?? codec.toUpperCase();
}

export function audioCodecName(codec: string): string {
  return AUDIO_CODECS[codec.toLowerCase()] ?? codec.toUpperCase();
}

export function channelsName(channels: number | null): string | null {
  if (!channels) return null;
  if (channels === 1) return 'mono';
  if (channels === 2) return 'stereo';
  if (channels === 6) return '5.1';
  if (channels === 8) return '7.1';
  return `${channels} channels`;
}

/** "4K", "1080p", "720p" from the frame size (wide films are judged by width). */
export function resolutionName(width: number | null | undefined, height: number | null | undefined): string | null {
  if (!width || !height) return null;
  const long = Math.max(width, height);
  const short = Math.min(width, height);
  if (long >= 3800) return '4K';
  if (long >= 2500) return '1440p';
  if (long >= 1900 || short >= 1060) return '1080p';
  if (long >= 1260 || short >= 700) return '720p';
  return `${short}p`;
}

export function hdrName(hdr: string | null | undefined): string | null {
  if (!hdr) return null;
  return HDR_NAMES[hdr.toLowerCase()] ?? 'HDR';
}

/** "4K HDR10, HEVC" */
export function videoSummary(video: MediaVideoStream | null): string | null {
  if (!video) return null;
  const quality = [resolutionName(video.width, video.height), hdrName(video.hdr)].filter(Boolean).join(' ');
  return [quality, videoCodecName(video.codec)].filter(Boolean).join(', ');
}

/** "English, Dolby Digital Plus 5.1" or the track's own title. */
export function audioTrackLabel(track: MediaAudioStream, position: number): string {
  const language = languageName(track.language);
  const base = track.title ?? language ?? `Track ${position + 1}`;
  return track.title && language && !track.title.includes(language) ? `${base} (${language})` : base;
}

export function audioTrackDetail(track: MediaAudioStream): string {
  return [audioCodecName(track.codec), channelsName(track.channels)].filter(Boolean).join(' ');
}

/** First track in words, plus how many more. */
export function audioSummary(audio: MediaAudioStream[]): string | null {
  const [first] = audio;
  if (!first) return null;
  const language = languageName(first.language);
  const head = [language, audioTrackDetail(first)].filter(Boolean).join(', ');
  return audio.length > 1 ? `${head}, ${audio.length - 1} more` : head;
}

export function subtitleLabel(track: MediaSubtitleStream): string {
  const language = languageName(track.language) ?? 'Unknown language';
  const parts = [track.title ?? language, SUBTITLE_CODECS[track.codec.toLowerCase()] ?? null];
  if (track.external) parts.push('separate file');
  return parts.filter(Boolean).join(', ');
}

export function subtitleSummary(subtitles: MediaSubtitleStream[]): string | null {
  if (subtitles.length === 0) return null;
  const languages = [...new Set(subtitles.map(track => languageName(track.language) ?? 'Unknown'))];
  return languages.length > 3 ? `${languages.slice(0, 3).join(', ')} and ${languages.length - 3} more` : languages.join(', ');
}

export function bitrateName(bitsPerSecond: number | null | undefined): string | null {
  if (!bitsPerSecond || bitsPerSecond <= 0) return null;
  if (bitsPerSecond >= 1e6) return `${(bitsPerSecond / 1e6).toFixed(bitsPerSecond >= 1e7 ? 0 : 1)} Mbps`;
  return `${Math.round(bitsPerSecond / 1000)} kbps`;
}

/** Friendly container name from ffprobe's format list. */
export function containerName(media: MediaSummary | null, relPath: string): string | null {
  const type = fileType(relPath);
  if (type) return type;
  return media?.container.split(',')[0]?.toUpperCase() ?? null;
}

// ---------------------------------------------------------------------------
// Organize destinations (architecture section 3)
// ---------------------------------------------------------------------------

/** Categories an item of this kind can be organized into. */
export function organizeCategories(kind: ItemKind): ItemCategory[] {
  if (kind === 'video') return ['movie', 'episode', 'home_video', 'other'];
  if (kind === 'photo') return ['photo', 'other'];
  return ['other'];
}

export const CATEGORY_SINGULAR: Record<ItemCategory, string> = {
  movie: 'Movie',
  episode: 'Episode',
  home_video: 'Home video',
  photo: 'Photo',
  other: 'Other',
};

/** Characters that cannot appear in a file or folder name on common systems. */
function safeName(value: string): string {
  return value
    .replace(/[\\/:*?"<>|]/g, ' ')
    .replace(/\p{Cc}/gu, '')
    .replace(/\s+/g, ' ')
    .trim();
}

export interface OrganizeFields {
  category: ItemCategory;
  title: string;
  year: number | null;
  show_name: string;
  season: number | null;
  episode: number | null;
}

/**
 * The relative path an item moves to, following the folder layout in
 * architecture section 3. Returns null until the required fields are filled.
 */
export function destinationFor(
  fields: OrganizeFields,
  item: Pick<Item, 'rel_path' | 'taken_at' | 'added_at'>,
): string | null {
  const name = fileName(item.rel_path);
  const type = fileType(item.rel_path)?.toLowerCase();
  const ext = type ? `.${type}` : '';
  const pad = (value: number) => String(value).padStart(2, '0');
  switch (fields.category) {
    case 'movie': {
      const title = safeName(fields.title);
      if (!title) return null;
      const folder = fields.year ? `${title} (${fields.year})` : title;
      return `Movies/${folder}/${folder}${ext}`;
    }
    case 'episode': {
      const show = safeName(fields.show_name);
      if (!show || fields.season === null || fields.episode === null) return null;
      return `Shows/${show}/Season ${pad(fields.season)}/${show} - S${pad(fields.season)}E${pad(fields.episode)}${ext}`;
    }
    case 'home_video':
    case 'photo': {
      const when = new Date((item.taken_at ?? item.added_at) * 1000);
      const folder = fields.category === 'photo' ? 'Photos' : 'Home Videos';
      return `${folder}/${when.getFullYear()}/${pad(when.getMonth() + 1)}/${name}`;
    }
    case 'other':
    default:
      return `Other/${name}`;
  }
}

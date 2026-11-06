import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Slider} from '@astryxdesign/core/Slider';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {MediaTheme} from '@astryxdesign/core/theme';
import {Maximize, Minimize, Pause, Play, Volume2, VolumeX} from 'lucide-react';
import {useCallback, useEffect, useRef, useState, type RefObject} from 'react';
import {ApiError, notifyUnauthorized} from '../../api/client';
import {formatClock} from '../../lib/format';
import {positionAt, previewSecond} from './seeking';
import './PlayerControls.css';

type SafariVideo = HTMLVideoElement & {webkitEnterFullscreen?: () => void};

const styles = stylex.create({
  chrome: {
    position: 'absolute', insetInline: 12, bottom: 12, paddingInline: 8,
    paddingBlockStart: {default: 0, '@media (max-width: 599px)': 20},
    borderRadius: 'var(--radius-container)', color: colorVars['--color-on-dark'],
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 88%, transparent)',
    display: 'flex', alignItems: 'center', gap: 4,
    opacity: {default: 1, ':focus-within': 1},
  },
  hidden: {opacity: {default: 0, ':focus-within': 1}, pointerEvents: {default: 'none', ':focus-within': 'auto'}},
  timeline: {position: 'relative', flexGrow: 1, minWidth: 0},
  time: {fontSize: 12, fontVariantNumeric: 'tabular-nums', whiteSpace: 'nowrap', paddingInline: 4},
  elapsed: {position: {default: 'static', '@media (max-width: 599px)': 'absolute'}, top: 2, insetInlineStart: 8},
  total: {position: {default: 'static', '@media (max-width: 599px)': 'absolute'}, top: 2, insetInlineEnd: 8},
  volume: {position: 'relative', flexShrink: 0},
  volumePanel: {
    position: 'absolute', bottom: '100%', insetInlineEnd: 0, width: 160,
    padding: 12, borderRadius: 'var(--radius-container)', backgroundColor: 'var(--popcorn-stage)',
  },
  button: {color: 'inherit', flexShrink: 0, minWidth: 44, minHeight: 44},
  preview: {
    position: 'absolute', bottom: '100%', marginBottom: 12, width: 160, maxWidth: '100%',
    borderRadius: 'var(--radius-media)', overflow: 'hidden', pointerEvents: 'none',
    backgroundColor: 'var(--popcorn-stage)', color: 'inherit', textAlign: 'center',
  },
  frame: {width: '100%', aspectRatio: '16 / 9', display: 'grid', placeItems: 'center', fontSize: 12},
  image: {width: '100%', height: '100%', objectFit: 'contain'},
  stamp: {display: 'block', padding: 8, fontSize: 13, fontVariantNumeric: 'tabular-nums'},
});

function SeekFrame({itemId, position, duration}: {itemId: string; position: number; duration: number}) {
  const second = previewSecond(position, duration);
  const key = `/api/items/${encodeURIComponent(itemId)}/preview?time_s=${second}`;
  const [image, setImage] = useState<{key: string; url: string} | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    let url: string | null = null;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          let response = await fetch(key, {signal: abort.signal, credentials: 'same-origin', cache: 'no-store'});
          if (response.status === 409) {
            await new Promise(resolve => setTimeout(resolve, 300));
            if (abort.signal.aborted) return;
            response = await fetch(key, {signal: abort.signal, credentials: 'same-origin', cache: 'no-store'});
          }
          if (response.status === 401) notifyUnauthorized(new ApiError('unauthorized', 'Sign in again.', 401));
          if (!response.ok) throw new Error('Preview unavailable');
          const blob = await response.blob();
          if (abort.signal.aborted) return;
          url = URL.createObjectURL(blob);
          setImage({key, url});
        } catch {
          if (!abort.signal.aborted) setFailed(key);
        }
      })();
    }, 180);
    return () => { clearTimeout(timer); abort.abort(); if (url) URL.revokeObjectURL(url); };
  }, [key]);
  return <>
    <div {...stylex.props(styles.frame)}>
      {image?.key === key ? <img src={image.url} alt="" {...stylex.props(styles.image)} /> : failed === key ? 'Preview unavailable' : 'Loading preview'}
    </div>
    <span {...stylex.props(styles.stamp)}>{formatClock(position)}</span>
  </>;
}

export function PlayerControls({videoRef, itemId, durationHint, onSeek}: {
  videoRef: RefObject<HTMLVideoElement | null>; itemId: string; durationHint: number; onSeek: () => void;
}) {
  const [clock, setClock] = useState({position: 0, duration: durationHint, paused: true, muted: false, volume: 1, canFullscreen: false});
  const [draft, setDraft] = useState<number | null>(null);
  const [preview, setPreview] = useState<number | null>(null);
  const [awake, setAwake] = useState(true);
  const [full, setFull] = useState(false);
  const [volumeOpen, setVolumeOpen] = useState(false);
  const [fullscreenUnavailable, setFullscreenUnavailable] = useState(false);
  const timeline = useRef<HTMLDivElement>(null);
  const [track, setTrack] = useState({left: 0, width: 0, available: 0, rtl: false});
  const pointer = useRef(false);
  const cancelled = useRef(false);
  const sleep = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previewTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reveal = useCallback(() => {
    setAwake(true);
    if (sleep.current) clearTimeout(sleep.current);
    sleep.current = setTimeout(() => setAwake(false), 3000);
  }, []);
  useEffect(() => {
    const element = timeline.current;
    if (!element?.parentElement) return;
    const parent = element.parentElement;
    const observer = new ResizeObserver(() => {
      const box = element.getBoundingClientRect(), outer = parent.getBoundingClientRect();
      setTrack({left: box.left - outer.left, width: box.width, available: outer.width, rtl: getComputedStyle(element).direction === 'rtl'});
    });
    observer.observe(element); observer.observe(parent);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const update = () => setClock({position: video.currentTime, duration: Number.isFinite(video.duration) && video.duration > 0 ? video.duration : durationHint, paused: video.paused, muted: video.muted, volume: video.volume,
      canFullscreen: (typeof video.parentElement?.requestFullscreen === 'function' && document.fullscreenEnabled !== false) || typeof (video as SafariVideo).webkitEnterFullscreen === 'function'});
    const events = ['timeupdate', 'durationchange', 'loadedmetadata', 'play', 'pause', 'ended', 'volumechange'];
    events.forEach(e => video.addEventListener(e, update));
    const wake = () => reveal();
    video.addEventListener('pointermove', wake);
    video.addEventListener('pointerdown', wake);
    const fullscreen = () => setFull(Boolean(document.fullscreenElement));
    document.addEventListener('fullscreenchange', fullscreen);
    const nativeEnd = () => { video.controls = false; setFull(false); };
    const nativeBegin = () => setFull(true);
    video.addEventListener('webkitbeginfullscreen', nativeBegin);
    video.addEventListener('webkitendfullscreen', nativeEnd);
    const tick = setInterval(update, 250);
    return () => {
      events.forEach(e => video.removeEventListener(e, update));
      video.removeEventListener('pointermove', wake); video.removeEventListener('pointerdown', wake);
      document.removeEventListener('fullscreenchange', fullscreen);
      video.removeEventListener('webkitbeginfullscreen', nativeBegin); video.removeEventListener('webkitendfullscreen', nativeEnd);
      clearInterval(tick);
      if (sleep.current) clearTimeout(sleep.current);
      if (previewTimer.current) clearTimeout(previewTimer.current);
    };
  }, [videoRef, durationHint, reveal]);
  const seek = (value: number) => {
    const video = videoRef.current;
    if (video && !cancelled.current) { video.currentTime = Math.max(0, Math.min(value, clock.duration)); onSeek(); }
    setDraft(null);
    if (pointer.current || cancelled.current) setPreview(null);
    else { setPreview(value); previewTimer.current = setTimeout(() => setPreview(null), 900); }
    pointer.current = false; cancelled.current = false;
  };
  const toggle = () => {
    const video = videoRef.current;
    if (video) { if (video.paused) void video.play().catch(() => undefined); else video.pause(); }
    reveal();
  };
  const fullscreen = async () => {
    reveal();
    const video = videoRef.current as SafariVideo | null;
    if (!video) return;
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (typeof video.parentElement?.requestFullscreen === 'function') await video.parentElement.requestFullscreen();
      else if (video.webkitEnterFullscreen) { video.controls = true; video.webkitEnterFullscreen(); }
    } catch {
      if (video.webkitEnterFullscreen) {
        try { video.controls = true; video.webkitEnterFullscreen(); return; } catch { video.controls = false; }
      }
      setFullscreenUnavailable(true);
    }
  };
  const visible = awake || clock.paused || draft !== null || preview !== null || volumeOpen;
  const fraction = preview === null ? 0 : preview / Math.max(1, clock.duration);
  const previewWidth = track.available < 330 ? 96 : track.available < 600 ? 128 : 160;
  const previewLeft = Math.max(0, Math.min(track.available - previewWidth, track.left + 10 + (track.rtl ? 1 - fraction : fraction) * Math.max(0, track.width - 20) - previewWidth / 2));
  return <MediaTheme mode="dark">
    <div role="group" aria-label="Playback controls" data-player-controls="true" {...stylex.props(styles.chrome, !visible && styles.hidden)} onPointerMove={reveal}
      onKeyDown={event => { if (event.key === 'Escape') { cancelled.current = pointer.current; setPreview(null); setDraft(null); setVolumeOpen(false); } }}>
      {preview !== null && clock.duration > 0 ? <div aria-hidden="true" data-testid="seek-preview" {...stylex.props(styles.preview)} style={{left: previewLeft, width: previewWidth}}>
        <SeekFrame itemId={itemId} position={preview} duration={clock.duration} />
      </div> : null}
      <Button label={clock.paused ? 'Play' : 'Pause'} isIconOnly variant="ghost" onClick={toggle}
        icon={clock.paused ? <Play size={18} aria-hidden="true" /> : <Pause size={18} aria-hidden="true" />} xstyle={styles.button} />
      <span aria-label="Elapsed time" {...stylex.props(styles.time, styles.elapsed)}>{formatClock(draft ?? clock.position)}</span>
      <div ref={timeline} {...stylex.props(styles.timeline)} onPointerDownCapture={() => { pointer.current = true; cancelled.current = false; }}
        onPointerCancelCapture={() => { cancelled.current = true; }}
        onPointerMove={event => {
          if (event.pointerType === 'touch' || pointer.current || clock.duration <= 0) return;
          if (previewTimer.current) clearTimeout(previewTimer.current);
          setVolumeOpen(false);
          const bounds = event.currentTarget.getBoundingClientRect();
          setPreview(positionAt(event.clientX, bounds.left, bounds.width, clock.duration, getComputedStyle(event.currentTarget).direction === 'rtl'));
        }} onPointerLeave={() => { if (!pointer.current) setPreview(null); }}
        onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setPreview(null); }}>
        <Slider label="Position" isLabelHidden value={draft ?? Math.min(clock.position, clock.duration)} min={0} max={Math.max(1, clock.duration)} step={1}
          valueDisplay="none" width="100%" isDisabled={clock.duration <= 0} formatValue={v => `${formatClock(v)} of ${formatClock(clock.duration)}`}
          onChange={(v: number) => { setDraft(v); setPreview(v); if (previewTimer.current) clearTimeout(previewTimer.current); reveal(); }} onChangeEnd={seek} />
      </div>
      <span aria-label="Duration" {...stylex.props(styles.time, styles.total)}>{formatClock(clock.duration)}</span>
      <div {...stylex.props(styles.volume)} onPointerEnter={() => setVolumeOpen(true)}
        onPointerLeave={event => { if (!event.currentTarget.contains(document.activeElement)) setVolumeOpen(false); }}
        onFocusCapture={() => setVolumeOpen(true)} onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget)) setVolumeOpen(false); }}>
        {volumeOpen ? <div role="group" aria-label="Volume controls" {...stylex.props(styles.volumePanel)}>
          <Slider label="Volume" isLabelHidden min={0} max={100} step={1} value={clock.muted ? 0 : Math.round(clock.volume * 100)}
            formatValue={v => `${v}%`} valueDisplay="none" width="100%" onChange={(value: number) => {
              const video = videoRef.current; if (video) { video.volume = value / 100; video.muted = value === 0; } reveal();
            }} />
        </div> : null}
        <Button label={clock.muted ? 'Unmute' : 'Mute'} isIconOnly variant="ghost" xstyle={styles.button}
          icon={clock.muted ? <VolumeX size={18} aria-hidden="true" /> : <Volume2 size={18} aria-hidden="true" />}
          onClick={() => { const video = videoRef.current; if (video) video.muted = !video.muted; setVolumeOpen(true); reveal(); }} />
      </div>
      {clock.canFullscreen ? <Button label={full ? 'Exit fullscreen' : 'Fullscreen'} isIconOnly variant="ghost" xstyle={styles.button}
        isDisabled={fullscreenUnavailable} tooltip={fullscreenUnavailable ? 'Fullscreen is unavailable in this browser.' : undefined}
        icon={full ? <Minimize size={18} aria-hidden="true" /> : <Maximize size={18} aria-hidden="true" />} onClick={() => { void fullscreen(); }} /> : null}
    </div>
  </MediaTheme>;
}

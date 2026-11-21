import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Slider} from '@astryxdesign/core/Slider';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {MediaTheme} from '@astryxdesign/core/theme';
import {Maximize, Minimize, Pause, Play, RotateCcw, RotateCw, Volume2, VolumeX} from 'lucide-react';
import {useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type PointerEvent, type RefObject} from 'react';
import {formatClock} from '../../lib/format';
import {PreviewFrames} from './previewFrames';
import {positionAt, previewPlacement, previewSecond} from './seeking';
import './PlayerControls.css';

type SafariVideo = HTMLVideoElement & {webkitEnterFullscreen?: () => void};

const styles = stylex.create({
  chrome: {
    position: 'absolute', insetInline: 12, bottom: 12, paddingInline: 8,
    borderRadius: 'var(--radius-container)', color: colorVars['--color-on-dark'],
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 88%, transparent)',
    display: 'flex', alignItems: 'center', gap: 4,
    opacity: {default: 1, ':focus-within': 1},
  },
  hidden: {opacity: {default: 0, ':focus-within': 1}, pointerEvents: {default: 'none', ':focus-within': 'auto'}},
  timeline: {position: 'relative', flexGrow: 1, minWidth: 0},
  time: {fontSize: 12, fontVariantNumeric: 'tabular-nums', whiteSpace: 'nowrap', paddingInline: 4},
  actions: {display: 'flex', alignItems: 'center', flexShrink: 0},
  volume: {position: 'relative', flexShrink: 0},
  volumePanel: {
    position: 'absolute', bottom: '100%', insetInlineEnd: 0, width: 160,
    padding: 12, borderRadius: 'var(--radius-container)', backgroundColor: 'var(--popcorn-stage)',
  },
  button: {color: 'inherit', flexShrink: 0, minWidth: 44, minHeight: 44},
  preview: {
    position: 'absolute', bottom: '100%', marginBottom: 8, width: 160, maxWidth: '100%',
    borderRadius: 'var(--popcorn-radius-media)', overflow: 'hidden', pointerEvents: 'none',
    backgroundColor: 'var(--popcorn-stage)', color: 'inherit', textAlign: 'center',
    display: 'flex', flexDirection: 'column',
  },
  frame: {position: 'relative', width: '100%', aspectRatio: '16 / 9', minHeight: 0, overflow: 'hidden', display: 'grid', placeItems: 'center', fontSize: 12},
  stamp: {display: 'block', flexShrink: 0, padding: 8, fontSize: 13, fontVariantNumeric: 'tabular-nums'},
});

function SeekPreview({itemId, position, sampleDuration, placement, dismiss}: {
  itemId: string; position: number | null; sampleDuration: number;
  placement: {left: number; width: number; maxHeight: number}; dismiss: () => void;
}) {
  const [frames] = useState(() => new PreviewFrames(itemId));
  const second = position === null ? null : previewSecond(position, sampleDuration);
  const frame = useSyncExternalStore(frames.subscribe, () => frames.read(second));
  const attachFrame = useCallback((slot: HTMLDivElement | null) => {
    if (!slot || !frame || frame === 'unavailable') return;
    // Mount the actual decoded image, including on cache hits.
    slot.append(frame);
    return () => frame.remove();
  }, [frame]);
  useLayoutEffect(() => {
    const clear = () => { frames.clear(); dismiss(); };
    const visibility = () => { if (document.hidden) clear(); };
    window.addEventListener('pagehide', clear);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      window.removeEventListener('pagehide', clear);
      document.removeEventListener('visibilitychange', visibility);
      frames.clear();
    };
  }, [frames, dismiss]);
  useLayoutEffect(() => { frames.select(second); }, [frames, second]);
  if (position === null || !placement.width) return null;

  return <div aria-hidden="true" data-testid="seek-preview" {...stylex.props(styles.preview)} style={placement}>
    <div ref={attachFrame} data-seek-frame {...stylex.props(styles.frame)}>
      {frame === 'unavailable' ? 'Preview unavailable' : !frame ? 'Loading preview' : null}
    </div>
    <span {...stylex.props(styles.stamp)}>{formatClock(position)}</span>
  </div>;
}

function SkipIcon({back = false}: {back?: boolean}) {
  const Arrow = back ? RotateCcw : RotateCw;
  return <Arrow size={24} strokeWidth={1.75} aria-hidden="true">
    <text x="12" y="15.5" textAnchor="middle" fontSize="8.5" fontWeight="650" fill="currentColor" stroke="none">15</text>
  </Arrow>;
}

export function PlayerControls({videoRef, itemId, durationHint, onSeek, onSkip}: {
  videoRef: RefObject<HTMLVideoElement | null>; itemId: string; durationHint: number; onSeek: () => void; onSkip: (seconds: -15 | 15) => void;
}) {
  const [clock, setClock] = useState({position: 0, duration: durationHint, paused: true, muted: false, volume: 1, canFullscreen: false});
  const [draft, setDraft] = useState<number | null>(null);
  const [preview, setPreview] = useState<number | null>(null);
  const [awake, setAwake] = useState(true);
  const [full, setFull] = useState(false);
  const [volumeOpen, setVolumeOpen] = useState(false);
  const [fullscreenUnavailable, setFullscreenUnavailable] = useState(false);
  const timeline = useRef<HTMLDivElement>(null);
  const volumeControl = useRef<HTMLDivElement>(null);
  const volumePanel = useRef<HTMLDivElement>(null);
  const [track, setTrack] = useState({left: 0, width: 0, available: 0, headroom: 0, rtl: false});
  const drag = useRef<number | null>(null);
  const sleep = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previewTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reveal = useCallback(() => {
    setAwake(true);
    if (sleep.current) clearTimeout(sleep.current);
    sleep.current = setTimeout(() => setAwake(false), 3000);
  }, []);
  useLayoutEffect(() => {
    const element = timeline.current;
    const controls = element?.parentElement;
    const stage = videoRef.current?.parentElement;
    if (!element || !controls || !stage) return;
    const measure = () => {
      const box = element.getBoundingClientRect();
      const outer = controls.getBoundingClientRect();
      setTrack({
        left: box.left - outer.left, width: box.width, available: outer.width,
        headroom: Math.max(0, outer.top - stage.getBoundingClientRect().top - 8),
        rtl: getComputedStyle(element).direction === 'rtl',
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const node of [element, controls, stage]) observer.observe(node);
    return () => observer.disconnect();
  }, [videoRef]);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const update = () => setClock({position: video.currentTime, duration: Number.isFinite(video.duration) && video.duration > 0 ? video.duration : durationHint, paused: video.paused, muted: video.muted, volume: video.volume,
      canFullscreen: (typeof video.parentElement?.requestFullscreen === 'function' && document.fullscreenEnabled !== false) || typeof (video as SafariVideo).webkitEnterFullscreen === 'function'});
    const events = ['timeupdate', 'seeking', 'seeked', 'durationchange', 'loadedmetadata', 'play', 'pause', 'ended', 'volumechange'];
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
  const dismissPreview = useCallback(() => {
    const id = drag.current;
    drag.current = null;
    if (id !== null && timeline.current?.hasPointerCapture(id)) timeline.current.releasePointerCapture(id);
    if (previewTimer.current) clearTimeout(previewTimer.current);
    setDraft(null);
    setPreview(null);
  }, []);

  const hasPreview = preview !== null;
  useEffect(() => {
    if (!hasPreview) return;
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') dismissPreview(); };
    window.addEventListener('keydown', escape);
    window.addEventListener('blur', dismissPreview);
    return () => {
      window.removeEventListener('keydown', escape);
      window.removeEventListener('blur', dismissPreview);
    };
  }, [hasPreview, dismissPreview]);

  const commitSeek = (value: number) => {
    const video = videoRef.current;
    if (!video || clock.duration <= 0) return;
    video.currentTime = Math.max(0, Math.min(value, clock.duration));
    // Repeated keyboard steps can arrive before the next video timeupdate.
    setClock(previous => ({...previous, position: video.currentTime}));
    onSeek();
  };
  const showKeyboardPreview = (value: number) => {
    if (previewTimer.current) clearTimeout(previewTimer.current);
    setPreview(value);
    previewTimer.current = setTimeout(() => setPreview(null), 900);
    reveal();
  };
  const keyboardSeek = (value: number) => {
    if (drag.current !== null) return;
    commitSeek(value);
    showKeyboardPreview(value);
  };
  const pointerPosition = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return positionAt(event.clientX, box.left, box.width, clock.duration, track.rtl);
  };
  const showPointerPosition = (event: PointerEvent<HTMLDivElement>) => {
    if (previewTimer.current) clearTimeout(previewTimer.current);
    const value = pointerPosition(event);
    setPreview(value);
    if (drag.current !== null) setDraft(value);
  };
  const handleVolumeHover = (event: PointerEvent<HTMLDivElement>) => {
    const control = volumeControl.current;
    const panel = volumePanel.current;
    if (!panel || !control || event.pointerType === 'touch' || event.buttons !== 0 || control.contains(event.target as Node)) return;
    // Only the compact two-row layout has a gap between the button and panel.
    const button = control.getBoundingClientRect();
    if (event.clientX >= button.left && event.clientX < button.right &&
        event.clientY >= panel.getBoundingClientRect().bottom && event.clientY < button.top) {
      event.stopPropagation();
      reveal();
      return;
    }
    if (!panel.contains(document.activeElement)) setVolumeOpen(false);
  };
  const endDrag = (event: PointerEvent<HTMLDivElement>, commit: boolean) => {
    if (drag.current !== event.pointerId) return;
    if (commit) commitSeek(pointerPosition(event));
    dismissPreview();
  };
  const skip = (seconds: -15 | 15) => {
    dismissPreview();
    onSkip(seconds);
    reveal();
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
  const placement = {...previewPlacement(preview ?? 0, clock.duration, track), maxHeight: track.headroom};
  return <MediaTheme mode="dark">
    <div role="group" aria-label="Playback controls" data-player-controls="true" {...stylex.props(styles.chrome, !visible && styles.hidden)} onPointerMove={reveal}
      onPointerMoveCapture={handleVolumeHover}
      onPointerLeave={() => {
        if (!volumeControl.current?.contains(document.activeElement)) setVolumeOpen(false);
      }}
      onKeyDown={event => { if (event.key === 'Escape') { dismissPreview(); setVolumeOpen(false); } }}>
      <SeekPreview key={itemId} itemId={itemId} position={clock.duration > 0 ? preview : null}
        sampleDuration={durationHint > 0 ? durationHint : clock.duration} placement={placement} dismiss={dismissPreview} />
      <div data-player-transport {...stylex.props(styles.actions)}>
        <Button label="Back 15 seconds" isIconOnly variant="ghost" icon={<SkipIcon back />}
          isDisabled={clock.duration <= 0} onClick={() => skip(-15)} xstyle={styles.button} />
        <Button label={clock.paused ? 'Play' : 'Pause'} isIconOnly variant="ghost" onClick={toggle}
          icon={clock.paused ? <Play size={18} aria-hidden="true" /> : <Pause size={18} aria-hidden="true" />} xstyle={styles.button} />
        <Button label="Forward 15 seconds" isIconOnly variant="ghost" icon={<SkipIcon />}
          isDisabled={clock.duration <= 0} onClick={() => skip(15)} xstyle={styles.button} />
      </div>
      <span aria-label="Elapsed time" data-player-time="elapsed" {...stylex.props(styles.time)}>{formatClock(draft ?? clock.position)}</span>
      <div ref={timeline} data-player-timeline {...stylex.props(styles.timeline)} data-dragging={draft !== null || undefined}
        onPointerDownCapture={event => {
          // Astryx ends a cancelled pointer just like a release. Own the
          // pointer gesture so only an intentional release changes playback.
          event.stopPropagation();
          if (drag.current !== null || !event.isPrimary || event.button !== 0 || clock.duration <= 0) return;
          event.preventDefault();
          setVolumeOpen(false);
          drag.current = event.pointerId;
          event.currentTarget.setPointerCapture(event.pointerId);
          event.currentTarget.querySelector<HTMLElement>('[role="slider"]')?.focus({preventScroll: true});
          showPointerPosition(event);
          reveal();
        }}
        onPointerMove={event => {
          if (clock.duration <= 0) return;
          if (drag.current !== null && drag.current !== event.pointerId) return;
          if (drag.current === null && (event.pointerType === 'touch' || event.buttons !== 0)) return;
          showPointerPosition(event);
        }}
        onPointerUp={event => endDrag(event, true)}
        onPointerCancel={event => endDrag(event, false)}
        onLostPointerCapture={event => { if (event.target === event.currentTarget) endDrag(event, false); }}
        onPointerLeave={() => { if (drag.current === null) setPreview(null); }}
        onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) dismissPreview(); }}
        onKeyDownCapture={event => {
          // Astryx snaps End to a whole step, even on a fractional last second.
          if (event.key === 'End' && clock.duration > 0 && !Number.isInteger(clock.duration)) {
            event.preventDefault();
            event.stopPropagation();
            keyboardSeek(clock.duration);
          }
        }}>
        <Slider label="Position" isLabelHidden value={draft ?? Math.min(clock.position, clock.duration)} min={0}
          max={clock.duration || 1} step={Math.min(1, clock.duration) || 1}
          valueDisplay="none" width="100%" isDisabled={clock.duration <= 0} formatValue={v => `${formatClock(v)} of ${formatClock(clock.duration)}`}
          onChangeEnd={keyboardSeek} />
      </div>
      <span aria-label="Duration" data-player-time="duration" {...stylex.props(styles.time)}>{formatClock(clock.duration)}</span>
      <div data-player-tools {...stylex.props(styles.actions)}>
        <div ref={volumeControl} data-player-volume {...stylex.props(styles.volume)} onPointerEnter={() => { dismissPreview(); setVolumeOpen(true); }}
          onFocusCapture={() => setVolumeOpen(true)} onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget)) setVolumeOpen(false); }}>
          {volumeOpen ? <div ref={volumePanel} role="group" aria-label="Volume controls" {...stylex.props(styles.volumePanel)}>
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
    </div>
  </MediaTheme>;
}

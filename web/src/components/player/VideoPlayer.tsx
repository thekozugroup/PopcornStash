/**
 * The video stage: a true black surface with the native <video> controls.
 *
 * Playback starts on demand (so opening a page never starts a transcode):
 * the browser's codec support goes to `POST /api/items/:id/playback`, which
 * answers with the original file (direct play) or an HLS stream. HLS uses
 * hls.js (loaded on demand) wherever Media Source Extensions work, and the
 * browser's own player only where they do not (iOS without them). The stream
 * session is ended when the player goes away.
 *
 * Progress is saved every 10 seconds while playing, on pause, at the end,
 * and when the page is hidden or closed, so "Continue watching" and resume
 * stay accurate across devices.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {DropdownMenu, DropdownMenuRadioGroup, DropdownMenuRadioItem} from '@astryxdesign/core/DropdownMenu';
import {Spinner} from '@astryxdesign/core/Spinner';
import {Text} from '@astryxdesign/core/Text';
import {MediaTheme} from '@astryxdesign/core/theme';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AudioLines, Download, Gauge, Play, RotateCcw, SkipForward} from 'lucide-react';
import {useEffect, useRef, useState} from 'react';
import type HlsType from 'hls.js';
import {items as itemsApi, stream} from '../../api/client';
import type {ItemDetail, PlaybackRequest, PlaybackResponse} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatClock} from '../../lib/format';
import {audioTrackDetail, audioTrackLabel, largeThumb, resolutionName, resumePosition} from '../library/libraryFormat';
import {SAFE_CAPS, detectPlaybackCaps, hasNativeHls, type PlaybackCaps} from './capabilities';
import {endStreamOnExit, saveProgressOnExit} from './keepalive';

const styles = stylex.create({
  seekFeedback: {
    position: 'absolute', top: '40%', padding: 16, borderRadius: 'var(--radius-control)',
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 80%, transparent)',
    color: colorVars['--color-on-dark'], pointerEvents: 'none',
  },
  seekLeft: {left: '10%'}, seekRight: {right: '10%'},
  stage: {
    position: 'relative',
    width: 'auto',
    aspectRatio: '16 / 9',
    maxHeight: {default: 'calc(100dvh - 200px)', '@media (max-width: 799px)': '70dvh'},
    minHeight: 200,
    marginInline: {default: 0, '@media (max-width: 799px)': -20},
    overflow: 'hidden',
    borderRadius: {default: 'var(--radius-container)', '@media (max-width: 799px)': 0},
    backgroundColor: 'var(--popcorn-stage)',
    colorScheme: 'dark',
  },
  video: {
    position: 'absolute',
    inset: 0,
    width: '100%',
    height: '100%',
    objectFit: 'contain',
    backgroundColor: 'var(--popcorn-stage)',
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: -2,
  },
  overlay: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 14,
    padding: 24,
    textAlign: 'center',
    // The black stage colour over the poster, the same in both themes.
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 48%, transparent)',
    color: colorVars['--color-on-dark'],
  },
  // Secondary controls on the stage read as light text on black in both themes.
  onStage: {
    color: colorVars['--color-on-dark'],
  },
  overlaySolid: {
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 72%, transparent)',
  },
  message: {
    maxWidth: '44ch',
    lineHeight: 1.55,
  },
  messageTitle: {
    fontWeight: fontWeightVars['--font-weight-semibold'],
  },
  row: {
    display: 'flex',
    flexWrap: 'wrap',
    justifyContent: 'center',
    gap: 8,
  },
  switching: {
    position: 'absolute',
    top: 14,
    insetInlineEnd: 14,
    pointerEvents: 'none',
  },
  bar: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    justifyContent: 'space-between',
    columnGap: 16,
    rowGap: 8,
    marginTop: 12,
  },
  barGroup: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 4,
    minWidth: 0,
  },
  barGroupStart: {
    marginInlineStart: -10,
  },
  note: {
    paddingInline: 10,
  },
});

type Phase = 'idle' | 'starting' | 'ready' | 'error';

const QUALITY_STEPS = [2160, 1440, 1080, 720, 480];
const PROGRESS_EVERY_MS = 10_000;
const ORIGINAL = 'original';

interface StartOptions {
  startAt: number;
  quality: number | null;
  audioIndex: number | null;
  caps?: PlaybackCaps;
  autoplay: boolean;
}

export interface VideoPlayerProps {
  item: ItemDetail;
  /** Start as soon as the page opens (arrived from a Play button). */
  autoPlay?: boolean;
  /** Called by the "Next episode" button. */
  onNext?: (() => void) | null;
}

export function VideoPlayer({item, autoPlay = false, onNext = null}: VideoPlayerProps) {
  const [phase, setPhase] = useState<Phase>(autoPlay ? 'starting' : 'idle');
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<PlaybackResponse['mode'] | null>(null);
  const [quality, setQuality] = useState<number | null>(null);
  const [audioIndex, setAudioIndex] = useState<number | null>(null);
  const [isSwitching, setIsSwitching] = useState(false);
  const [hasEnded, setHasEnded] = useState(false);
  const [seekFeedback, setSeekFeedback] = useState<{side: 'left' | 'right'; time: number} | null>(null);
  const touchTap = useRef<{time: number; x: number; y: number; side: 'left' | 'right'} | null>(null);
  const lastTouchSeek = useRef(-Infinity);
  useEffect(() => {
    if (!seekFeedback) return;
    const timeout = setTimeout(() => setSeekFeedback(null), 900);
    return () => clearTimeout(timeout);
  }, [seekFeedback]);

  const videoRef = useRef<HTMLVideoElement | null>(null);
  // React clears element refs before unmount cleanups run; the last element
  // is kept so the final progress report still knows where playback stopped.
  const lastVideoRef = useRef<HTMLVideoElement | null>(null);
  const hlsRef = useRef<HlsType | null>(null);
  const sessionRef = useRef<string | null>(null);
  const requestRef = useRef(0);
  const resumeRef = useRef(0);
  const startedRef = useRef(false);
  const fellBackRef = useRef(false);
  const lastSavedRef = useRef(-1);
  const modeRef = useRef<PlaybackResponse['mode'] | null>(null);
  // Play and Try again disappear once pressed; focus moves to the video so
  // keyboard users keep their place and can pause with the space bar.
  const focusOnReadyRef = useRef(false);

  const resumeAt = resumePosition(item);
  const poster = largeThumb(item) ?? undefined;
  const sourceHeight = item.media?.video?.height ?? item.height ?? 0;
  const qualities = QUALITY_STEPS.filter(step => step < sourceHeight - 40);
  const audioTracks = item.media?.audio ?? [];
  const sourceQuality = resolutionName(item.media?.video?.width ?? item.width, sourceHeight);

  /** Stops the current stream: hls.js, the server session and the element source. */
  const teardown = (onExit = false) => {
    hlsRef.current?.destroy();
    hlsRef.current = null;
    const session = sessionRef.current;
    sessionRef.current = null;
    if (session) {
      if (onExit) endStreamOnExit(session);
      else stream.end(session).catch(() => undefined);
    }
  };

  const report = (onExit = false) => {
    const video = videoRef.current ?? lastVideoRef.current;
    if (!video || !startedRef.current) return;
    const duration = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : (item.duration_s ?? 0);
    const position = video.ended ? duration : video.currentTime;
    if (!duration || position <= 0) return;
    if (!onExit && Math.abs(position - lastSavedRef.current) < 1) return;
    lastSavedRef.current = position;
    const body = {position_s: Math.round(position * 10) / 10, duration_s: Math.round(duration * 10) / 10};
    if (onExit) saveProgressOnExit(item.id, body);
    else itemsApi.saveProgress(item.id, body).catch(() => undefined);
  };

  const fail = (message: string) => {
    teardown();
    setIsSwitching(false);
    setError(message);
    setPhase('error');
  };

  const start = async ({startAt, quality: maxHeight, audioIndex: track, caps, autoplay}: StartOptions) => {
    const video = videoRef.current;
    if (!video) return;
    const token = ++requestRef.current;
    const body: PlaybackRequest = {...(caps ?? detectPlaybackCaps(video))};
    if (startAt > 0) body.start_s = Math.floor(startAt);
    if (maxHeight) body.max_height = maxHeight;
    if (track !== null) body.audio_index = track;
    try {
      const response = await itemsApi.playback(item.id, body);
      if (token !== requestRef.current) {
        if (response.mode === 'hls') stream.end(response.session_id).catch(() => undefined);
        return;
      }
      teardown();
      startedRef.current = true;
      modeRef.current = response.mode;
      resumeRef.current = startAt;
      if (response.mode === 'hls') {
        sessionRef.current = response.session_id;
        // hls.js wherever Media Source Extensions work, native HLS only where they do not
        // (iOS without them). Chrome also claims native HLS now, but its own player loads
        // the start of the stream before it seeks, which makes the server restart the
        // conversion twice on every resume; hls.js starts at the resume point.
        const {default: Hls} = await import('hls.js');
        if (token !== requestRef.current) return;
        if (!Hls.isSupported() && !hasNativeHls(video)) {
          fail('This browser cannot play streamed video. Try another browser, or download the original file.');
          return;
        }
        if (Hls.isSupported()) {
          const hls = new Hls({startPosition: startAt > 0 ? startAt : -1});
          let recovered = false;
          hls.on(Hls.Events.ERROR, (_event, data) => {
            if (!data.fatal) return;
            if (data.type === Hls.ErrorTypes.MEDIA_ERROR && !recovered) {
              recovered = true;
              hls.recoverMediaError();
              return;
            }
            fail(
              data.type === Hls.ErrorTypes.NETWORK_ERROR
                ? 'The stream stopped. Check the connection and try again.'
                : 'This video could not be played in this browser.',
            );
          });
          resumeRef.current = 0;
          hlsRef.current = hls;
          hls.loadSource(response.url);
          hls.attachMedia(video);
        } else {
          video.src = response.url;
        }
      } else {
        video.src = response.url;
      }
      setMode(response.mode);
      setError(null);
      setPhase('ready');
      setIsSwitching(false);
      if (autoplay) video.play().catch(() => undefined);
    } catch (failure) {
      if (token !== requestRef.current) return;
      fail(errorMessage(failure, 'Playback could not start. Try again in a moment.'));
    }
  };

  // Latest handlers for effects that run once.
  const latest = useRef({start, report, teardown});
  useEffect(() => {
    latest.current = {start, report, teardown};
  });

  useEffect(() => {
    if (autoPlay) {
      void latest.current.start({startAt: resumeAt, quality: null, audioIndex: null, autoplay: true});
    }
    // Arrival only: later changes go through the controls.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const timer = setInterval(() => {
      const video = videoRef.current;
      if (video && !video.paused && !video.ended) latest.current.report();
    }, PROGRESS_EVERY_MS);
    const onPageHide = () => {
      latest.current.report(true);
      latest.current.teardown(true);
    };
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') latest.current.report(true);
    };
    window.addEventListener('pagehide', onPageHide);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      clearInterval(timer);
      window.removeEventListener('pagehide', onPageHide);
      document.removeEventListener('visibilitychange', onVisibility);
      latest.current.report(true);
      latest.current.teardown(true);
    };
  }, []);

  useEffect(() => {
    if (phase !== 'ready' || !focusOnReadyRef.current) return;
    focusOnReadyRef.current = false;
    videoRef.current?.focus({preventScroll: true});
  }, [phase]);

  const begin = (startAt: number) => {
    focusOnReadyRef.current = true;
    setPhase('starting');
    setError(null);
    void start({startAt, quality, audioIndex, autoplay: true});
  };

  const switchStream = (next: {quality: number | null; audioIndex: number | null}) => {
    const video = videoRef.current;
    setQuality(next.quality);
    setAudioIndex(next.audioIndex);
    if (!video || phase !== 'ready') return;
    report();
    setIsSwitching(true);
    void start({startAt: video.currentTime, ...next, autoplay: !video.paused});
  };

  const onVideoError = () => {
    const video = videoRef.current;
    // hls.js reports its own errors; an empty source is not a failure.
    if (!video || hlsRef.current || !video.currentSrc || phase !== 'ready') return;
    if (modeRef.current === 'direct' && !fellBackRef.current) {
      // The browser claimed support it does not have. Ask for the safest stream once.
      fellBackRef.current = true;
      setIsSwitching(true);
      void start({startAt: video.currentTime || resumeAt, quality, audioIndex, caps: SAFE_CAPS, autoplay: true});
      return;
    }
    fail('This video could not be played in this browser. You can still download the original file.');
  };

  const showQuality = qualities.length > 0;
  const seekAt = (video: HTMLVideoElement, x: number, y: number) => {
    if (phase !== 'ready') return;
    const rect = video.getBoundingClientRect();
    // Leave the browser's playback/volume/scrubber strip untouched.
    if (y > rect.bottom - 64) return;
    const side = x < rect.left + rect.width / 2 ? 'left' : 'right';
    const duration = Number.isFinite(video.duration) ? video.duration : item.duration_s;
    if (!duration || duration <= 0) return;
    video.currentTime = Math.max(0, Math.min(duration, video.currentTime + (side === 'left' ? -15 : 15)));
    setHasEnded(false);
    setSeekFeedback({side, time: performance.now()});
    report();
  };
  const showAudio = audioTracks.length > 1;
  const showBar = phase === 'ready' || showQuality || showAudio || Boolean(onNext) || mode === 'hls';
  const qualityLabel = quality ? `${quality}p` : 'Original';
  const currentTrack = audioTracks.find(track => track.index === audioIndex) ?? audioTracks[0];

  return (
    <div>
      <div {...stylex.props(styles.stage)}>
        <video
          ref={node => {
            videoRef.current = node;
            if (node) lastVideoRef.current = node;
          }}
          poster={poster}
          controls={phase === 'ready'}
          playsInline
          preload="metadata"
          aria-label={item.title}
          tabIndex={phase === 'ready' ? 0 : -1}
          onDoubleClick={event => {
            event.preventDefault();
            if (performance.now() - lastTouchSeek.current > 500) seekAt(event.currentTarget, event.clientX, event.clientY);
          }}
          onPointerUp={event => {
            if (event.pointerType !== 'touch' || !event.isPrimary) return;
            const rect = event.currentTarget.getBoundingClientRect();
            const side = event.clientX < rect.left + rect.width / 2 ? 'left' : 'right';
            const previous = touchTap.current;
            const time = performance.now();
            if (previous && time - previous.time < 300 && previous.side === side && Math.hypot(previous.x - event.clientX, previous.y - event.clientY) < 48) {
              touchTap.current = null;
              lastTouchSeek.current = time;
              seekAt(event.currentTarget, event.clientX, event.clientY);
            } else touchTap.current = {time, x: event.clientX, y: event.clientY, side};
          }}
          onLoadedMetadata={event => {
            const video = event.currentTarget;
            if (resumeRef.current > 0) {
              video.currentTime = Math.min(resumeRef.current, Math.max(0, (video.duration || resumeRef.current) - 1));
              resumeRef.current = 0;
            }
          }}
          onPlay={() => setHasEnded(false)}
          onPause={() => report()}
          onEnded={() => {
            report();
            setHasEnded(true);
          }}
          onError={onVideoError}
          {...stylex.props(styles.video)}
        />
        {seekFeedback ? <div role="status" {...stylex.props(styles.seekFeedback, seekFeedback.side === 'left' ? styles.seekLeft : styles.seekRight)}>{seekFeedback.side === 'left' ? '−15 seconds' : '+15 seconds'}</div> : null}
        {phase !== 'ready' ? (
          <div {...stylex.props(styles.overlay, phase === 'error' && styles.overlaySolid)}>
            {phase === 'idle' ? (
              <>
                <Button
                  label={resumeAt > 0 ? `Resume from ${formatClock(resumeAt)}` : 'Play'}
                  variant="primary"
                  size="lg"
                  icon={<Play size={18} strokeWidth={2} aria-hidden="true" />}
                  onClick={() => begin(resumeAt)}
                />
                {resumeAt > 0 ? (
                  <MediaTheme mode="dark">
                    <Button
                      label="Start from the beginning"
                      variant="ghost"
                      icon={<RotateCcw size={16} strokeWidth={1.75} aria-hidden="true" />}
                      onClick={() => begin(0)}
                      xstyle={styles.onStage}
                    />
                  </MediaTheme>
                ) : null}
              </>
            ) : null}
            {phase === 'starting' ? <Spinner size="xl" shade="onMedia" aria-label="Starting playback" /> : null}
            {phase === 'error' ? (
              <>
                <div role="alert">
                  <Text as="p" color="inherit" xstyle={styles.message}>
                    <span {...stylex.props(styles.messageTitle)}>Playback stopped. </span>
                    {error}
                  </Text>
                </div>
                <div {...stylex.props(styles.row)}>
                  <Button
                    label="Try again"
                    variant="primary"
                    onClick={() => {
                      fellBackRef.current = false;
                      begin(videoRef.current?.currentTime || resumeAt);
                    }}
                  />
                  <MediaTheme mode="dark">
                    <Button
                      label="Download original"
                      variant="ghost"
                      href={itemsApi.fileUrl(item.id, true)}
                      icon={<Download size={16} strokeWidth={1.75} aria-hidden="true" />}
                      xstyle={styles.onStage}
                    />
                  </MediaTheme>
                </div>
              </>
            ) : null}
          </div>
        ) : null}
        {isSwitching && phase === 'ready' ? (
          <span {...stylex.props(styles.switching)}>
            <Spinner size="lg" shade="onMedia" aria-label="Switching stream" />
          </span>
        ) : null}
      </div>

      {showBar ? (
        <div {...stylex.props(styles.bar)}>
          <div {...stylex.props(styles.barGroup, styles.barGroupStart)}>
            {phase === 'ready' ? <>
              <Button label="Back 15 seconds" variant="ghost" onClick={() => { const video = videoRef.current; if (video) { const r = video.getBoundingClientRect(); seekAt(video, r.left + r.width / 4, r.top); } }} />
              <Button label="Forward 15 seconds" variant="ghost" onClick={() => { const video = videoRef.current; if (video) { const r = video.getBoundingClientRect(); seekAt(video, r.left + r.width * 3 / 4, r.top); } }} />
            </> : null}
            {showAudio && currentTrack ? (
              <DropdownMenu
                button={{
                  label: `Audio: ${audioTrackLabel(currentTrack, audioTracks.indexOf(currentTrack))}`,
                  variant: 'ghost',
                  icon: <AudioLines size={16} strokeWidth={1.75} aria-hidden="true" />,
                }}
                menuWidth={260}>
                <DropdownMenuRadioGroup
                  label="Audio track"
                  value={String(currentTrack.index)}
                  onChange={value => {
                    const index = Number(value);
                    switchStream({quality, audioIndex: index === audioTracks[0]?.index ? null : index});
                  }}>
                  {audioTracks.map((track, position) => (
                    <DropdownMenuRadioItem
                      key={track.index}
                      value={String(track.index)}
                      label={audioTrackLabel(track, position)}
                      description={audioTrackDetail(track)}
                    />
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenu>
            ) : null}
            {showQuality ? (
              <DropdownMenu
                button={{
                  label: `Quality: ${qualityLabel}`,
                  variant: 'ghost',
                  icon: <Gauge size={16} strokeWidth={1.75} aria-hidden="true" />,
                }}
                menuWidth={240}>
                <DropdownMenuRadioGroup
                  label="Quality"
                  value={quality ? String(quality) : ORIGINAL}
                  onChange={value => switchStream({quality: value === ORIGINAL ? null : Number(value), audioIndex})}>
                  <DropdownMenuRadioItem
                    value={ORIGINAL}
                    label="Original"
                    description={sourceQuality ? `${sourceQuality}, converted only if needed` : 'Converted only if needed'}
                  />
                  {qualities.map(step => (
                    <DropdownMenuRadioItem
                      key={step}
                      value={String(step)}
                      label={`${step}p`}
                      description="Smaller stream for slow connections"
                    />
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenu>
            ) : null}
            {mode === 'hls' ? (
              <Text type="supporting" color="secondary" xstyle={styles.note}>
                Converted for this device while you watch
              </Text>
            ) : null}
          </div>
          {onNext ? (
            <div {...stylex.props(styles.barGroup)}>
              <Button
                label="Next episode"
                variant={hasEnded ? 'primary' : 'secondary'}
                icon={<SkipForward size={16} strokeWidth={1.75} aria-hidden="true" />}
                onClick={() => {
                  report();
                  onNext();
                }}
              />
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

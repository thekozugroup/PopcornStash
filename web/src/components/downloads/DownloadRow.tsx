/**
 * An in-progress torrent or web download from the server, live through
 * `download` events.
 */
import {IconButton} from '@astryxdesign/core/IconButton';
import {Badge} from '@astryxdesign/core/Badge';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {ArrowDown, ArrowUp, Globe, Magnet, Pause, Play, Upload, Users, type LucideIcon} from 'lucide-react';
import {useState} from 'react';
import {downloads as downloadsApi} from '../../api/client';
import type {Download, DownloadKind} from '../../api/types';
import {formatBytes, formatBytesOf, formatCount, formatEta} from '../../lib/format';
import {activeStateWords, canPause, canResume, fetchingTorrentDetails} from './downloadText';
import {TransferRow} from './TransferRow';

export const KIND_ICON: Record<DownloadKind, LucideIcon> = {
  torrent: Magnet,
  http: Globe,
  upload: Upload,
};

export interface DownloadRowProps {
  download: Download;
  onFiles: (download: Download) => void;
  onChanged: (download: Download) => void;
  onRemove: (download: Download) => void;
  onError: (message: string) => void;
}

export function DownloadRow({download, onFiles, onChanged, onRemove, onError}: DownloadRowProps) {
  const [isBusy, setIsBusy] = useState(false);
  const pausable = canPause(download);
  const resumable = canResume(download);

  const toggle = async () => {
    setIsBusy(true);
    try {
      const next = resumable ? await downloadsApi.resume(download.id) : await downloadsApi.pause(download.id);
      onChanged(next);
    } catch (error) {
      onError(error instanceof Error && error.message ? error.message : 'That did not work. Try again.');
    } finally {
      setIsBusy(false);
    }
  };

  const known = download.total_bytes !== null && download.total_bytes > 0;
  const progress = known || download.progress > 0 ? Math.min(1, Math.max(0, download.progress)) : null;
  const moving = download.state === 'downloading';
  const fetchingDetails = fetchingTorrentDetails(download);
  const detail = [
    fetchingDetails ? 'Waiting for a peer to share the file list. Retrying automatically.' : null,
    known ? formatBytesOf(download.done_bytes, download.total_bytes) : download.done_bytes > 0 ? formatBytesOf(download.done_bytes, null) : null,
    moving ? formatEta(download.eta_s) : null,
  ].filter(Boolean).join(' · ');
  const down = `${formatBytes(download.down_bps)}/s`;
  const up = `${formatBytes(download.up_bps)}/s`;
  const peers = download.peers === null ? 'Peers pending' : formatCount(download.peers, 'peer');

  return (
    <TransferRow
      icon={KIND_ICON[download.kind]}
      name={download.name}
      stateText={activeStateWords(download)}
      progress={progress}
      tone={download.state === 'paused' || download.state === 'queued' ? 'paused' : 'active'}
      detail={detail}
      telemetry={
        <>
          <span role="img" aria-label={`Download speed: ${down}`}><Badge variant={download.down_bps > 0 ? 'green' : 'neutral'} icon={<ArrowDown size={12} />} label={down} /></span>
          {download.kind === 'torrent' ? <>
            <span role="img" aria-label={`Upload speed: ${up}`}><Badge variant={download.up_bps > 0 ? 'blue' : 'neutral'} icon={<ArrowUp size={12} />} label={up} /></span>
            <span role="img" aria-label={download.peers === null ? 'Connected peers: not reported yet' : `${peers} connected`}><Badge variant="neutral" icon={<Users size={12} />} label={peers} /></span>
          </> : null}
        </>
      }
      actions={
        <>
          {pausable || resumable ? (
            <IconButton
              label={resumable ? `Resume ${download.name}` : `Pause ${download.name}`}
              tooltip={resumable ? 'Resume' : 'Pause'}
              icon={resumable ? <Play size={18} strokeWidth={1.75} /> : <Pause size={18} strokeWidth={1.75} />}
              variant="ghost"
              isLoading={isBusy}
              onClick={() => void toggle()}
            />
          ) : null}
          <MoreMenu
            label={`More for ${download.name}`}
            alignment="end"
            items={[...(download.kind === 'torrent' ? [{label: 'Choose files…', onClick: () => onFiles(download)}] : []), {label: 'Remove…', variant: 'destructive', onClick: () => onRemove(download)}]}
          />
        </>
      }
    />
  );
}

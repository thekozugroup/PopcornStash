/**
 * An in-progress torrent or web download from the server, live through
 * `download` events.
 */
import {IconButton} from '@astryxdesign/core/IconButton';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {Globe, Magnet, Pause, Play, Upload, type LucideIcon} from 'lucide-react';
import {useState} from 'react';
import {downloads as downloadsApi} from '../../api/client';
import type {Download, DownloadKind} from '../../api/types';
import {formatBytesOf, formatCount, formatEta, formatSpeed} from '../../lib/format';
import {activeStateWords, canPause, canResume, KIND_LABEL} from './downloadText';
import {TransferRow} from './TransferRow';

export const KIND_ICON: Record<DownloadKind, LucideIcon> = {
  torrent: Magnet,
  http: Globe,
  upload: Upload,
};

export interface DownloadRowProps {
  download: Download;
  onChanged: (download: Download) => void;
  onRemove: (download: Download) => void;
  onError: (message: string) => void;
}

export function DownloadRow({download, onChanged, onRemove, onError}: DownloadRowProps) {
  const [isBusy, setIsBusy] = useState(false);
  const pausable = canPause(download);
  const resumable = canResume(download);
  const isSeeding = download.state === 'seeding';

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
  const meta = [
    known ? formatBytesOf(download.done_bytes, download.total_bytes) : download.done_bytes > 0 ? formatBytesOf(download.done_bytes, null) : null,
    moving ? formatSpeed(download.down_bps) : null,
    isSeeding && download.up_bps > 0 ? `Sharing at ${formatSpeed(download.up_bps)}` : null,
    moving ? formatEta(download.eta_s) : null,
    download.kind === 'torrent' && download.peers !== null && (moving || isSeeding) ? formatCount(download.peers, 'peer') : null,
  ];

  return (
    <TransferRow
      icon={KIND_ICON[download.kind]}
      name={download.name}
      stateText={`${KIND_LABEL[download.kind]} · ${activeStateWords(download)}`}
      progress={progress}
      tone={download.state === 'paused' || download.state === 'queued' ? 'paused' : 'active'}
      meta={meta}
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
            items={[{label: 'Remove…', variant: 'destructive', onClick: () => onRemove(download)}]}
          />
        </>
      }
    />
  );
}

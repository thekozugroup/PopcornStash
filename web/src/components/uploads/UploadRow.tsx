/**
 * One browser upload in the In progress list: per-file progress, speed and
 * time left, pause and resume, and cancel from the menu. An upload
 * interrupted on an earlier visit asks for the same file again to continue.
 * While finishing, the server is already turning the file into a download,
 * so the menu is disabled for that moment.
 */
import {Button} from '@astryxdesign/core/Button';
import {IconButton} from '@astryxdesign/core/IconButton';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {Pause, Play, RotateCcw, Upload} from 'lucide-react';
import {formatBytesOf, formatEta, formatPercent, formatSpeed} from '../../lib/format';
import {TransferRow, type TransferTone} from '../downloads/TransferRow';
import {pauseUpload, resumeUpload, type UploadEntry} from './uploadQueue';

function stateWords(entry: UploadEntry): string {
  switch (entry.state) {
    case 'waiting':
      return 'Waiting for the uploads ahead of it';
    case 'starting':
      return 'Starting';
    case 'uploading':
      return entry.resumedFrom && entry.resumedFrom > 0 && entry.size > 0
        ? `Uploading, continued from ${formatPercent(entry.resumedFrom / entry.size)}`
        : 'Uploading';
    case 'finishing':
      return 'Finishing';
    case 'paused':
      return 'Paused';
    case 'failed':
      return 'Stopped';
    case 'interrupted':
      return 'Interrupted. Choose the same file again to continue';
    default:
      return 'Uploading';
  }
}

function toneOf(entry: UploadEntry): TransferTone {
  if (entry.state === 'failed') return 'failed';
  if (entry.state === 'paused' || entry.state === 'interrupted' || entry.state === 'waiting') return 'paused';
  return 'active';
}

export interface UploadRowProps {
  entry: UploadEntry;
  onChooseAgain: () => void;
  onCancel: (entry: UploadEntry) => void;
}

export function UploadRow({entry, onChooseAgain, onCancel}: UploadRowProps) {
  const sending = entry.state === 'uploading' || entry.state === 'starting';
  const progress = entry.size > 0 ? Math.min(1, entry.loaded / entry.size) : entry.state === 'finishing' ? 1 : 0;
  const etaSeconds = sending && entry.bytesPerSecond > 0 ? (entry.size - entry.loaded) / entry.bytesPerSecond : null;

  let control = null;
  if (entry.state === 'interrupted') {
    control = <Button label="Choose file" variant="secondary" onClick={onChooseAgain} />;
  } else if (entry.state === 'failed') {
    control = (
      <IconButton
        label={`Try ${entry.name} again`}
        tooltip="Try again"
        icon={<RotateCcw size={18} strokeWidth={1.75} />}
        variant="ghost"
        onClick={() => resumeUpload(entry.key)}
      />
    );
  } else if (entry.state === 'paused') {
    control = (
      <IconButton
        label={`Resume ${entry.name}`}
        tooltip="Resume"
        icon={<Play size={18} strokeWidth={1.75} />}
        variant="ghost"
        onClick={() => resumeUpload(entry.key)}
      />
    );
  } else if (entry.state !== 'finishing') {
    control = (
      <IconButton
        label={`Pause ${entry.name}`}
        tooltip="Pause"
        icon={<Pause size={18} strokeWidth={1.75} />}
        variant="ghost"
        onClick={() => pauseUpload(entry.key)}
      />
    );
  }

  return (
    <TransferRow
      icon={Upload}
      name={entry.name}
      stateText={`Upload · ${stateWords(entry)}`}
      progress={progress}
      tone={toneOf(entry)}
      meta={[
        formatBytesOf(entry.loaded, entry.size),
        sending && entry.bytesPerSecond > 0 ? formatSpeed(entry.bytesPerSecond) : null,
        etaSeconds !== null ? formatEta(etaSeconds) : null,
      ]}
      note={entry.state === 'failed' ? entry.error : null}
      actions={
        <>
          {control}
          <MoreMenu
            label={`More for ${entry.name}`}
            alignment="end"
            isDisabled={entry.state === 'finishing'}
            items={[
              {
                label: entry.state === 'interrupted' ? 'Discard upload…' : 'Cancel upload…',
                variant: 'destructive',
                onClick: () => onCancel(entry),
              },
            ]}
          />
        </>
      }
    />
  );
}

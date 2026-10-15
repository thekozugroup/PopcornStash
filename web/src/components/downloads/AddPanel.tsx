/**
 * The top of Downloads: one field for a web link or magnet with the primary
 * Add action, then the two other ways in (upload files, add a .torrent
 * file), then the VPN line when torrents are affected.
 *
 * The field says what it recognised while the person types: a magnet link,
 * a link to a .torrent file or a web link. Other schemes are turned down in
 * one plain sentence.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {CircleAlert, FileUp, Globe, Link2, Magnet, Upload, type LucideIcon} from 'lucide-react';
import {useState, type ChangeEvent, type FormEvent, type RefObject} from 'react';
import {ApiError, downloads as downloadsApi} from '../../api/client';
import type {Download, VpnStatus} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {checkLink, describeLink, isTorrentLink} from './linkDetect';
import {isNoVpnMode, showsVpnLine, torrentsOff, torrentsWait} from '../../lib/vpn';
import {VpnLine} from './VpnLine';
import {DestinationSelect} from './DestinationSelect';

const styles = stylex.create({
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    paddingBlock: {default: 24, '@media (max-width: 559px)': 18},
    paddingInline: {default: 28, '@media (max-width: 559px)': 16},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
    marginBottom: 40,
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  fieldRow: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-end',
    gap: 8,
  },
  field: {
    flex: '1 1 320px',
    minWidth: 0,
  },
  submit: {
    flex: {default: '0 0 auto', '@media (max-width: 559px)': '1 1 100%'},
    minWidth: 112,
  },
  feedback: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 8,
    minHeight: 20,
    margin: 0,
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
  },
  feedbackError: {
    color: colorVars['--color-error'],
  },
  feedbackIcon: {
    display: 'inline-flex',
    flexShrink: 0,
    marginTop: 2,
  },
  others: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
    paddingTop: 20,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  otherButton: {
    flex: {default: '0 0 auto', '@media (max-width: 559px)': '1 1 140px'},
  },
  dropHint: {
    // Touch screens cannot drop files.
    display: {default: 'inline', '@media (pointer: coarse)': 'none'},
    marginInlineStart: {default: 8, '@media (max-width: 559px)': 0},
    flexBasis: {default: 'auto', '@media (max-width: 559px)': '100%'},
  },
  vpn: {
    paddingTop: 16,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  hiddenInput: {
    display: 'none',
  },
});

const KIND_ICON: Record<'magnet' | 'torrent-url' | 'http', LucideIcon> = {
  magnet: Magnet,
  'torrent-url': FileUp,
  http: Globe,
};

export interface AddPanelProps {
  libraryId: string;
  onLibrary: (value: string) => void;
  isOwner: boolean;
  vpnStatus: VpnStatus | null;
  vpnUnavailable: boolean;
  /** The downloads could not be loaded: the server is not answering. */
  isServerQuiet?: boolean;
  uploadInputRef: RefObject<HTMLInputElement | null>;
  isAddingTorrents: boolean;
  onAdded: (download: Download) => void;
  onPickFiles: (files: File[]) => void;
  onPickTorrents: (files: File[]) => void;
}

export function AddPanel({
  libraryId, onLibrary,
  isOwner,
  vpnStatus,
  vpnUnavailable,
  isServerQuiet = false,
  uploadInputRef,
  isAddingTorrents,
  onAdded,
  onPickFiles,
  onPickTorrents,
}: AddPanelProps) {
  const [value, setValue] = useState('');
  const [touched, setTouched] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);
  const torrentInputId = 'downloads-torrent-input';

  const check = checkLink(value);
  const torrentsQueue = torrentsWait(vpnStatus, vpnUnavailable);
  const noVpn = isNoVpnMode(vpnStatus);

  let feedback: {text: string; icon: LucideIcon; isError: boolean};
  if (serverError) {
    feedback = {text: serverError, icon: CircleAlert, isError: true};
  } else if (check.status === 'invalid' && touched) {
    feedback = {text: check.message, icon: CircleAlert, isError: true};
  } else if (check.status === 'ok') {
    const isTorrent = isTorrentLink(check.link);
    const parts = [
      check.extracted ? 'Found a link in the pasted text.' : null,
      isTorrent && torrentsOff(vpnStatus)
        ? 'A torrent. Torrents are off on this server, so it cannot be added.'
        : describeLink(check.link, {viaVpn: !noVpn}),
    ];
    if (isTorrent && torrentsQueue) parts.push('It waits until the VPN is ready.');
    feedback = {text: parts.filter(Boolean).join(' '), icon: KIND_ICON[check.link.kind], isError: false};
  } else {
    // Never promise magnets on a server where torrents are off.
    feedback = {
      // Without a VPN the line below already says where torrents go.
      text: torrentsOff(vpnStatus)
        ? 'Web links download right away. Magnet links cannot start while torrents are off.'
        : noVpn
          ? 'Web links and magnet links both work.'
          : 'Web links and magnet links both work. Torrents only ever run through the VPN.',
      icon: Link2,
      isError: false,
    };
  }

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    setTouched(true);
    setServerError(null);
    if (check.status === 'empty') {
      setServerError('Paste a web link or a magnet link first.');
      return;
    }
    if (check.status === 'invalid') return;
    setIsBusy(true);
    try {
      const download = await downloadsApi.create({url: check.link.url, library_id: libraryId || undefined});
      onAdded(download);
      setValue('');
      setTouched(false);
    } catch (error) {
      if (error instanceof ApiError && error.code === 'vpn_unavailable') {
        setServerError(
          error.message ||
            'Torrents are turned off on this server, so magnet links cannot start. Web links and uploads still work.',
        );
      } else {
        setServerError(errorMessage(error, 'The link was not added. Try again.'));
      }
    } finally {
      setIsBusy(false);
    }
  };

  const onFilesChosen = (event: ChangeEvent<HTMLInputElement>, handler: (files: File[]) => void) => {
    const files = Array.from(event.target.files ?? []);
    // Clear so choosing the same file again (to resume it) fires a change.
    event.target.value = '';
    if (files.length > 0) handler(files);
  };

  const FeedbackIcon = feedback.icon;
  const showError = feedback.isError;

  return (
    <section aria-label="Add to downloads" {...stylex.props(styles.panel)}>
      <form onSubmit={event => void onSubmit(event)} noValidate {...stylex.props(styles.form)}>
        <div {...stylex.props(styles.fieldRow)}>
          <div {...stylex.props(styles.field)}>
            <TextInput
              label="Paste a link or magnet"
              value={value}
              onChange={next => {
                setValue(next);
                setServerError(null);
                if (!next.trim()) setTouched(false);
              }}
              placeholder="https://… or magnet:?xt=…"
              htmlName="link"
              autoComplete="off"
              hasClear
              isDisabled={isBusy}
              status={showError ? {type: 'error'} : undefined}
              width="100%"
            />
          </div>
          <div {...stylex.props(styles.submit)}>
            <Button type="submit" variant="primary" label="Add" isLoading={isBusy} width="100%" />
          </div>
        </div>
        {/* Only problems are announced; the description changes on every keystroke. */}
        <p
          key={showError ? 'error' : 'hint'}
          role={showError ? 'alert' : undefined}
          {...stylex.props(styles.feedback, showError && styles.feedbackError)}>
          <span {...stylex.props(styles.feedbackIcon)} aria-hidden="true">
            <FeedbackIcon size={15} strokeWidth={2} />
          </span>
          <span>{feedback.text}</span>
        </p>
      </form>

      <DestinationSelect value={libraryId} onChange={onLibrary} disabled={isBusy || isAddingTorrents} />
      <div {...stylex.props(styles.others)}>
        <input
          ref={uploadInputRef}
          type="file"
          multiple
          tabIndex={-1}
          aria-hidden="true"
          onChange={event => onFilesChosen(event, onPickFiles)}
          {...stylex.props(styles.hiddenInput)}
        />
        <input
          id={torrentInputId}
          type="file"
          multiple
          accept=".torrent,application/x-bittorrent"
          tabIndex={-1}
          aria-hidden="true"
          onChange={event => onFilesChosen(event, onPickTorrents)}
          {...stylex.props(styles.hiddenInput)}
        />
        <div {...stylex.props(styles.otherButton)}>
          <Button
            label="Upload files"
            variant="secondary"
            icon={<Upload size={16} strokeWidth={1.75} />}
            onClick={() => uploadInputRef.current?.click()}
            width="100%"
          />
        </div>
        <div {...stylex.props(styles.otherButton)}>
          <Button
            label="Add a .torrent file"
            variant="secondary"
            icon={<FileUp size={16} strokeWidth={1.75} />}
            isLoading={isAddingTorrents}
            onClick={() => (document.getElementById(torrentInputId) as HTMLInputElement | null)?.click()}
            width="100%"
          />
        </div>
        <Text type="supporting" color="secondary" xstyle={styles.dropHint}>
          Or drop files anywhere on this page.
        </Text>
      </div>

      {showsVpnLine(vpnStatus, vpnUnavailable, isServerQuiet) ? (
        <div {...stylex.props(styles.vpn)}>
          <VpnLine status={vpnStatus} isUnavailable={vpnUnavailable} isOwner={isOwner} />
        </div>
      ) : null}
    </section>
  );
}

/**
 * Share target (`/share?url&text&title`, from the PWA manifest). Takes the
 * first web link or magnet out of what another app shared, asks once, then
 * adds it to Downloads. The session gate in front of this route sends
 * signed-out visitors to sign in and back here with the same query.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {CircleCheck, FileUp, Globe, Link2Off, Magnet, type LucideIcon} from 'lucide-react';
import {useState} from 'react';
import {useNavigate, useSearchParams} from 'react-router';
import {ApiError, downloads as downloadsApi} from '../api/client';
import type {Download} from '../api/types';
import {useAuth} from '../auth/AuthProvider';
import {findLink, isTorrentLink, type DetectedLink} from '../components/downloads/linkDetect';
import {torrentsAffected, VpnLine} from '../components/downloads/VpnLine';
import {PageHeader} from '../components/PageHeader';
import {useVpnStatus} from '../hooks/useVpnStatus';
import {errorMessage} from '../lib/errors';

const styles = stylex.create({
  card: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    maxWidth: 600,
    paddingBlock: {default: 28, '@media (max-width: 559px)': 20},
    paddingInline: {default: 28, '@media (max-width: 559px)': 16},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
  },
  summary: {
    display: 'grid',
    gridTemplateColumns: {default: 'auto minmax(0, 1fr)', '@media (max-width: 559px)': 'minmax(0, 1fr)'},
    columnGap: 16,
    alignItems: 'start',
  },
  icon: {
    display: {default: 'grid', '@media (max-width: 559px)': 'none'},
    placeItems: 'center',
    width: 44,
    height: 44,
    borderRadius: 10,
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-secondary'],
  },
  iconDone: {
    backgroundColor: colorVars['--color-success-muted'],
    color: colorVars['--color-success'],
  },
  text: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
  },
  title: {
    overflowWrap: 'anywhere',
  },
  url: {
    display: '-webkit-box',
    WebkitBoxOrient: 'vertical',
    WebkitLineClamp: 3,
    overflow: 'hidden',
    margin: 0,
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    wordBreak: 'break-all',
  },
  shared: {
    margin: 0,
    paddingBlock: 10,
    paddingInline: 12,
    borderRadius: 8,
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
    whiteSpace: 'pre-wrap',
    maxHeight: 160,
    overflow: 'auto',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
  vpn: {
    paddingTop: 16,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
});

const KIND: Record<DetectedLink['kind'], {icon: LucideIcon; words: (link: DetectedLink) => string}> = {
  magnet: {icon: Magnet, words: () => 'Magnet link. Downloads as a torrent, only through the VPN.'},
  'torrent-url': {
    icon: FileUp,
    words: link => `Link to a .torrent file on ${link.host ?? 'the web'}. Downloads as a torrent, only through the VPN.`,
  },
  http: {icon: Globe, words: link => `Web link from ${link.host ?? 'the web'}.`},
};

function cleanTitle(value: string | null, link: DetectedLink | null): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  if (!trimmed || (link && trimmed.includes(link.url))) return null;
  return trimmed.length > 160 ? `${trimmed.slice(0, 157)}...` : trimmed;
}

export default function SharePage() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const {isOwner} = useAuth();
  const vpn = useVpnStatus();
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState<{message: string; isVpn: boolean} | null>(null);
  const [sent, setSent] = useState<Download | null>(null);

  const sharedUrl = params.get('url');
  const sharedText = params.get('text');
  const sharedTitle = params.get('title');
  const link = findLink(sharedUrl) ?? findLink(sharedText) ?? findLink(sharedTitle);
  const title = cleanTitle(sharedTitle, link) ?? link?.name ?? link?.host ?? 'Shared link';
  const received = [sharedTitle, sharedText, sharedUrl].filter(Boolean).join('\n');

  const send = async () => {
    if (!link || isSending) return;
    setIsSending(true);
    setError(null);
    try {
      setSent(await downloadsApi.create({url: link.url}));
    } catch (cause) {
      if (cause instanceof ApiError && cause.code === 'vpn_unavailable') {
        setError({
          message:
            cause.message ||
            'Magnet links need the VPN, and torrents are turned off on this server. Web links still work.',
          isVpn: true,
        });
      } else {
        setError({message: errorMessage(cause, 'The link was not added. Try again.'), isVpn: false});
      }
    } finally {
      setIsSending(false);
    }
  };

  if (sent) {
    const waiting = sent.state === 'queued';
    return (
      <>
        <PageHeader title="Added to Downloads" documentTitle="Add shared link" />
        <section {...stylex.props(styles.card)} aria-labelledby="share-done-title">
          <div {...stylex.props(styles.summary)}>
            <span {...stylex.props(styles.icon, styles.iconDone)} aria-hidden="true">
              <CircleCheck size={22} strokeWidth={1.75} />
            </span>
            <div {...stylex.props(styles.text)}>
              <Heading level={2} id="share-done-title">
                <span {...stylex.props(styles.title)}>{sent.name}</span>
              </Heading>
              <Text as="p" color="secondary">
                {waiting
                  ? sent.error ?? 'It is waiting to start. It appears under In progress in Downloads.'
                  : 'It is downloading now and appears under In progress in Downloads. When it finishes, it is sorted into your library.'}
              </Text>
            </div>
          </div>
          <div {...stylex.props(styles.actions)}>
            <Button label="Open Downloads" variant="primary" href="/downloads" />
            <Button label="Done" variant="secondary" onClick={() => navigate('/', {replace: true})} />
          </div>
        </section>
      </>
    );
  }

  if (!link) {
    return (
      <>
        <PageHeader title="Add shared link" subtitle="Shared from another app." />
        <section {...stylex.props(styles.card)} aria-labelledby="share-none-title">
          <div {...stylex.props(styles.summary)}>
            <span {...stylex.props(styles.icon)} aria-hidden="true">
              <Link2Off size={22} strokeWidth={1.75} />
            </span>
            <div {...stylex.props(styles.text)}>
              <Heading level={2} id="share-none-title">
                Nothing to download in what was shared
              </Heading>
              <Text as="p" color="secondary">
                PopcornStash can take web links and magnet links. What arrived did not include one.
              </Text>
            </div>
          </div>
          {received ? (
            <div>
              <Text as="p" type="supporting" color="secondary">
                What arrived
              </Text>
              <p {...stylex.props(styles.shared)}>{received}</p>
            </div>
          ) : null}
          <div {...stylex.props(styles.actions)}>
            <Button label="Open Downloads" variant="secondary" href="/downloads" />
          </div>
        </section>
      </>
    );
  }

  const kind = KIND[link.kind];
  const KindIcon = kind.icon;
  const showVpn = isTorrentLink(link) && torrentsAffected(vpn.status, vpn.isUnavailable);

  return (
    <>
      <PageHeader title="Add shared link" subtitle="Check the link, then add it to Downloads." />
      <section {...stylex.props(styles.card)} aria-labelledby="share-title">
        <div {...stylex.props(styles.summary)}>
          <span {...stylex.props(styles.icon)} aria-hidden="true">
            <KindIcon size={22} strokeWidth={1.75} />
          </span>
          <div {...stylex.props(styles.text)}>
            <Heading level={2} id="share-title">
              <span {...stylex.props(styles.title)}>{title}</span>
            </Heading>
            <Text as="p" color="secondary">
              {showVpn ? `${kind.words(link)} It waits until the VPN is ready.` : kind.words(link)}
            </Text>
            <p {...stylex.props(styles.url)} title={link.url}>
              {link.url}
            </p>
          </div>
        </div>

        {showVpn ? (
          <div {...stylex.props(styles.vpn)}>
            <VpnLine status={vpn.status} isUnavailable={vpn.isUnavailable} isOwner={isOwner} />
          </div>
        ) : null}

        {error ? (
          <Banner
            status="error"
            title="Not added"
            description={error.message}
            collapsible={false}
            endContent={
              // The VPN line above already links to the settings when it is shown.
              error.isVpn && isOwner && !showVpn ? (
                <Button label="Open VPN settings" variant="secondary" href="/settings/vpn" />
              ) : undefined
            }
          />
        ) : null}

        <div {...stylex.props(styles.actions)}>
          <Button label="Add to Downloads" variant="primary" isLoading={isSending} onClick={() => void send()} />
          <Button label="Cancel" variant="secondary" isDisabled={isSending} onClick={() => navigate('/', {replace: true})} />
        </div>
      </section>
    </>
  );
}

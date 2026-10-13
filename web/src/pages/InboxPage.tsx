/**
 * Inbox: files the sorter was not sure about. Each one shows why it waited
 * and where it would go; nothing moves until someone accepts, edits or
 * ignores it. The list follows `inbox` events, so files that arrive or are
 * decided on another device appear and disappear on their own.
 *
 * After a decision the row leaves the list, so keyboard focus moves on to the
 * next file's actions (or to the page when Inbox is empty).
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {useToast} from '@astryxdesign/core/Toast';
import {Inbox} from 'lucide-react';
import {useEffect, useState} from 'react';
import {inbox as inboxApi} from '../api/client';
import type {InboxDecision, ItemDetail} from '../api/types';
import {useAuth} from '../auth/AuthProvider';
import {ListSkeleton} from '../components/downloads/ListSkeleton';
import {EmptyPanel} from '../components/EmptyPanel';
import {InboxRow} from '../components/inbox/InboxRow';
import {baseName, describeDestination} from '../components/inbox/places';
import {useInbox} from '../components/inbox/useInbox';
import {PageHeader} from '../components/PageHeader';
import {errorMessage} from '../lib/errors';
import {formatCount} from '../lib/format';

const styles = stylex.create({
  panel: {
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
    overflow: 'clip',
  },
  list: {
    margin: 0,
    padding: 0,
    listStyle: 'none',
  },
  item: {
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  banner: {
    marginBottom: 24,
  },
});

type Busy = {id: string; kind: 'accept' | 'ignore' | 'edit'} | {id: '*'; kind: 'all'};

const MAIN_ID = 'astryx-app-shell-main';

export default function InboxPage() {
  const {isOwner} = useAuth();
  const showToast = useToast();
  const {status, error, items, reload, removeItem} = useInbox();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Busy | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);
  const [allProgress, setAllProgress] = useState<{done: number; total: number} | null>(null);
  /** Where keyboard focus goes once a decided row has left the list (null: the page). */
  const [focusNext, setFocusNext] = useState<{id: string | null} | null>(null);

  const withSuggestion = items.filter(item => item.suggestion !== null);
  const count = items.length;

  const neighbourOf = (id: string): string | null => {
    const index = items.findIndex(item => item.id === id);
    return (items[index + 1] ?? items[index - 1])?.id ?? null;
  };

  const decide = async (item: ItemDetail, decision: InboxDecision): Promise<boolean> => {
    await inboxApi.decide(item.id, decision);
    removeItem(item.id);
    return true;
  };

  // Focus left on a removed row (or nowhere) moves to the next file's actions.
  useEffect(() => {
    if (!focusNext) return;
    const timer = window.setTimeout(() => {
      setFocusNext(null);
      const active = document.activeElement;
      if (active && active !== document.body && document.contains(active)) return;
      const target = focusNext.id
        ? document.querySelector<HTMLElement>(`[data-inbox-id="${CSS.escape(focusNext.id)}"] [data-inbox-actions] button`)
        : null;
      (target ?? document.getElementById(MAIN_ID))?.focus({preventScroll: !target});
    }, 0);
    return () => window.clearTimeout(timer);
  }, [focusNext, items]);

  const accept = async (item: ItemDetail) => {
    if (!item.suggestion) return;
    setBusy({id: item.id, kind: 'accept'});
    const next = neighbourOf(item.id);
    try {
      await decide(item, {action: 'accept'});
      setFocusNext({id: next});
      showToast({body: `Moved to ${describeDestination(item.suggestion.destination)}.`});
    } catch (cause) {
      showToast({body: errorMessage(cause, `${baseName(item.rel_path)} did not move. Try again.`), type: 'error'});
    } finally {
      setBusy(null);
    }
  };

  const ignore = async (item: ItemDetail) => {
    setBusy({id: item.id, kind: 'ignore'});
    const next = neighbourOf(item.id);
    try {
      await decide(item, {action: 'ignore'});
      setFocusNext({id: next});
      showToast({body: `Ignored. ${baseName(item.rel_path)} stays where it is.`});
    } catch (cause) {
      showToast({body: errorMessage(cause, 'That did not work. Try again.'), type: 'error'});
    } finally {
      setBusy(null);
    }
  };

  const submitEdit = async (item: ItemDetail, decision: InboxDecision, destination: string) => {
    setBusy({id: item.id, kind: 'edit'});
    setEditError(null);
    const next = neighbourOf(item.id);
    try {
      await decide(item, decision);
      setEditingId(null);
      setFocusNext({id: next});
      showToast({body: `Moved to ${destination}.`});
    } catch (cause) {
      setEditError(errorMessage(cause, 'The file did not move. Check the details and try again.'));
    } finally {
      setBusy(null);
    }
  };

  const acceptAll = async () => {
    const targets = withSuggestion;
    setBusy({id: '*', kind: 'all'});
    setAllProgress({done: 0, total: targets.length});
    let moved = 0;
    let failed = 0;
    for (const item of targets) {
      try {
        await decide(item, {action: 'accept'});
        moved += 1;
      } catch {
        failed += 1;
      }
      setAllProgress({done: moved + failed, total: targets.length});
    }
    setBusy(null);
    setAllProgress(null);
    setConfirmAll(false);
    setFocusNext({id: items.find(item => item.suggestion === null)?.id ?? null});
    if (failed === 0) {
      showToast({body: `${formatCount(moved, 'file')} moved to their suggested places.`});
    } else {
      showToast({
        body: `${formatCount(moved, 'file')} moved. ${formatCount(failed, 'file')} did not move and ${failed === 1 ? 'is' : 'are'} still here.`,
        type: 'error',
      });
    }
  };

  const subtitle =
    status === 'ready' && count > 0
      ? `${count === 1 ? 'One file needs' : `${formatCount(count, 'file')} need`} a decision. Nothing moves until you choose.`
      : 'Files PopcornStash was not sure about wait here for one look before they are sorted.';

  return (
    <>
      <PageHeader
        title="Inbox"
        subtitle={subtitle}
        actions={
          withSuggestion.length >= 2 && status === 'ready' ? (
            <Button
              label="Accept all suggestions"
              variant="secondary"
              isDisabled={busy !== null}
              onClick={() => setConfirmAll(true)}
            />
          ) : undefined
        }
      />

      {status === 'error' ? (
        <Banner
          status="error"
          title="Inbox could not be loaded"
          description={error ?? 'PopcornStash did not answer. Check the connection and try again.'}
          collapsible={false}
          endContent={<Button label="Try again" variant="secondary" onClick={reload} />}
          xstyle={styles.banner}
        />
      ) : null}

      {status === 'loading' ? (
        <div {...stylex.props(styles.panel)}>
          <ListSkeleton label="Loading Inbox" rows={3} thumb="frame" />
        </div>
      ) : null}

      {status === 'ready' && count === 0 ? (
        <EmptyPanel
          title="Nothing needs a decision"
          description="New files are sorted automatically. Anything PopcornStash is unsure about will wait here."
          icon={<Inbox size={28} strokeWidth={1.5} />}
        />
      ) : null}

      {status === 'ready' && count > 0 ? (
        <div {...stylex.props(styles.panel)}>
          <ul {...stylex.props(styles.list)} aria-label="Files waiting for a decision">
            {items.map((item, index) => (
              <li key={item.id} {...stylex.props(styles.item)}>
                <InboxRow
                  item={item}
                  isOwner={isOwner}
                  isNext={index === 0}
                  isEditing={editingId === item.id}
                  busy={busy && busy.id === item.id && busy.kind !== 'all' ? busy.kind : null}
                  error={editingId === item.id ? editError : null}
                  isLocked={busy !== null}
                  onAccept={target => void accept(target)}
                  onIgnore={target => void ignore(target)}
                  onEdit={target => {
                    setEditError(null);
                    setEditingId(target ? target.id : null);
                  }}
                  onSubmitEdit={(target, decision, destination) => void submitEdit(target, decision, destination)}
                />
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      <AlertDialog
        isOpen={confirmAll}
        onOpenChange={open => {
          if (!open && busy === null) setConfirmAll(false);
        }}
        title={`Accept all ${withSuggestion.length} suggestions?`}
        description={
          allProgress
            ? `Moving ${allProgress.done + 1 > allProgress.total ? allProgress.total : allProgress.done + 1} of ${allProgress.total}…`
            : [
                'Each file moves to the place shown in its row.',
                count > withSuggestion.length
                  ? `${count - withSuggestion.length === 1 ? 'One file' : formatCount(count - withSuggestion.length, 'file')} without a suggestion ${count - withSuggestion.length === 1 ? 'stays' : 'stay'} in Inbox.`
                  : null,
              ]
                .filter(Boolean)
                .join(' ')
        }
        actionLabel="Accept all"
        actionVariant="primary"
        isActionLoading={busy?.kind === 'all'}
        onAction={() => void acceptAll()}
      />
    </>
  );
}

/**
 * Inbox: files that wait for a decision. Each row says why it waits (when
 * that adds something) and shows where it goes in a Library selector, with
 * the suggestion preselected; nothing moves until someone accepts, edits or
 * ignores it. Accept always sends the library the row shows, never a later
 * guess. Ignored files sit after the waiting ones under a closed disclosure,
 * with the same row and Accept.
 *
 * The list follows `inbox` and `libraries` events, so files that arrive or
 * are decided on another device, and suggestions re-pointed by a library
 * change, appear on their own. After a decision the row leaves the list, so
 * keyboard focus moves on to the next file's actions (or to the page).
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Collapsible} from '@astryxdesign/core/Collapsible';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {HStack} from '@astryxdesign/core/Layout';
import {useToast} from '@astryxdesign/core/Toast';
import {Inbox} from 'lucide-react';
import {useEffect, useState} from 'react';
import {ApiError, downloads as downloadsApi, inbox as inboxApi} from '../api/client';
import type {InboxDecision, ItemDetail} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {useAuth} from '../auth/AuthProvider';
import {ListSkeleton} from '../components/downloads/ListSkeleton';
import {EmptyPanel} from '../components/EmptyPanel';
import {InboxRow} from '../components/inbox/InboxRow';
import {bulkReady, matchingInbox} from '../components/inbox/bulk';
import {fieldOfServerError, targetSelectorOptions, type MoveFieldErrors} from '../components/inbox/MoveFields';
import {
  baseName,
  needsDetails,
  resolveTarget,
  suggestedTarget,
  targetChoices,
  targetName,
  targetValue,
  type TargetValue,
} from '../components/inbox/places';
import {useInbox} from '../components/inbox/useInbox';
import {PageHeader} from '../components/PageHeader';
import {errorMessage} from '../lib/errors';
import {formatCount} from '../lib/format';

const styles = stylex.create({
  panel: {
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
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
  ignored: {
    marginTop: 32,
  },
  ignoredBody: {
    paddingTop: 12,
  },
});

type Busy = {id: string; kind: 'accept' | 'ignore' | 'edit'} | {id: '*'; kind: 'all'};

const MAIN_ID = 'astryx-app-shell-main';
const NO_ERRORS: MoveFieldErrors = {};

export default function InboxPage() {
  const {isOwner} = useAuth();
  const showToast = useToast();
  const {libraries: listed} = useLibraries();
  const libraries = listed ?? [];
  const {status, error, items, ignored, reload, removeItem} = useInbox();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<MoveFieldErrors>({});
  const [busy, setBusy] = useState<Busy | null>(null);
  const [downloadFilter, setDownloadFilter] = useState('all');
  const [typeFilter, setTypeFilter] = useState('all');
  const [reviewFilter, setReviewFilter] = useState('all');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkTarget, setBulkTarget] = useState('rows');
  const [confirmSelected, setConfirmSelected] = useState(false);
  const [downloadNames, setDownloadNames] = useState<Record<string, string>>({});
  useEffect(() => {
    const controller = new AbortController();
    void downloadsApi.list('all', 200, controller.signal).then(result => {
      setDownloadNames(Object.fromEntries(result.items.map(download => [download.id, download.name])));
    }).catch(() => undefined);
    return () => controller.abort();
  }, []);
  const [confirmAll, setConfirmAll] = useState(false);
  const [allProgress, setAllProgress] = useState<{done: number; total: number} | null>(null);
  /** What each row's selector shows, once the person changed it. */
  const [chosen, setChosen] = useState<Record<string, TargetValue | null>>({});
  /** Where keyboard focus goes once a decided row has left the list (null: the page). */
  const [focusNext, setFocusNext] = useState<{id: string | null} | null>(null);

  const choicesOf = (item: ItemDetail) => targetChoices(item, libraries, isOwner);
  /** The row's target: the person's choice, else the suggestion when it is one of the choices. */
  const targetOf = (item: ItemDetail): TargetValue | null => {
    const choices = choicesOf(item).map(targetValue);
    const picked = chosen[item.id];
    if (picked !== undefined && (picked === null || choices.includes(picked))) return picked;
    const suggested = suggestedTarget(item, libraries);
    if (suggested && choices.includes(suggested)) return suggested;
    // One place to go (a document: Other files) needs no choosing.
    return choices.length === 1 ? (choices[0] ?? null) : null;
  };

  const resetFilters = (change: () => void) => { setSelected(new Set()); change(); };
  const visibleItems = matchingInbox(reviewFilter === 'ignored' ? [] : items, downloadFilter, typeFilter);
  const visibleIgnored = matchingInbox(reviewFilter === 'waiting' ? [] : ignored, downloadFilter, typeFilter);
  const matching = [...visibleItems, ...visibleIgnored];
  const selection = matching.filter(item => selected.has(item.id));
  const commonTargets = selection.length ? choicesOf(selection[0]!).filter(choice => selection.every(item => choicesOf(item).some(candidate => targetValue(candidate) === targetValue(choice)))) : [];
  const selectionReady = bulkReady(selection, libraries, isOwner, item => bulkTarget === 'rows' ? targetOf(item) : bulkTarget as TargetValue);
  const sourceIds = [...new Set([...items, ...ignored].map(item => item.source_download_id).filter((id): id is string => id !== null))];
  const ready = items.filter(item => {
    const target = resolveTarget(targetOf(item), libraries);
    return target !== null && !needsDetails(target, item);
  });
  const count = items.length;
  const unchosen = count - ready.length;

  const neighbourOf = (id: string): string | null => {
    const all = [...items, ...ignored];
    const index = all.findIndex(item => item.id === id);
    return (all[index + 1] ?? all[index - 1])?.id ?? null;
  };

  const decide = async (item: ItemDetail, decision: InboxDecision) => {
    await inboxApi.decide(item.id, decision);
    removeItem(item.id);
    setSelected(current => new Set([...current].filter(id => id !== item.id)));
  };

  /** The request for a plain Accept: exactly the place and library the row shows. */
  const acceptDecision = (value: TargetValue): InboxDecision | null => {
    const target = resolveTarget(value, libraries);
    if (!target) return null;
    return target.library
      ? {action: 'accept', place: 'library', library_id: target.library.id}
      : {action: 'accept', place: 'other_files'};
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

  const accept = async (item: ItemDetail, value: TargetValue) => {
    const decision = acceptDecision(value);
    const target = resolveTarget(value, libraries);
    if (!decision || !target) return;
    setBusy({id: item.id, kind: 'accept'});
    const next = neighbourOf(item.id);
    try {
      await decide(item, decision);
      setFocusNext({id: next});
      showToast({body: `Moved to ${targetName(target)}.`});
    } catch (cause) {
      showToast({body: errorMessage(cause, `${baseName(item.rel_path)} did not move. Try again.`), type: 'error'});
      // A target that changed meanwhile (a claim moved, a library was hidden): show what is true now.
      if (cause instanceof ApiError && (cause.code === 'conflict' || cause.code === 'not_found')) reload();
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
      reload();
    } catch (cause) {
      showToast({body: errorMessage(cause, 'That did not work. Try again.'), type: 'error'});
    } finally {
      setBusy(null);
    }
  };

  const submitEdit = async (item: ItemDetail, decision: InboxDecision, destination: string, value: TargetValue) => {
    setBusy({id: item.id, kind: 'edit'});
    setEditError(null);
    setFieldErrors({});
    setChosen(current => ({...current, [item.id]: value}));
    const next = neighbourOf(item.id);
    try {
      await decide(item, decision);
      setEditingId(null);
      setFocusNext({id: next});
      showToast({body: `Moved to ${destination}.`});
    } catch (cause) {
      const field = cause instanceof ApiError ? fieldOfServerError(cause.field) : null;
      const message = errorMessage(cause, 'The file did not move. Check the details and try again.');
      if (field) setFieldErrors({[field]: message});
      else setEditError(message);
    } finally {
      setBusy(null);
    }
  };

  const acceptAll = async (onlySelected = false) => {
    const targets = onlySelected ? selectionReady : ready.map(item => ({item, value: targetOf(item)}));
    setBusy({id: '*', kind: 'all'});
    setAllProgress({done: 0, total: targets.length});
    let moved = 0;
    let failed = 0;
    for (const {item, value} of targets) {
      const decision = value ? acceptDecision(value) : null;
      try {
        if (!decision) throw new Error('no target');
        await decide(item, decision);
        moved += 1;
      } catch {
        failed += 1;
      }
      setAllProgress({done: moved + failed, total: targets.length});
    }
    setBusy(null);
    setAllProgress(null);
    setConfirmAll(false);
    setConfirmSelected(false);
    setFocusNext({id: items.find(item => !ready.includes(item))?.id ?? null});
    if (failed === 0) {
      showToast({body: `${formatCount(moved, 'file')} moved to the places shown.`});
    } else {
      showToast({
        body: `${formatCount(moved, 'file')} moved. ${formatCount(failed, 'file')} did not move and ${failed === 1 ? 'is' : 'are'} still here.`,
        type: 'error',
      });
      reload();
    }
  };

  const subtitle =
    status === 'ready' && visibleItems.length > 0
      ? `${count === 1 ? 'One file needs' : `${formatCount(count, 'file')} need`} a decision. Nothing moves until you choose.`
      : 'Files PopcornStash is not sure about wait here until you choose where they go.';

  const renderRow = (item: ItemDetail, index: number, isIgnored: boolean) => (
    <li key={item.id} {...stylex.props(styles.item)}>
      <InboxRow
        item={item}
        selected={selected.has(item.id)}
        onSelect={checked => setSelected(current => { const next = new Set(current); if (checked) next.add(item.id); else next.delete(item.id); return next; })}
        libraries={libraries}
        choices={choicesOf(item)}
        target={targetOf(item)}
        onTargetChange={(row, value) => setChosen(current => ({...current, [row.id]: value}))}
        isOwner={isOwner}
        isNext={!isIgnored && index === 0}
        isIgnored={isIgnored}
        isEditing={editingId === item.id}
        busy={busy && busy.id === item.id && busy.kind !== 'all' ? busy.kind : null}
        error={editingId === item.id ? editError : null}
        fieldErrors={editingId === item.id ? fieldErrors : NO_ERRORS}
        onClearServerErrors={() => {
          if (Object.keys(fieldErrors).length) setFieldErrors(NO_ERRORS);
        }}
        isLocked={busy !== null}
        onAccept={(row, value) => void accept(row, value)}
        onIgnore={row => void ignore(row)}
        onEdit={row => {
          setEditError(null);
          setFieldErrors({});
          setEditingId(row ? row.id : null);
        }}
        onSubmitEdit={(row, decision, destination, value) => void submitEdit(row, decision, destination, value)}
      />
    </li>
  );

  return (
    <>
      <PageHeader
        title="Inbox"
        subtitle={subtitle}
        actions={<HStack gap={2} wrap="wrap">
          {isOwner ? <Button label="Clean files" variant="secondary" href="/clean?folder=Inbox" /> : null}
          {ready.length >= 2 && status === 'ready' ? (
            <Button
              label="Accept all suggestions"
              variant="secondary"
              isDisabled={busy !== null}
              onClick={() => setConfirmAll(true)}
            />
          ) : null}
        </HStack>}
      />

      {status === 'ready' && items.length + ignored.length > 0 ? (
        <HStack gap={3} wrap="wrap" vAlign="end" paddingBlock={4}>
          <Selector label="Download" value={downloadFilter} options={[
            {value: 'all', label: 'Every download'}, {value: 'none', label: 'Uploads and copied files'},
            ...sourceIds.map((id, index) => ({value: id, label: downloadNames[id] ?? `Download ${index + 1}`})),
          ]} onChange={value => resetFilters(() => setDownloadFilter(value))} />
          <Selector label="File type" value={typeFilter} options={[
            {value: 'all', label: 'Every file type'}, {value: 'video', label: 'Videos'}, {value: 'photo', label: 'Images'},
            {value: 'subtitle', label: 'Subtitles'}, {value: 'audio', label: 'Audio'}, {value: 'other', label: 'Other files'},
          ]} onChange={value => resetFilters(() => setTypeFilter(value))} />
          <Selector label="Review" value={reviewFilter} options={[{value: 'waiting', label: 'Waiting'}, {value: 'ignored', label: 'Ignored'}, {value: 'all', label: 'Waiting and ignored'}]} onChange={value => resetFilters(() => setReviewFilter(value))} />
          <Button label={`Select matching (${matching.length})`} variant="secondary" isDisabled={busy !== null || matching.length === 0} onClick={() => setSelected(new Set(matching.map(item => item.id)))} />
          {selection.length ? <>
            <Selector label="Import selected to" value={bulkTarget} options={[{value: 'rows', label: 'Each file’s chosen library'}, ...targetSelectorOptions(commonTargets)]} onChange={setBulkTarget} isDisabled={busy !== null} />
            <Button label={`Import selected (${selectionReady.length})`} variant="primary" isDisabled={busy !== null || selectionReady.length === 0} onClick={() => setConfirmSelected(true)} />
            <Button label="Clear selection" variant="ghost" isDisabled={busy !== null} onClick={() => setSelected(new Set())} />
            {selectionReady.length < selection.length ? <Text type="supporting">{selection.length - selectionReady.length} need a compatible library or episode details.</Text> : null}
          </> : null}
          {isOwner ? <Text type="supporting" color="secondary">Unindexed subtitles, artwork and instructions are in Clean files.</Text> : null}
        </HStack>
      ) : null}
      {status === 'ready' && items.length + ignored.length > 0 && matching.length === 0 ? <Text as="p">No files match these filters.</Text> : null}
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

      {status === 'ready' && count === 0 && ignored.length === 0 ? (
        <EmptyPanel
          title="Nothing needs a decision"
          description="Everything new was sorted on its own."
          icon={<Inbox size={28} strokeWidth={1.5} />}
        />
      ) : null}

      {status === 'ready' && visibleItems.length > 0 ? (
        <div {...stylex.props(styles.panel)}>
          <ul {...stylex.props(styles.list)} aria-label="Files waiting for a decision">
            {visibleItems.map((item, index) => renderRow(item, index, false))}
          </ul>
        </div>
      ) : null}

      {status === 'ready' && visibleIgnored.length > 0 ? (
        <Collapsible
          trigger={<Text weight="semibold">{`Left where they are (${visibleIgnored.length})`}</Text>}
          key={reviewFilter}
          defaultIsOpen={reviewFilter === 'ignored'}
          chevronPosition="start"
          xstyle={styles.ignored}>
          <div {...stylex.props(styles.ignoredBody)}>
            <div {...stylex.props(styles.panel)}>
              <ul {...stylex.props(styles.list)} aria-label="Ignored files">
                {visibleIgnored.map((item, index) => renderRow(item, index, true))}
              </ul>
            </div>
          </div>
        </Collapsible>
      ) : null}

      <AlertDialog
        isOpen={confirmSelected}
        onOpenChange={open => { if (!open && busy === null) setConfirmSelected(false); }}
        title={`Import ${selectionReady.length} selected files?`}
        description={allProgress ? `Moving ${allProgress.done} of ${allProgress.total}…` : 'Only the selected files with a compatible destination and complete details will move. Unselected files stay here.'}
        actionLabel="Import selected"
        actionVariant="primary"
        isActionLoading={busy?.kind === 'all'}
        onAction={() => void acceptAll(true)}
      />
      <AlertDialog
        isOpen={confirmAll}
        onOpenChange={open => {
          if (!open && busy === null) setConfirmAll(false);
        }}
        title={`Accept all ${ready.length} suggestions?`}
        description={
          allProgress
            ? `Moving ${allProgress.done + 1 > allProgress.total ? allProgress.total : allProgress.done + 1} of ${allProgress.total}…`
            : [
                'Each file moves to the library shown in its row.',
                unchosen > 0 ? `Choose a library for ${formatCount(unchosen, 'file')}.` : null,
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

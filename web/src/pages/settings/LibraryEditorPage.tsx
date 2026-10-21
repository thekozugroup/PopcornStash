/**
 * The library editor (`/settings/libraries/new`, `/settings/libraries/:id`),
 * owners only: a page inside Settings with a back link to Libraries, the
 * library's name (or "New library") as its heading, and one grouped form:
 *
 * 1. Name and icon, with a preview of the sidebar row.
 * 2. What it holds: Videos (Series or One-offs) or Photos; read-only once
 *    the library holds files, with the reason.
 * 3. New files: the kinds that fit, each saying where it goes now; taking a
 *    kind from another library is confirmed before saving.
 * 4. Folder: a new one or one that is already there (never moved); fixed
 *    once made.
 * 5. Who sees it: everyone, or only the people chosen (owners see all).
 * 6. Delete (editing only): the files stay where they are.
 *
 * One primary action at the end ("Make library" or "Save changes"). Errors
 * sit under the field the server names (`error.field`).
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxList, CheckboxListItem} from '@astryxdesign/core/CheckboxList';
import {Heading} from '@astryxdesign/core/Heading';
import {RadioList, RadioListItem} from '@astryxdesign/core/RadioList';
import {Selector} from '@astryxdesign/core/Selector';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useToast} from '@astryxdesign/core/Toast';
import {ChevronLeft} from 'lucide-react';
import {useEffect, useMemo, useRef, useState, type FormEvent} from 'react';
import {useNavigate, useParams, useSearchParams} from 'react-router';
import {ApiError, libraries as librariesApi, users as usersApi} from '../../api/client';
import type {
  ClaimKind,
  ContentType,
  Id,
  Library,
  LibraryCreateRequest,
  LibraryEditRequest,
  LibraryIcon,
  LibraryVisibility,
  Structure,
  UnassignedFolder,
  User,
} from '../../api/types';
import {useLibraries} from '../../app/LibrariesProvider';
import {useDocumentTitle} from '../../components/PageHeader';
import {IconPicker} from '../../components/settings/IconPicker';
import {DeleteLibraryDialog} from '../../components/settings/DeleteLibraryDialog';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {LoadError} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {StatusTag} from '../../components/StatusTag';
import {useConfirm} from '../../components/settings/useConfirm';
import {errorMessage} from '../../lib/errors';
import {formatCount} from '../../lib/format';
import {CLAIM_LABELS, CLAIM_WORDS, claimFits, fittingClaims, holdsLine, joinWords} from '../../lib/libraries';
import {KNOWN_LIBRARY_ICONS, LibraryGlyph, defaultIconFor} from '../../lib/libraryIcons';
import {folderCounts} from './LibrariesSettingsPage';

const NAME_MAX = 40;

const styles = stylex.create({
  back: {
    marginInlineStart: -12,
    marginBottom: 8,
  },
  title: {
    marginBottom: 16,
  },
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
  field: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    maxWidth: 400,
    minWidth: 0,
  },
  previewBlock: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  // The sidebar row as it will look: icon and name on the selected navigation fill.
  sidebarRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    width: 212,
    maxWidth: '100%',
    minHeight: 44,
    paddingInline: 10,
    borderRadius: 'var(--radius-element)',
    backgroundColor: colorVars['--color-neutral'],
    color: colorVars['--color-text-primary'],
    fontSize: 14,
    fontWeight: fontWeightVars['--font-weight-medium'],
  },
  sidebarName: {
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  sidebarIcon: {
    flexShrink: 0,
  },
  code: {
    fontFamily: typographyVars['--font-family-code'],
  },
  // A CheckboxList row is a list item with an inline inset and a tinted fill when
  // checked, where a RadioList row is bare. Bare here too: each checkbox sits in the
  // radios' column, and a choice shows by its own mark, as a radio's does.
  checkboxRow: {
    paddingInline: 0,
    backgroundColor: 'transparent',
  },
  preview: {
    margin: 0,
    lineHeight: 1.55,
    maxWidth: '58ch',
  },
  fact: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'baseline',
    columnGap: 12,
    rowGap: 4,
  },
  factValue: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.875rem',
    color: colorVars['--color-text-primary'],
    overflowWrap: 'anywhere',
  },
  footer: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
    paddingTop: 32,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  banner: {
    marginBottom: 20,
  },
  skeleton: {
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
    marginTop: 8,
  },
});

type FieldKey = 'name' | 'icon' | 'type' | 'claims' | 'folder' | 'visibility';

/** Where a server error's `field` sits in the form. */
function fieldOf(field: string | null): FieldKey | null {
  switch (field) {
    case 'name':
      return 'name';
    case 'icon':
      return 'icon';
    case 'content_type':
    case 'structure':
      return 'type';
    case 'claims':
      return 'claims';
    case 'folder':
    case 'folder.mode':
    case 'folder.name':
      return 'folder';
    case 'visibility':
    case 'member_ids':
      return 'visibility';
    default:
      return null;
  }
}

interface Draft {
  name: string;
  icon: LibraryIcon;
  /** The owner picked an icon: changing the type no longer changes it. */
  iconTouched: boolean;
  contentType: ContentType;
  structure: Structure | null;
  claims: ClaimKind[];
  folderMode: 'new' | 'existing';
  newFolder: string;
  /** The owner edited the folder name: it no longer follows the library name. */
  newFolderTouched: boolean;
  existingFolder: string | null;
  visibility: LibraryVisibility;
  memberIds: Id[];
}

function draftFrom(library: Library): Draft {
  return {
    name: library.name,
    icon: library.icon,
    iconTouched: true,
    contentType: library.content_type,
    structure: library.structure,
    claims: [...library.claims],
    folderMode: 'existing',
    newFolder: '',
    newFolderTouched: true,
    existingFolder: library.folder,
    visibility: library.visibility ?? 'everyone',
    memberIds: [...(library.member_ids ?? [])],
  };
}

function newDraft(folder: UnassignedFolder | null, folderName: string | null): Draft {
  if (folderName) {
    // A folder that is already there: its files were seen by owners only until now,
    // so it starts on "Only some people" with nobody chosen.
    const contentType = folder?.suggested.content_type ?? 'video';
    return {
      name: folderName.slice(0, NAME_MAX),
      icon: defaultIconFor(contentType),
      iconTouched: false,
      contentType,
      structure: contentType === 'video' ? (folder?.suggested.structure ?? 'one_offs') : null,
      claims: [],
      folderMode: 'existing',
      newFolder: '',
      newFolderTouched: false,
      existingFolder: folderName,
      visibility: 'selected',
      memberIds: [],
    };
  }
  return {
    name: '',
    icon: 'film',
    iconTouched: false,
    contentType: 'video',
    structure: 'one_offs',
    claims: [],
    folderMode: 'new',
    newFolder: '',
    newFolderTouched: false,
    existingFolder: null,
    visibility: 'everyone',
    memberIds: [],
  };
}

/** What of a folder a library of this type and claims will not show (section 4). */
function hiddenCounts(folder: UnassignedFolder, draft: Draft): {photos: number; other: number} {
  const photosShown =
    draft.contentType === 'photos' ||
    (draft.structure === 'one_offs' && !(draft.claims.includes('movie') && !draft.claims.includes('home_video')));
  return {photos: photosShown ? 0 : folder.photo_count, other: folder.other_count};
}

/** "124 videos", "124 videos and 12 photos": what the library will show of a folder. */
function shownWords(folder: UnassignedFolder, draft: Draft): string | null {
  const hidden = hiddenCounts(folder, draft);
  const parts = [
    folder.video_count ? formatCount(folder.video_count, 'video') : null,
    folder.photo_count - hidden.photos ? formatCount(folder.photo_count - hidden.photos, 'photo') : null,
  ].filter((entry): entry is string => Boolean(entry));
  return parts.length ? joinWords(parts) : null;
}

function LibraryEditor() {
  const {id} = useParams();
  const isNew = !id;
  const [params] = useSearchParams();
  const fromFolder = params.get('folder');
  const navigate = useNavigate();
  const showToast = useToast();
  const {libraries, reload, claimedBy} = useLibraries();
  const {ask, dialog} = useConfirm();

  const [library, setLibrary] = useState<Library | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [icons, setIcons] = useState<LibraryIcon[]>(KNOWN_LIBRARY_ICONS);
  const [folders, setFolders] = useState<UnassignedFolder[] | null>(isNew ? null : []);
  const [people, setPeople] = useState<User[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [errors, setErrors] = useState<Partial<Record<FieldKey, string>>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [nonce, setNonce] = useState(0);
  const formRef = useRef<HTMLFormElement | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    librariesApi.icons(controller.signal).then(result => setIcons(result.icons), () => undefined);
    usersApi.list(controller.signal).then(setPeople, () => undefined);
    return () => controller.abort();
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    if (isNew) {
      librariesApi.folders(controller.signal).then(
        result => {
          setFolders(result.folders);
          setDraft(current => current ?? newDraft(result.folders.find(folder => folder.name === fromFolder) ?? null, fromFolder));
        },
        (error: unknown) => {
          if (controller.signal.aborted) return;
          // The list of folders is only needed to take one; a new folder still works.
          setFolders([]);
          setDraft(current => current ?? newDraft(null, fromFolder));
          if (fromFolder) setLoadError(error);
        },
      );
    } else {
      librariesApi.get(id, controller.signal).then(
        loaded => {
          setLibrary(loaded);
          setDraft(draftFrom(loaded));
        },
        (error: unknown) => {
          if (!controller.signal.aborted) setLoadError(error);
        },
      );
    }
    return () => controller.abort();
  }, [id, isNew, fromFolder, nonce]);

  const title = isNew ? 'New library' : (library?.name ?? 'Library');
  useDocumentTitle(isNew ? 'New library' : library ? `${library.name} settings` : 'Library settings');

  const members = useMemo(() => people.filter(person => person.role === 'member'), [people]);
  const chosenFolder = draft?.folderMode === 'existing' && isNew ? (folders ?? []).find(folder => folder.name === draft.existingFolder) : undefined;

  const back = (
    <div {...stylex.props(styles.back)}>
      <Button
        label="Libraries"
        variant="ghost"
        href="/settings/libraries"
        icon={<ChevronLeft size={18} strokeWidth={1.75} aria-hidden="true" />}
      />
    </div>
  );

  if (!draft) {
    return (
      <>
        {back}
        <Heading level={2} xstyle={styles.title}>
          {title}
        </Heading>
        {loadError ? (
          <LoadError
            title={
              loadError instanceof ApiError && loadError.code === 'not_found'
                ? 'This library is not here any more'
                : 'This library could not be loaded'
            }
            error={loadError}
            onRetry={() => {
              setLoadError(null);
              setNonce(n => n + 1);
            }}
          />
        ) : (
          <div {...stylex.props(styles.skeleton)} role="status" aria-busy="true" aria-label="Loading">
            <Skeleton width={400} height={44} radius={2} />
            <Skeleton width={380} height={140} radius={2} />
          </div>
        )}
      </>
    );
  }

  const isLocked = !isNew && library?.is_empty === false;
  const fitting = fittingClaims(draft.contentType, draft.structure);
  const update = (change: Partial<Draft>) => {
    setDraft(current => (current ? {...current, ...change} : current));
    setErrors({});
  };

  const setName = (value: string) => {
    const name = value.slice(0, NAME_MAX);
    update({name, ...(draft.newFolderTouched ? {} : {newFolder: name.trim()})});
  };

  const setType = (contentType: ContentType, structure: Structure | null) => {
    // Kinds that no longer fit are unchecked; an icon the owner has not picked follows the type.
    update({
      contentType,
      structure,
      claims: draft.claims.filter(kind => claimFits(kind, contentType, structure)),
      ...(draft.iconTouched ? {} : {icon: defaultIconFor(contentType)}),
    });
  };

  /** Where each kind goes now, beside its checkbox. */
  const claimDescription = (kind: ClaimKind): string => {
    const holder = claimedBy(kind);
    const isHere = !isNew && holder?.id === library?.id;
    const checked = draft.claims.includes(kind);
    if (checked) {
      if (holder && !isHere) return `Moves here from ${holder.name}`;
      return isHere ? 'Sorted here now' : 'Sorted here from now on';
    }
    if (isHere) return 'Sorted here now. Unchecked, new ones wait in Inbox';
    return holder ? `Sorted into ${holder.name} now` : 'Not sorted anywhere: these wait in Inbox';
  };

  const nameForSentences = draft.name.trim() || 'this library';
  const folderShown = isNew ? (draft.folderMode === 'new' ? draft.newFolder.trim() : draft.existingFolder) : library?.folder;

  const validate = (): boolean => {
    const next: Partial<Record<FieldKey, string>> = {};
    if (!draft.name.trim()) next.name = 'Give the library a name.';
    if (isNew && draft.folderMode === 'new' && !draft.newFolder.trim()) next.folder = 'Name the new folder.';
    if (isNew && draft.folderMode === 'existing' && !draft.existingFolder) next.folder = 'Choose a folder.';
    setErrors(next);
    return Object.keys(next).length === 0;
  };

  const save = async () => {
    setIsSaving(true);
    setFormError(null);
    try {
      if (isNew) {
        const body: LibraryCreateRequest = {
          name: draft.name.trim(),
          icon: draft.icon,
          content_type: draft.contentType,
          structure: draft.contentType === 'video' ? draft.structure : null,
          claims: draft.claims,
          folder:
            draft.folderMode === 'new'
              ? {mode: 'new', name: draft.newFolder.trim()}
              : {mode: 'existing', name: draft.existingFolder ?? ''},
          visibility: draft.visibility,
          member_ids: draft.visibility === 'selected' ? draft.memberIds : [],
        };
        const created = await librariesApi.create(body);
        await reload();
        showToast({body: `${created.name} is ready.`});
      } else if (library) {
        const body: LibraryEditRequest = {};
        if (draft.name.trim() !== library.name) body.name = draft.name.trim();
        if (draft.icon !== library.icon) body.icon = draft.icon;
        if (draft.contentType !== library.content_type || draft.structure !== library.structure) {
          body.content_type = draft.contentType;
          body.structure = draft.contentType === 'video' ? draft.structure : null;
        }
        const sameClaims =
          draft.claims.length === library.claims.length && draft.claims.every(kind => library.claims.includes(kind));
        if (!sameClaims || body.content_type) body.claims = draft.claims;
        if (draft.visibility !== library.visibility) body.visibility = draft.visibility;
        const sameMembers =
          draft.memberIds.length === (library.member_ids ?? []).length &&
          draft.memberIds.every(member => (library.member_ids ?? []).includes(member));
        if (!sameMembers) body.member_ids = draft.memberIds;
        if (Object.keys(body).length > 0) {
          await librariesApi.update(library.id, body);
          await reload();
          showToast({body: 'Changes saved.'});
        }
      }
      navigate('/settings/libraries');
    } catch (cause) {
      const field = cause instanceof ApiError ? fieldOf(cause.field) : null;
      const message = errorMessage(cause, 'The library was not saved. Try again.');
      if (field) {
        setErrors({[field]: message});
        formRef.current?.querySelector<HTMLElement>(`[data-field="${field}"]`)?.scrollIntoView({block: 'center'});
      } else {
        setFormError(message);
      }
    } finally {
      setIsSaving(false);
    }
  };

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isSaving || !validate()) return;
    // Taking a kind from another library changes where new files go: confirm it first.
    const moving = draft.claims
      .map(kind => ({kind, holder: claimedBy(kind)}))
      .filter(({holder}) => holder && holder.id !== library?.id);
    if (moving.length === 0) {
      void save();
      return;
    }
    const kinds = joinWords(moving.map(({kind}) => CLAIM_WORDS[kind]));
    const from = joinWords([...new Set(moving.map(({holder}) => holder?.name ?? ''))]);
    ask({
      title: `Sort ${kinds} into ${nameForSentences}?`,
      description: `New ${kinds} will go to ${nameForSentences} instead of ${from}. Files already sorted stay where they are.`,
      actionLabel: isNew ? 'Move and make library' : 'Move and save',
      actionVariant: 'primary',
      failure: 'The library was not saved. Try again.',
      run: save,
    });
  };


  const shown = chosenFolder ? shownWords(chosenFolder, draft) : null;
  const hidden = chosenFolder ? hiddenCounts(chosenFolder, draft) : null;
  const hiddenSentence =
    hidden && (hidden.photos || hidden.other)
      ? `${joinWords(
          [
            hidden.photos ? formatCount(hidden.photos, 'photo') : null,
            hidden.other ? formatCount(hidden.other, 'other file') : null,
          ].filter((entry): entry is string => Boolean(entry)),
        )} there will not show in this library.`
      : null;
  const chosenNames = members.filter(member => draft.memberIds.includes(member.id)).map(member => member.display_name);
  const whoPreview =
    shown && isNew && draft.folderMode === 'existing'
      ? draft.visibility === 'everyone'
        ? `Everyone will see its ${shown}.`
        : chosenNames.length === 0
          ? `Only owners will see its ${shown}.`
          : `${joinWords(chosenNames)} will see its ${shown}.`
      : null;
  const status = (key: FieldKey) => (errors[key] ? {type: 'error' as const, message: errors[key]} : undefined);
  const otherLibraries = (libraries ?? []).filter(entry => entry.id !== library?.id);

  return (
    <>
      {back}
      <Heading level={2} xstyle={styles.title}>
        {title}
      </Heading>
      {formError ? (
        <Banner status="error" title="The library was not saved" description={formError} collapsible={false} xstyle={styles.banner} />
      ) : null}
      {loadError && fromFolder ? (
        <Banner
          status="warning"
          title="The folders could not be listed"
          description={errorMessage(loadError, 'Try again, or make a new folder instead.')}
          collapsible={false}
          xstyle={styles.banner}
        />
      ) : null}
      <form ref={formRef} onSubmit={onSubmit} noValidate aria-label={title}>
        <div {...stylex.props(styles.groups)}>
          <SettingsGroup title="Name and icon" description="How the library appears in the sidebar, on Home and on its page.">
            <div {...stylex.props(styles.field)} data-field="name">
              <TextInput
                label="Name"
                value={draft.name}
                onChange={setName}
                description={`Up to ${NAME_MAX} characters.`}
                isRequired
                isDisabled={isSaving}
                status={status('name')}
                statusVariant="detached"
                width="100%"
              />
              <div data-field="icon">
                <IconPicker
                  icons={icons}
                  value={draft.icon}
                  onChange={icon => update({icon, iconTouched: true})}
                  isDisabled={isSaving}
                  error={errors.icon}
                />
              </div>
              <div {...stylex.props(styles.previewBlock)}>
                <Text type="supporting" color="secondary">
                  In the sidebar
                </Text>
                <div {...stylex.props(styles.sidebarRow)} aria-label={`Sidebar preview: ${draft.name.trim() || 'New library'}`} role="img">
                  <LibraryGlyph name={draft.icon} size={18} {...stylex.props(styles.sidebarIcon)} />
                  <span {...stylex.props(styles.sidebarName)}>{draft.name.trim() || 'New library'}</span>
                </div>
              </div>
            </div>
          </SettingsGroup>

          <SettingsGroup
            title="What it holds"
            description={
              isLocked
                ? 'The type can change only while the library is empty.'
                : 'Videos are shows with seasons and episodes, or films and clips that stand on their own.'
            }>
            {isLocked ? (
              // Its files fix what it holds: a fact in the text colour, like the folder, not greyed choices.
              <div {...stylex.props(styles.fact)} data-field="type" data-testid="holds-fact">
                <Text type="label">Holds</Text>
                <Text type="body">{holdsLine(draft.contentType, draft.structure)}</Text>
              </div>
            ) : (
              <div {...stylex.props(styles.field)} data-field="type">
                <RadioList
                  label="Holds"
                  value={draft.contentType}
                  onChange={value =>
                    setType(value as ContentType, value === 'video' ? (draft.structure ?? 'one_offs') : null)
                  }
                  isDisabled={isSaving}
                  status={status('type')}>
                  <RadioListItem value="video" label="Videos" />
                  <RadioListItem value="photos" label="Photos" description="Photos and the short clips phones take with them" />
                </RadioList>
                {draft.contentType === 'video' ? (
                  <RadioList
                    label="Arranged as"
                    value={draft.structure ?? 'one_offs'}
                    onChange={value => setType('video', value as Structure)}
                    isDisabled={isSaving}>
                    <RadioListItem value="episodes" label="Series" description="Shows with seasons and episodes" />
                    <RadioListItem value="one_offs" label="One-offs" description="Films and clips that stand on their own" />
                  </RadioList>
                ) : null}
              </div>
            )}
          </SettingsGroup>

          <SettingsGroup
            title="New files"
            description="Each kind of new file goes to one library at most. A kind no library takes waits in Inbox.">
            <div {...stylex.props(styles.field)} data-field="claims">
              <CheckboxList
                label="Sort these here"
                value={draft.claims}
                onChange={values => update({claims: values.filter((kind): kind is ClaimKind => fitting.includes(kind as ClaimKind))})}
                isDisabled={isSaving}
                status={status('claims')}>
                {fitting.map(kind => (
                  <CheckboxListItem
                    key={kind}
                    value={kind}
                    label={CLAIM_LABELS[kind]}
                    description={claimDescription(kind)}
                    xstyle={styles.checkboxRow}
                  />
                ))}
              </CheckboxList>
            </div>
          </SettingsGroup>

          <SettingsGroup
            title="Folder"
            description={
              isNew
                ? 'Every library is one folder in your PopcornStash folder. Files already in a folder are never moved.'
                : 'A library keeps its folder for good.'
            }>
            <div {...stylex.props(styles.field)} data-field="folder">
              {isNew ? (
                <>
                  <RadioList
                    label="Where its files live"
                    value={draft.folderMode}
                    onChange={value => update({folderMode: value as 'new' | 'existing'})}
                    isDisabled={isSaving}>
                    <RadioListItem value="new" label="Make a new folder" />
                    <RadioListItem
                      value="existing"
                      label="Use a folder that is already there"
                      isDisabled={(folders ?? []).length === 0}
                      description={(folders ?? []).length === 0 ? 'Every top-level folder is already a library.' : undefined}
                    />
                  </RadioList>
                  {draft.folderMode === 'new' ? (
                    <TextInput
                      label="Folder name"
                      value={draft.newFolder}
                      onChange={value => update({newFolder: value, newFolderTouched: true})}
                      isDisabled={isSaving}
                      status={status('folder')}
                      statusVariant="detached"
                      width="100%"
                      xstyle={styles.code}
                    />
                  ) : (
                    <Selector
                      label="Folder"
                      options={(folders ?? []).map(folder => ({
                        value: folder.name,
                        label: `${folder.name}/`,
                        description: folderCounts(folder).join(', '),
                      }))}
                      value={draft.existingFolder ?? ''}
                      placeholder="Choose a folder"
                      onChange={value => {
                        const folder = (folders ?? []).find(entry => entry.name === value);
                        update({
                          existingFolder: value || null,
                          ...(folder && !draft.name.trim() ? {name: folder.name.slice(0, NAME_MAX)} : {}),
                          // Its files were seen by owners only: nobody else until the owner chooses.
                          visibility: draft.visibility === 'everyone' && isNew ? 'selected' : draft.visibility,
                        });
                      }}
                      isDisabled={isSaving}
                      status={status('folder')}
                      statusVariant="detached"
                      width="100%"
                    />
                  )}
                  {draft.folderMode === 'existing' && draft.existingFolder ? (
                    <Text as="p" type="supporting" color="secondary" xstyle={styles.preview}>
                      {[`Files stay in ${draft.existingFolder}/. Nothing is moved.`, hiddenSentence].filter(Boolean).join(' ')}
                    </Text>
                  ) : null}
                </>
              ) : (
                <>
                  <div {...stylex.props(styles.fact)}>
                    <Text type="label">Folder</Text>
                    <span {...stylex.props(styles.factValue)}>{`${folderShown ?? ''}/`}</span>
                    {library?.folder_state === 'missing' ? <StatusTag tone="warning">Folder missing</StatusTag> : null}
                    {library?.folder_state === 'read_only' ? <StatusTag tone="warning">Folder read-only</StatusTag> : null}
                  </div>
                  <Text as="p" type="supporting" color="secondary" xstyle={styles.preview}>
                    {library?.folder_state === 'missing'
                      ? 'The folder is not there right now (a disk or share that did not mount?). New files of its kinds wait in Inbox until it is back.'
                      : library?.folder_state === 'read_only'
                        ? 'PopcornStash cannot write to the folder, so new files of its kinds wait in Inbox.'
                        : 'Renaming the library keeps this folder.'}
                  </Text>
                </>
              )}
            </div>
          </SettingsGroup>

          <SettingsGroup title="Who sees it" description="Owners see every library.">
            <div {...stylex.props(styles.field)} data-field="visibility">
              <RadioList
                label="Who sees it"
                isLabelHidden
                value={draft.visibility}
                onChange={value => update({visibility: value as LibraryVisibility})}
                isDisabled={isSaving}
                status={status('visibility')}>
                <RadioListItem value="everyone" label="Everyone" />
                <RadioListItem value="selected" label="Only some people" />
              </RadioList>
              {draft.visibility === 'selected' ? (
                members.length === 0 ? (
                  <Text as="p" type="supporting" color="secondary" xstyle={styles.preview}>
                    Nobody else has joined yet, so only owners will see it. Invite people in Settings, People.
                  </Text>
                ) : (
                  <CheckboxList
                    label="People"
                    value={draft.memberIds}
                    onChange={values => update({memberIds: values})}
                    isDisabled={isSaving}>
                    {members.map(member => (
                      <CheckboxListItem
                        key={member.id}
                        value={member.id}
                        label={member.display_name}
                        xstyle={styles.checkboxRow}
                      />
                    ))}
                  </CheckboxList>
                )
              ) : null}
              {whoPreview ? (
                <Text as="p" type="supporting" color="secondary" xstyle={styles.preview}>
                  {whoPreview}
                </Text>
              ) : null}
            </div>
          </SettingsGroup>

          {!isNew && library?.content_type === 'video' ? (
            <SettingsGroup title="Video conversion" description="Choose a space-saving format for this library. Originals are replaced only after verification.">
              <Button label="Manage video conversion" href={`/settings/transcoding/${library.id}`} />
            </SettingsGroup>
          ) : null}

          {!isNew && library ? (
            <SettingsGroup
              title="Delete library"
              description={`Takes ${library.name} out of the app. Its files stay in ${library.folder ?? 'its folder'}/ and only owners will see them.`}
              actions={<Button label="Delete library" variant="destructive" onClick={() => setDeleting(true)} isDisabled={isSaving} />}
            />
          ) : null}
        </div>

        <div {...stylex.props(styles.footer)}>
          <Button type="submit" variant="primary" label={isNew ? 'Make library' : 'Save changes'} isLoading={isSaving} />
          <Button variant="ghost" label="Cancel" href="/settings/libraries" isDisabled={isSaving} />
          {otherLibraries.length >= 23 && isNew ? (
            <Text type="supporting" color="secondary">
              A stash can have 24 libraries.
            </Text>
          ) : null}
        </div>
      </form>
      {dialog}
      {deleting && library ? <DeleteLibraryDialog library={library} onClose={() => setDeleting(false)} onDeleted={removed => {
        setDeleting(false); void reload();
        showToast({body: removed ? `${library.name} and its folder moved to Trash.` : `${library.name} is no longer in the app. Its files stay where they are.`});
        navigate('/settings/libraries');
      }} /> : null}
    </>
  );
}

export default function LibraryEditorPage() {
  return (
    <OwnerOnly>
      <LibraryEditor />
    </OwnerOnly>
  );
}

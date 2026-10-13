/**
 * Edit how an item appears (`PATCH /api/items/:id`). Metadata only: the file
 * is never moved or renamed here. The fields follow the category: films
 * have a title and year, episodes a show, season, episode and episode title,
 * everything else a title.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useId, useState, type FormEvent} from 'react';
import {items as itemsApi} from '../../api/client';
import type {ItemDetail, ItemPatch} from '../../api/types';
import {errorMessage} from '../../lib/errors';

const styles = stylex.create({
  form: {
    display: 'contents',
  },
  pair: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 12,
  },
});

const MIN_YEAR = 1888;
const MAX_YEAR = new Date().getFullYear() + 2;

interface Draft {
  title: string;
  year: number | null;
  show_name: string;
  season: number | null;
  episode: number | null;
  episode_title: string;
}

type Errors = Partial<Record<keyof Draft, string>>;

function draftOf(item: ItemDetail): Draft {
  return {
    title: item.title,
    year: item.year,
    show_name: item.show_name ?? '',
    season: item.season,
    episode: item.episode,
    episode_title: item.episode_title ?? '',
  };
}

function validate(item: ItemDetail, draft: Draft): Errors {
  const errors: Errors = {};
  if (item.category === 'episode') {
    if (!draft.show_name.trim()) errors.show_name = 'Enter the name of the show.';
    if (draft.season === null || draft.season < 0) errors.season = 'Enter a season number.';
    if (draft.episode === null || draft.episode < 1) errors.episode = 'Enter an episode number.';
  } else if (!draft.title.trim()) {
    errors.title = 'Enter a title.';
  }
  if (item.category === 'movie' && draft.year !== null && (draft.year < MIN_YEAR || draft.year > MAX_YEAR)) {
    errors.year = `Enter a year between ${MIN_YEAR} and ${MAX_YEAR}.`;
  }
  return errors;
}

function patchOf(item: ItemDetail, draft: Draft): ItemPatch {
  const patch: ItemPatch = {};
  if (item.category === 'episode') {
    if (draft.show_name.trim() !== (item.show_name ?? '')) patch.show_name = draft.show_name.trim();
    if (draft.season !== item.season) patch.season = draft.season;
    if (draft.episode !== item.episode) patch.episode = draft.episode;
    const episodeTitle = draft.episode_title.trim() || null;
    if (episodeTitle !== item.episode_title) patch.episode_title = episodeTitle;
    return patch;
  }
  if (draft.title.trim() !== item.title) patch.title = draft.title.trim();
  if (item.category === 'movie' && draft.year !== item.year) patch.year = draft.year;
  return patch;
}

export interface EditDetailsDialogProps {
  item: ItemDetail;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: (item: ItemDetail) => void;
}

export function EditDetailsDialog({item, isOpen, onOpenChange, onSaved}: EditDetailsDialogProps) {
  const formId = useId();
  const [draft, setDraft] = useState<Draft>(() => draftOf(item));
  const [errors, setErrors] = useState<Errors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => {
    setDraft(current => ({...current, [key]: value}));
    if (errors[key]) setErrors(current => ({...current, [key]: undefined}));
  };

  const status = (key: keyof Draft) => (errors[key] ? {type: 'error' as const, message: errors[key]} : undefined);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isSaving) return;
    const found = validate(item, draft);
    setErrors(found);
    setFormError(null);
    if (Object.values(found).some(Boolean)) return;
    const patch = patchOf(item, draft);
    if (Object.keys(patch).length === 0) {
      onOpenChange(false);
      return;
    }
    setIsSaving(true);
    try {
      const saved = await itemsApi.update(item.id, patch);
      onSaved(saved);
      onOpenChange(false);
    } catch (error) {
      setFormError(errorMessage(error, 'The changes were not saved. Try again.'));
    } finally {
      setIsSaving(false);
    }
  };

  const fields =
    item.category === 'episode' ? (
      <>
        <TextInput
          label="Show"
          value={draft.show_name}
          onChange={value => set('show_name', value)}
          status={status('show_name')}
          statusVariant="detached"
          isRequired
          width="100%"
        />
        <div {...stylex.props(styles.pair)}>
          <NumberInput
            label="Season"
            value={draft.season}
            onChange={value => set('season', value)}
            min={0}
            isIntegerOnly
            isWheelEnabled={false}
            status={status('season')}
            statusVariant="detached"
            isRequired
            width="100%"
          />
          <NumberInput
            label="Episode"
            value={draft.episode}
            onChange={value => set('episode', value)}
            min={1}
            isIntegerOnly
            isWheelEnabled={false}
            status={status('episode')}
            statusVariant="detached"
            isRequired
            width="100%"
          />
        </div>
        <TextInput
          label="Episode title"
          value={draft.episode_title}
          onChange={value => set('episode_title', value)}
          isOptional
          width="100%"
        />
      </>
    ) : (
      <>
        <TextInput
          label="Title"
          value={draft.title}
          onChange={value => set('title', value)}
          status={status('title')}
          statusVariant="detached"
          isRequired
          width="100%"
        />
        {item.category === 'movie' ? (
          <NumberInput
            label="Year"
            description="The year the film came out."
            value={draft.year}
            onChange={value => set('year', value)}
            min={MIN_YEAR}
            max={MAX_YEAR}
            isIntegerOnly
            isWheelEnabled={false}
            hasClear
            formatValue={value => String(value)}
            status={status('year')}
            statusVariant="detached"
            isOptional
            width="100%"
          />
        ) : null}
      </>
    );

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} purpose="form" width={480}>
      <form id={formId} onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <Layout
          header={
            <DialogHeader
              title="Edit details"
              subtitle="Changes how this appears in PopcornStash. The file itself is not moved or renamed."
              onOpenChange={onOpenChange}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={4}>
                {fields}
                {formError ? (
                  <Banner status="error" title="Not saved" description={formError} collapsible={false} />
                ) : null}
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end" wrap="wrap">
                <Button label="Cancel" variant="secondary" onClick={() => onOpenChange(false)} />
                <Button label="Save" variant="primary" type="submit" form={formId} isLoading={isSaving} />
              </HStack>
            </LayoutFooter>
          }
        />
      </form>
    </Dialog>
  );
}

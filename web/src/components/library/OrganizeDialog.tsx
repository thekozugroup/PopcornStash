/**
 * Move a file that sits outside the library folders into the right place
 * (`POST /api/items/:id/organize`). This is the only way PopcornStash moves
 * a file it did not bring in itself, so the dialog says exactly where the
 * file will go before anything moves.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useId, useState, type FormEvent} from 'react';
import {items as itemsApi} from '../../api/client';
import type {ItemCategory, ItemDetail, OrganizeRequest} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {
  CATEGORY_FOLDER,
  CATEGORY_SINGULAR,
  destinationFor,
  organizeCategories,
  type OrganizeFields,
} from './libraryFormat';

const styles = stylex.create({
  form: {
    display: 'contents',
  },
  pair: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 12,
  },
  destination: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    paddingBlock: 14,
    paddingInline: 16,
    borderRadius: 12,
    backgroundColor: colorVars['--color-background-muted'],
  },
  label: {
    fontSize: 13,
    fontWeight: 600,
    color: colorVars['--color-text-secondary'],
  },
  path: {
    fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    fontSize: 13,
    lineHeight: 1.55,
    overflowWrap: 'anywhere',
    color: colorVars['--color-text-primary'],
  },
  from: {
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
  },
  note: {
    lineHeight: 1.55,
  },
});

const MIN_YEAR = 1888;
const MAX_YEAR = new Date().getFullYear() + 2;

function initialFields(item: ItemDetail): OrganizeFields {
  const allowed = organizeCategories(item.kind);
  const suggestion = item.suggestion;
  const category: ItemCategory =
    suggestion && allowed.includes(suggestion.category)
      ? suggestion.category
      : allowed.includes(item.category)
        ? item.category
        : (allowed[0] ?? 'other');
  return {
    category,
    title: suggestion?.title ?? item.title,
    year: suggestion?.year ?? item.year,
    show_name: suggestion?.show_name ?? item.show_name ?? '',
    season: suggestion?.season ?? item.season,
    episode: suggestion?.episode ?? item.episode,
  };
}

const DATE_SORTED_NOTE: Partial<Record<ItemCategory, string>> = {
  home_video: 'Home videos are filed by the month they were recorded.',
  photo: 'Photos are filed by the month they were taken.',
  other: 'The file keeps its own name.',
};

export interface OrganizeDialogProps {
  item: ItemDetail;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onOrganized: (item: ItemDetail) => void;
}

export function OrganizeDialog({item, isOpen, onOpenChange, onOrganized}: OrganizeDialogProps) {
  const formId = useId();
  const [fields, setFields] = useState<OrganizeFields>(() => initialFields(item));
  const [formError, setFormError] = useState<string | null>(null);
  const [isMoving, setIsMoving] = useState(false);
  const [showErrors, setShowErrors] = useState(false);

  const allowed = organizeCategories(item.kind);
  const destination = destinationFor(fields, item);
  const set = <K extends keyof OrganizeFields>(key: K, value: OrganizeFields[K]) =>
    setFields(current => ({...current, [key]: value}));

  const missing = {
    title: fields.category === 'movie' && !fields.title.trim(),
    show_name: fields.category === 'episode' && !fields.show_name.trim(),
    season: fields.category === 'episode' && fields.season === null,
    episode: fields.category === 'episode' && fields.episode === null,
  };
  const status = (key: keyof typeof missing, message: string) =>
    showErrors && missing[key] ? {type: 'error' as const, message} : undefined;

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isMoving) return;
    setShowErrors(true);
    if (!destination) return;
    const body: OrganizeRequest = {category: fields.category};
    if (fields.category === 'movie') {
      body.title = fields.title.trim();
      body.year = fields.year;
    } else if (fields.category === 'episode') {
      body.show_name = fields.show_name.trim();
      body.season = fields.season;
      body.episode = fields.episode;
    }
    setIsMoving(true);
    setFormError(null);
    try {
      const moved = await itemsApi.organize(item.id, body);
      onOrganized(moved);
      onOpenChange(false);
    } catch (error) {
      setFormError(errorMessage(error, 'The file was not moved. Try again.'));
    } finally {
      setIsMoving(false);
    }
  };

  const folder = CATEGORY_FOLDER[fields.category];

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} purpose="form" width={540}>
      <form id={formId} onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <Layout
          header={
            <DialogHeader
              title="Organize this file"
              subtitle="Moves the file into your library so it is sorted like everything else."
              onOpenChange={onOpenChange}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={4}>
                <Text as="p" type="supporting" color="secondary" xstyle={styles.from}>
                  {`Now in ${item.rel_path.split('/').slice(0, -1).join('/') || 'the top of your PopcornStash folder'}`}
                </Text>
                {allowed.length > 1 ? (
                  <Selector
                    label="What is it?"
                    options={allowed.map(category => ({value: category, label: CATEGORY_SINGULAR[category]}))}
                    value={fields.category}
                    onChange={value => {
                      const next = allowed.find(category => category === value);
                      if (next) set('category', next);
                    }}
                    width="100%"
                  />
                ) : null}
                {fields.category === 'movie' ? (
                  <div {...stylex.props(styles.pair)}>
                    <TextInput
                      label="Title"
                      value={fields.title}
                      onChange={value => set('title', value)}
                      status={status('title', 'Enter the film’s title.')}
                      statusVariant="detached"
                      isRequired
                      width="100%"
                    />
                    <NumberInput
                      label="Year"
                      value={fields.year}
                      onChange={value => set('year', value)}
                      min={MIN_YEAR}
                      max={MAX_YEAR}
                      isIntegerOnly
                      isWheelEnabled={false}
                      hasClear
                      formatValue={value => String(value)}
                      isOptional
                      width="100%"
                    />
                  </div>
                ) : null}
                {fields.category === 'episode' ? (
                  <>
                    <TextInput
                      label="Show"
                      value={fields.show_name}
                      onChange={value => set('show_name', value)}
                      status={status('show_name', 'Enter the name of the show.')}
                      statusVariant="detached"
                      isRequired
                      width="100%"
                    />
                    <div {...stylex.props(styles.pair)}>
                      <NumberInput
                        label="Season"
                        value={fields.season}
                        onChange={value => set('season', value)}
                        min={0}
                        isIntegerOnly
                        isWheelEnabled={false}
                        status={status('season', 'Enter a season number.')}
                        statusVariant="detached"
                        isRequired
                        width="100%"
                      />
                      <NumberInput
                        label="Episode"
                        value={fields.episode}
                        onChange={value => set('episode', value)}
                        min={1}
                        isIntegerOnly
                        isWheelEnabled={false}
                        status={status('episode', 'Enter an episode number.')}
                        statusVariant="detached"
                        isRequired
                        width="100%"
                      />
                    </div>
                  </>
                ) : null}

                <div {...stylex.props(styles.destination)} aria-live="polite">
                  <span {...stylex.props(styles.label)}>Moves to</span>
                  <span {...stylex.props(styles.path)}>{destination ?? `${folder}/…`}</span>
                  <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
                    {[
                      DATE_SORTED_NOTE[fields.category],
                      'Inside your PopcornStash folder. Subtitles with the same name move with it, and if a file with this name is already there, a number is added to the end.',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                  </Text>
                </div>

                {formError ? (
                  <Banner status="error" title="The file was not moved" description={formError} collapsible={false} />
                ) : null}
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end" wrap="wrap">
                <Button label="Cancel" variant="secondary" onClick={() => onOpenChange(false)} />
                <Button label={`Move to ${folder}`} variant="primary" type="submit" form={formId} isLoading={isMoving} />
              </HStack>
            </LayoutFooter>
          }
        />
      </form>
    </Dialog>
  );
}

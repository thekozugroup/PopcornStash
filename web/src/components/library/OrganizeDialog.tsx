/**
 * Move a file into a library or Other files on purpose
 * (`POST /api/items/:id/organize`, owners only). This is the only way
 * PopcornStash moves a file it did not bring in itself, so the dialog names
 * the library, asks for the details that library needs, and shows where the
 * file lands before anything moves.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {useId, useState, type FormEvent} from 'react';
import {ApiError, items as itemsApi} from '../../api/client';
import type {ItemDetail} from '../../api/types';
import {useLibraries} from '../../app/LibrariesProvider';
import {errorMessage} from '../../lib/errors';
import {
  MoveFields,
  fieldOfServerError,
  initialMoveForm,
  validateMove,
  type MoveFieldErrors,
  type MoveFormState,
} from '../inbox/MoveFields';
import {
  OTHER_FILES_TARGET,
  folderOf,
  libraryTarget,
  resolveTarget,
  suggestedTarget,
  targetChoices,
  targetName,
  targetValue,
  type TargetValue,
} from '../inbox/places';

const styles = stylex.create({
  form: {
    display: 'contents',
  },
  from: {
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
  },
});

export interface OrganizeDialogProps {
  item: ItemDetail;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onOrganized: (item: ItemDetail, destination: string) => void;
}

export function OrganizeDialog({item, isOpen, onOpenChange, onOrganized}: OrganizeDialogProps) {
  const formId = useId();
  const {libraries: listed} = useLibraries();
  const libraries = listed ?? [];
  const choices = targetChoices(item, libraries, true);
  const values = choices.map(targetValue);
  const first: TargetValue | null = (() => {
    const suggested = suggestedTarget(item, libraries);
    if (suggested && values.includes(suggested)) return suggested;
    if (item.library_id && values.includes(libraryTarget(item.library_id))) return libraryTarget(item.library_id);
    if (item.place === 'other_files') return OTHER_FILES_TARGET;
    return null;
  })();
  const [form, setForm] = useState<MoveFormState>(() => initialMoveForm(item, first));
  const [errors, setErrors] = useState<MoveFieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [isMoving, setIsMoving] = useState(false);
  const target = resolveTarget(form.target, libraries);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isMoving) return;
    const checked = validateMove(form, libraries);
    setErrors(checked.errors);
    if (!checked.body || !checked.target) return;
    setIsMoving(true);
    setFormError(null);
    try {
      const moved = await itemsApi.organize(item.id, checked.body);
      onOrganized(moved, targetName(checked.target));
      onOpenChange(false);
    } catch (error) {
      const field = error instanceof ApiError ? fieldOfServerError(error.field) : null;
      const message = errorMessage(error, 'The file was not moved. Try again.');
      if (field) setErrors({[field]: message});
      else setFormError(message);
    } finally {
      setIsMoving(false);
    }
  };

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} purpose="form" width={540}>
      <form id={formId} onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <Layout
          header={
            <DialogHeader
              title="Organize this file"
              subtitle="Moves the file into a library, laid out like everything else there."
              onOpenChange={onOpenChange}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={4}>
                <Text as="p" type="supporting" color="secondary" xstyle={styles.from}>
                  {`Now in ${folderOf(item.rel_path) || 'the top of your PopcornStash folder'}`}
                </Text>
                <MoveFields
                  item={item}
                  form={form}
                  onChange={next => {
                    setForm(next);
                    setErrors({});
                  }}
                  choices={choices}
                  libraries={libraries}
                  errors={errors}
                  showFolder
                  isDisabled={isMoving}
                />
                <Text as="p" type="supporting" color="secondary">
                  Subtitles with the same name move with it. If a file with this name is already there, a number is added to
                  the end.
                </Text>
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
                <Button
                  label={target ? `Move to ${targetName(target)}` : 'Move'}
                  variant="primary"
                  type="submit"
                  form={formId}
                  isLoading={isMoving}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </form>
    </Dialog>
  );
}

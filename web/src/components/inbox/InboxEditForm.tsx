/**
 * Inline edit for one Inbox item: the same Library selector as the row, the
 * details that library needs, and a live preview of where the file goes
 * before it moves. Opening the form moves focus to its first field. A server
 * error that names a field sits under that field.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {useEffect, useRef, useState, type FormEvent} from 'react';
import type {InboxDecision, ItemDetail, Library} from '../../api/types';
import {
  MoveFields,
  initialMoveForm,
  validateMove,
  type MoveFieldErrors,
  type MoveFormState,
} from './MoveFields';
import type {Target, TargetValue} from './places';

const styles = stylex.create({
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    paddingBlock: 20,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
    borderRadius: 'var(--popcorn-radius-media)',
    // Zinc Subtle in light, Zinc Black in dark: the form's focus rings keep 3:1 on it.
    backgroundColor: 'var(--popcorn-control-well)',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
});

export interface InboxEditFormProps {
  item: ItemDetail;
  libraries: Library[];
  choices: Target[];
  /** The target the row shows when the form opens. */
  target: TargetValue | null;
  isOwner: boolean;
  isBusy: boolean;
  /** A failure without a field to put it under. */
  error: string | null;
  /** A failure that names a field (cleared by the parent when the person edits). */
  fieldErrors: MoveFieldErrors;
  onClearServerErrors: () => void;
  onCancel: () => void;
  onSubmit: (decision: InboxDecision, destination: string, target: TargetValue) => void;
}

export function InboxEditForm({
  item,
  libraries,
  choices,
  target,
  isOwner,
  isBusy,
  error,
  fieldErrors,
  onClearServerErrors,
  onCancel,
  onSubmit,
}: InboxEditFormProps) {
  const [form, setForm] = useState<MoveFormState>(() => initialMoveForm(item, target));
  const [errors, setErrors] = useState<MoveFieldErrors>({});
  const formRef = useRef<HTMLFormElement | null>(null);

  useEffect(() => {
    formRef.current?.querySelector<HTMLElement>('input:not([disabled]), [role="combobox"], button')?.focus();
  }, []);

  // The server's verdict on a field wins over the form's own check for it.
  const shownErrors = {...errors, ...fieldErrors};

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const checked = validateMove(form, libraries);
    setErrors(checked.errors);
    if (!checked.body || !checked.target || !form.target) return;
    onSubmit({action: 'accept', ...checked.body}, checked.target.library?.name ?? 'Other files', form.target);
  };

  return (
    <form
      ref={formRef}
      onSubmit={submit}
      noValidate
      aria-label={`Edit where ${item.title} goes`}
      {...stylex.props(styles.form)}>
      <MoveFields
        item={item}
        form={form}
        onChange={next => {
          setForm(next);
          setErrors({});
          onClearServerErrors();
        }}
        choices={choices}
        libraries={libraries}
        errors={shownErrors}
        showFolder={isOwner}
        isDisabled={isBusy}
      />

      {error ? <Banner status="error" title="The file did not move" description={error} collapsible={false} /> : null}

      <div {...stylex.props(styles.actions)}>
        <Button type="submit" variant="primary" label="Move it there" isLoading={isBusy} />
        <Button variant="secondary" label="Cancel" onClick={onCancel} isDisabled={isBusy} />
      </div>
    </form>
  );
}

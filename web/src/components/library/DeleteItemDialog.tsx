/**
 * Owner only: move an item to Trash (`DELETE /api/items/:id`). The file is
 * not erased at once; it waits in Trash for the retention period set in
 * Settings (30 days unless changed), then it is deleted for good.
 */
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {useEffect, useState} from 'react';
import {items as itemsApi, settings as settingsApi} from '../../api/client';
import type {ItemDetail} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatCount} from '../../lib/format';
import {displayTitle} from './libraryFormat';

export interface DeleteItemDialogProps {
  item: ItemDetail;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onDeleted: () => void;
}

export function DeleteItemDialog({item, isOpen, onOpenChange, onDeleted}: DeleteItemDialogProps) {
  const [retentionDays, setRetentionDays] = useState(30);
  const [isDeleting, setIsDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    const controller = new AbortController();
    settingsApi.get(controller.signal).then(
      value => setRetentionDays(value.trash_retention_days),
      () => undefined,
    );
    return () => controller.abort();
  }, [isOpen]);

  const title = displayTitle(item);
  const description = [
    error ? `It was not moved: ${error}` : null,
    `It stays in Trash for ${formatCount(retentionDays, 'day')}, then it is deleted for good. Until then you can restore it.`,
    'Nobody in the family can watch or open it while it is in Trash.',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <AlertDialog
      isOpen={isOpen}
      onOpenChange={open => {
        if (!open) setError(null);
        onOpenChange(open);
      }}
      title={`Move “${title}” to Trash?`}
      description={description}
      actionLabel="Move to Trash"
      // Trash can be undone, and status colours are never action colours.
      actionVariant="primary"
      isActionLoading={isDeleting}
      onAction={async () => {
        setIsDeleting(true);
        setError(null);
        try {
          await itemsApi.remove(item.id);
          onOpenChange(false);
          onDeleted();
        } catch (failure) {
          setError(errorMessage(failure, 'Try again in a moment.'));
        } finally {
          setIsDeleting(false);
        }
      }}
      width={460}
    />
  );
}

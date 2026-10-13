/**
 * One confirmation dialog per section for destructive actions (revoke,
 * remove, empty trash). `ask()` opens it; the action runs with a spinner on
 * the button, and a failure is shown as an error toast.
 */
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {useToast} from '@astryxdesign/core/Toast';
import {useCallback, useState, type ReactNode} from 'react';
import {errorMessage} from '../../lib/errors';

export interface ConfirmRequest {
  title: string;
  description: string;
  actionLabel: string;
  /** Destructive by default. */
  actionVariant?: 'destructive' | 'primary';
  /** Shown as an error toast when `run` fails. */
  failure?: string;
  run: () => Promise<void>;
}

export function useConfirm(): {ask: (request: ConfirmRequest) => void; dialog: ReactNode} {
  const showToast = useToast();
  const [request, setRequest] = useState<ConfirmRequest | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [isBusy, setIsBusy] = useState(false);

  const ask = useCallback((next: ConfirmRequest) => {
    setRequest(next);
    setIsOpen(true);
  }, []);

  const run = async () => {
    if (!request || isBusy) return;
    setIsBusy(true);
    try {
      await request.run();
      setIsOpen(false);
    } catch (cause) {
      setIsOpen(false);
      showToast({type: 'error', body: errorMessage(cause, request.failure ?? 'That did not work. Try again.')});
    } finally {
      setIsBusy(false);
    }
  };

  const dialog = request ? (
    <AlertDialog
      isOpen={isOpen}
      onOpenChange={open => {
        if (!isBusy) setIsOpen(open);
      }}
      title={request.title}
      description={request.description}
      actionLabel={request.actionLabel}
      actionVariant={request.actionVariant ?? 'destructive'}
      onAction={() => void run()}
      isActionLoading={isBusy}
    />
  ) : null;

  return {ask, dialog};
}

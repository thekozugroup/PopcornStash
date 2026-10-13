/**
 * Confirms removing a download from the list. While its files are still in
 * Downloads, the person can also delete them; sorted files already live in
 * the library and are never offered for deletion here.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {useState} from 'react';
import {downloads as downloadsApi} from '../../api/client';
import type {Download} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {filesStillInDownloads} from './downloadText';

const styles = stylex.create({
  body: {
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
  },
  name: {
    overflowWrap: 'anywhere',
  },
});

export interface RemoveDownloadDialogProps {
  download: Download | null;
  onClose: () => void;
  onRemoved: (download: Download) => void;
}

export function RemoveDownloadDialog({download, onClose, onRemoved}: RemoveDownloadDialogProps) {
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState<Download | null>(download);

  // Reset the form for each download the dialog opens for.
  if (download && download.id !== shown?.id) {
    setShown(download);
    setDeleteFiles(false);
    setError(null);
  }

  const current = download ?? shown;
  if (!current) return null;
  const offerDelete = filesStillInDownloads(current);
  const isActive = current.state === 'downloading' || current.state === 'queued' || current.state === 'seeding' || current.state === 'paused';

  const remove = async () => {
    setIsBusy(true);
    setError(null);
    try {
      await downloadsApi.remove(current.id, offerDelete && deleteFiles);
      onRemoved(current);
      onClose();
    } catch (cause) {
      setError(errorMessage(cause, 'The download could not be removed. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  const explanation = !offerDelete
    ? 'It leaves this list. The sorted files stay in your library.'
    : isActive
      ? 'It stops downloading and leaves this list.'
      : 'It leaves this list.';

  return (
    <Dialog
      isOpen={download !== null}
      onOpenChange={open => {
        if (!open && !isBusy) onClose();
      }}
      width={460}
      purpose="form">
      <Layout
        header={<DialogHeader title="Remove this download?" onOpenChange={() => !isBusy && onClose()} />}
        content={
          <LayoutContent>
            <div {...stylex.props(styles.body)}>
              <Text as="p" type="label" xstyle={styles.name}>
                {current.name}
              </Text>
              <Text as="p" color="secondary">
                {explanation}
              </Text>
              {offerDelete ? (
                <CheckboxInput
                  label="Also delete the downloaded files"
                  description="Deletes what has arrived in Downloads so far. Nothing in your library is touched."
                  value={deleteFiles}
                  onChange={setDeleteFiles}
                  isDisabled={isBusy}
                />
              ) : null}
              {error ? <Banner status="error" title="Not removed" description={error} collapsible={false} /> : null}
            </div>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end" wrap="wrap">
              <Button label="Cancel" variant="secondary" onClick={onClose} isDisabled={isBusy} />
              <Button
                label={offerDelete && deleteFiles ? 'Remove and delete files' : 'Remove'}
                variant="primary"
                isLoading={isBusy}
                onClick={() => void remove()}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

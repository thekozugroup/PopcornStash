-- Activity visibility is independent of the verified replacement ledger.
ALTER TABLE conversion_jobs ADD COLUMN activity_hidden INTEGER NOT NULL DEFAULT 0 CHECK(activity_hidden IN (0,1));
-- Suppress only redundant checks of an already verified output from this policy.
UPDATE conversion_jobs AS duplicate SET activity_hidden=1
WHERE duplicate.state='skipped' AND duplicate.stage IS NULL
  AND duplicate.message='Already in the selected format.'
  AND EXISTS (SELECT 1 FROM conversion_jobs completed
    WHERE completed.item_id=duplicate.item_id AND completed.revision=duplicate.revision
      AND completed.state='complete' AND completed.output_version=duplicate.source_version);

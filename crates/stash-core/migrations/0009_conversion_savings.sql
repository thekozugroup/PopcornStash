-- Anchor savings to the exact catalog version produced by a verified replacement.
-- Existing completed jobs can be recovered only while their output still matches.
ALTER TABLE conversion_jobs ADD COLUMN output_version TEXT;
UPDATE conversion_jobs AS j SET output_version=(
 SELECT CAST(i.size AS TEXT)||':'||CAST(i.mtime AS TEXT)||':'||COALESCE(i.quick_hash,'')
 FROM items i WHERE i.id=j.item_id AND i.rel_path=j.target_path AND i.status='ready'
 AND i.size=CAST(substr(j.source_version,1,instr(j.source_version,':')-1) AS INTEGER)-j.saved_bytes
 AND i.mtime BETWEEN j.created_at AND j.updated_at
) WHERE j.state='complete' AND NOT EXISTS (
 SELECT 1 FROM conversion_jobs newer WHERE newer.item_id=j.item_id AND newer.state='complete'
 AND (newer.updated_at>j.updated_at OR (newer.updated_at=j.updated_at AND newer.rowid>j.rowid))
);

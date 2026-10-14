-- Capture times as the camera's clock read them (docs/architecture.md section 4, Capture
-- time; section 6, Migration 0003).
--
-- Like 0002, this runs inside sqlx's transaction and only adds and fills columns.

-- 1. The camera's clock (`YYYY-MM-DDTHH:MM:SS`) and the UTC offset in minutes the file
--    wrote beside it, next to the moment that `taken_at` already holds.
ALTER TABLE items ADD COLUMN taken_local TEXT;
ALTER TABLE items ADD COLUMN taken_offset INTEGER;

-- 2. Rows dated before: the old code stored a clock without an offset (EXIF without
--    OffsetTimeOriginal, a date in a file name) as if it were UTC, so `taken_at` read in
--    UTC gives that clock back exactly. That is the rule for every row here; the dates it
--    gets wrong (a clock that had an offset, a file time on a server outside UTC, a Pixel
--    file name) are read again by step 4.
UPDATE items
   SET taken_local = strftime('%Y-%m-%dT%H:%M:%S', taken_at, 'unixepoch')
 WHERE taken_at IS NOT NULL;

-- 3. The same rule for stored suggestions (`suggestion_json` holds a Classification).
UPDATE items
   SET suggestion_json = json_set(
         suggestion_json,
         '$.taken_local', strftime('%Y-%m-%dT%H:%M:%S', json_extract(suggestion_json, '$.taken_at'), 'unixepoch'),
         '$.taken_offset', NULL)
 WHERE suggestion_json IS NOT NULL
   AND json_valid(suggestion_json)
   AND json_type(suggestion_json, '$.taken_at') = 'integer';

-- 4. Read every dated photo and home video again with a probe job that redates it:
--    EXIF and QuickTime dates with their offsets, else the name, else the stored moment
--    read in the server's time zone. A probe already waiting for the item redates too.
UPDATE jobs
   SET payload = json_set(CASE WHEN json_valid(payload) THEN payload ELSE '{}' END, '$.redate', json('true'))
 WHERE kind = 'probe' AND state = 'queued';

INSERT INTO jobs (kind, item_id, payload, state, attempts, run_after, created_at, updated_at)
SELECT 'probe', i.id, '{"redate":true}', 'queued', 0,
       CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER),
       CAST(strftime('%s', 'now') AS INTEGER)
  FROM items i
 WHERE i.status IN ('ready', 'needs_review')
   AND (i.category IN ('photo', 'home_video')
        OR (i.taken_at IS NOT NULL AND i.category NOT IN ('movie', 'episode')))
   AND NOT EXISTS (SELECT 1 FROM jobs j
                    WHERE j.kind = 'probe' AND j.item_id = i.id AND j.state = 'queued')
 ORDER BY i.id;


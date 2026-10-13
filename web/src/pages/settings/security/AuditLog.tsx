/**
 * The audit log for owners: who did what and when, in plain words, a page
 * at a time (cursor pagination). A table on wide screens, rows on phones.
 */
import * as stylex from '@stylexjs/stylex';
import {ListItem} from '@astryxdesign/core/List';
import {Pagination} from '@astryxdesign/core/Pagination';
import {Table, pixel, proportional} from '@astryxdesign/core/Table';
import {Text} from '@astryxdesign/core/Text';
import {useMediaQuery} from '@astryxdesign/core/hooks';
import {useState} from 'react';
import {audit} from '../../../api/client';
import type {AuditEntry} from '../../../api/types';
import {auditDetail, auditEventName} from '../../../components/settings/describe';
import {
  EmptyRows,
  LoadError,
  LoadingRows,
  RowsPanel,
  PanelList,
  Wrap,
} from '../../../components/settings/SectionStates';
import {useResource} from '../../../components/settings/useResource';

const PAGE_SIZE = 20;

const styles = stylex.create({
  wrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
    minWidth: 0,
  },
  tableWrap: {
    overflowX: 'auto',
  },
  cellStack: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  pager: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
  },
  when: {
    whiteSpace: 'nowrap',
    fontVariantNumeric: 'tabular-nums',
  },
});

type AuditRow = {
  id: string;
  at: number;
  who: string;
  what: string;
  detail: string | null;
  ip: string;
};

const THIS_YEAR = new Date().getFullYear();

/** "Oct 12, 11:54 PM", with the year only when it is not this year. */
function formatWhen(at: number): string {
  const date = new Date(at * 1000);
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    year: date.getFullYear() === THIS_YEAR ? undefined : 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(date);
}

function toRow(entry: AuditEntry): AuditRow {
  return {
    id: String(entry.id),
    at: entry.at,
    // The server sends `{id, display_name}`; display_name is null once the person was removed.
    who: entry.user?.display_name ?? (entry.user ? 'Removed person' : 'Not signed in'),
    what: auditEventName(entry.event),
    detail: auditDetail(entry),
    ip: entry.ip ?? '',
  };
}

const COLUMNS = [
  {
    key: 'at',
    header: 'When',
    width: pixel(170),
    renderCell: (row: AuditRow) => (
      <time dateTime={new Date(row.at * 1000).toISOString()} {...stylex.props(styles.when)}>
        {formatWhen(row.at)}
      </time>
    ),
  },
  {key: 'who', header: 'Who', width: proportional(1, {minWidth: 140})},
  {
    key: 'what',
    header: 'What',
    width: proportional(2, {minWidth: 200}),
    renderCell: (row: AuditRow) => (
      <span {...stylex.props(styles.cellStack)}>
        <Text>{row.what}</Text>
        {row.detail ? (
          <Text type="supporting" color="secondary">
            {row.detail}
          </Text>
        ) : null}
      </span>
    ),
  },
  {key: 'ip', header: 'Address', width: pixel(140)},
];

export function AuditLog() {
  const isNarrow = useMediaQuery('(max-width: 799px)');
  // cursors[n] is the cursor for page n + 1 (page 1 has none).
  const [cursors, setCursors] = useState<(string | null)[]>([null]);
  const [page, setPage] = useState(1);
  const cursor = cursors[page - 1] ?? null;
  const entries = useResource(signal => audit.list({limit: PAGE_SIZE, cursor}, signal), {key: `audit:${cursor ?? ''}`});

  const rows = (entries.data?.items ?? []).map(toRow);
  const nextCursor = entries.data?.next_cursor ?? null;

  const goTo = (next: number) => {
    if (next > page && nextCursor) {
      setCursors(current => {
        const copy = current.slice(0, page);
        copy[page] = nextCursor;
        return copy;
      });
    }
    setPage(next);
  };

  if (entries.isLoading && rows.length === 0) return <LoadingRows rows={5} label="Loading the audit log" />;
  if (entries.error !== undefined && !entries.data) {
    return <LoadError title="The audit log did not load" error={entries.error} onRetry={entries.reload} />;
  }
  if (rows.length === 0 && page === 1) {
    return <EmptyRows>Nothing recorded yet. Sign-ins, invites, settings changes and deletions appear here.</EmptyRows>;
  }

  return (
    <div {...stylex.props(styles.wrap)} aria-busy={entries.isPending}>
      {isNarrow ? (
        <PanelList density="balanced">
          {rows.map(row => (
            <ListItem
              key={row.id}
              label={row.detail ? `${row.what}: ${row.detail}` : row.what}
              description={<Wrap>{[row.who, formatWhen(row.at), row.ip].filter(Boolean).join(', ')}</Wrap>}
            />
          ))}
        </PanelList>
      ) : (
        <RowsPanel>
          <div {...stylex.props(styles.tableWrap)}>
            <Table<AuditRow> data={rows} columns={COLUMNS} idKey="id" density="balanced" verticalAlign="top" />
          </div>
        </RowsPanel>
      )}
      {page > 1 || nextCursor ? (
        <div {...stylex.props(styles.pager)}>
          <Pagination
            page={page}
            onChange={goTo}
            hasMore={Boolean(nextCursor)}
            variant="compact"
            label="Audit log pages"
            isDisabled={entries.isPending}
          />
          <Text type="supporting" color="secondary">
            Page {page}
          </Text>
        </div>
      ) : null}
    </div>
  );
}

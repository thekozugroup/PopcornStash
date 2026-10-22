import * as stylex from '@stylexjs/stylex';
import {ProgressBar} from '@astryxdesign/core/ProgressBar';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import type {ConversionSavings} from '../../api/types';
import {formatBytes} from '../../lib/format';

const styles = stylex.create({group: {display: 'flex', flexDirection: 'column', gap: 16, minWidth: 0}, facts: {gap: 24, flexWrap: 'wrap'}});
export const percent = (value: number) => `${value.toLocaleString(undefined, {maximumFractionDigits: 1})}%`;
export function StorageSavings({savings}: {savings: ConversionSavings}) {
  const currentPercent = savings.original_bytes > 0 ? Math.max(0, Math.min(100, savings.current_bytes * 100 / savings.original_bytes)) : 0;
  return <div {...stylex.props(styles.group)}>
    <ProgressBar label={`Current storage: ${formatBytes(savings.current_bytes)} of ${formatBytes(savings.original_bytes)} original. ${formatBytes(savings.saved_bytes)} saved, ${percent(savings.percent)}.`} isLabelHidden value={currentPercent} variant="neutral" />
    <MetadataList orientation="horizontal" xstyle={styles.facts}>
    <MetadataListItem label="Original size">{formatBytes(savings.original_bytes)}</MetadataListItem>
    <MetadataListItem label="Current size">{formatBytes(savings.current_bytes)}</MetadataListItem>
    <MetadataListItem label="Space saved">{formatBytes(savings.saved_bytes)} · {percent(savings.percent)}</MetadataListItem>
  </MetadataList>
  </div>;
}

import * as stylex from '@stylexjs/stylex';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import type {ConversionSavings} from '../../api/types';
import {formatBytes} from '../../lib/format';

const styles = stylex.create({facts: {gap: 24, flexWrap: 'wrap'}});
export const percent = (value: number) => `${value.toLocaleString(undefined, {maximumFractionDigits: 1})}%`;
export function StorageSavings({savings}: {savings: ConversionSavings}) {
  return <MetadataList orientation="horizontal" xstyle={styles.facts}>
    <MetadataListItem label="Original size">{formatBytes(savings.original_bytes)}</MetadataListItem>
    <MetadataListItem label="Current size">{formatBytes(savings.current_bytes)}</MetadataListItem>
    <MetadataListItem label="Space saved">{formatBytes(savings.saved_bytes)} · {percent(savings.percent)}</MetadataListItem>
  </MetadataList>;
}

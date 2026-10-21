import * as stylex from '@stylexjs/stylex';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import type {TorrentFileKind} from '../../api/types';
import {fileKinds} from './fileSelection';

const styles = stylex.create({choices: {display: 'grid', gridTemplateColumns: {default: 'repeat(4, minmax(0, 1fr))', '@media (max-width: 559px)': 'repeat(2, minmax(0, 1fr))'}, gap: 8}});

export function TorrentTypeChoices({value, onChange, disabled, label = 'Include in new torrents', showHelp = true}: {
  value: TorrentFileKind[]; onChange: (value: TorrentFileKind[]) => void; disabled?: boolean; label?: string; showHelp?: boolean;
}) {
  return <VStack gap={2}>
    <Text type="supporting">{label}</Text>
    <div {...stylex.props(styles.choices)}>
      {fileKinds.map(kind => <CheckboxInput key={kind.value} label={kind.label} value={value.includes(kind.value)}
        isDisabled={disabled} onChange={checked => onChange(checked ? [...value, kind.value] : value.filter(v => v !== kind.value))} />)}
    </div>
    {showHelp ? <Text type="supporting" color="secondary">{value.length === 0 ? 'Torrents fetch details, then pause. Choose files from the torrent’s menu and resume when ready.' : 'Choose individual files from a torrent’s menu after its file list arrives. Folder structure is preserved until sorting.'}</Text> : null}
  </VStack>;
}

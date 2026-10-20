import * as stylex from '@stylexjs/stylex';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {VStack} from '@astryxdesign/core/Layout';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import type {FileRuleDraft} from './fileRules';

const styles = stylex.create({sizes: {display: 'grid', gridTemplateColumns: {default: 'minmax(0, 1fr) minmax(0, 1fr) 100px', '@media (max-width: 559px)': 'minmax(0, 1fr)'}, gap: 12}});

export function FileRulesEditor({value, onChange, disabled = false, error}: {value: FileRuleDraft; onChange: (value: FileRuleDraft) => void; disabled?: boolean; error?: string}) {
  const field = (key: keyof FileRuleDraft, next: string) => onChange({...value, [key]: next});
  return <VStack gap={3}>
    <CheckboxInput label="Apply file rules" value={value.enabled} isDisabled={disabled} onChange={enabled => onChange({...value, enabled})} />
    {value.enabled ? <>
      <Text as="p" type="supporting" color="secondary">Every filled condition must match. Names match the filename or a folder, ignoring case and punctuation. Separate alternative names with commas.</Text>
      <TextInput label="Name contains any of" placeholder="Jane Doe, Alex Smith" value={value.include} onChange={v => field('include', v)} isDisabled={disabled} width="100%" />
      <TextInput label="Name excludes" placeholder="sample, trailer" value={value.exclude} onChange={v => field('exclude', v)} isDisabled={disabled} width="100%" />
      <div {...stylex.props(styles.sizes)}>
        <TextInput label="Minimum size" value={value.minimum} onChange={v => field('minimum', v)} isDisabled={disabled} width="100%" />
        <TextInput label="Maximum size" value={value.maximum} onChange={v => field('maximum', v)} isDisabled={disabled} width="100%" />
        <Selector label="Size unit" value={value.unit} onChange={v => field('unit', v)} options={[{value: 'MB', label: 'MB'}, {value: 'GB', label: 'GB'}]} isDisabled={disabled} width="100%" />
      </div>
      <Text as="p" type="supporting" color="secondary">Blank sizes have no limit. MB = 1,000,000 bytes. Shared torrent pieces may contain small fragments of skipped files.</Text>
      {error ? <Text as="p" type="supporting" role="alert">{error}</Text> : null}
    </> : null}
  </VStack>;
}

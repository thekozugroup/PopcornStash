import * as stylex from '@stylexjs/stylex';
import {VStack} from '@astryxdesign/core/Layout';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import type {FileRuleDraft} from './fileRules';

const styles = stylex.create({
  sizes: {display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr) 80px', gap: 12},
  names: {display: 'grid', gridTemplateColumns: {default: 'minmax(0, 1fr) minmax(0, 1fr)', '@media (max-width: 559px)': 'minmax(0, 1fr)'}, gap: 12},
});

export function FileRulesEditor({value, onChange, disabled = false, error}: {value: FileRuleDraft; onChange: (value: FileRuleDraft) => void; disabled?: boolean; error?: string}) {
  const field = (key: keyof FileRuleDraft, next: string) => onChange({...value, enabled: true, [key]: next});
  return <VStack gap={3}>
    <div {...stylex.props(styles.sizes)}>
      <TextInput label="Minimum size" placeholder="No limit" value={value.minimum} onChange={v => field('minimum', v)} isDisabled={disabled} width="100%" />
      <TextInput label="Maximum size" placeholder="No limit" value={value.maximum} onChange={v => field('maximum', v)} isDisabled={disabled} width="100%" />
      <Selector label="Unit" value={value.unit} onChange={v => field('unit', v)} options={[{value: 'MB', label: 'MB'}, {value: 'GB', label: 'GB'}]} isDisabled={disabled} width="100%" />
    </div>
    <div {...stylex.props(styles.names)}>
      <TextInput label="Name contains" placeholder="Any name" value={value.include} onChange={v => field('include', v)} isDisabled={disabled} width="100%" />
      <TextInput label="Name excludes" placeholder="sample, trailer" value={value.exclude} onChange={v => field('exclude', v)} isDisabled={disabled} width="100%" />
    </div>
    <Text as="p" type="supporting" color="secondary">Names match files or folders; separate alternatives with commas.</Text>
    {error ? <Text as="p" type="supporting" role="alert">{error}</Text> : null}
  </VStack>;
}

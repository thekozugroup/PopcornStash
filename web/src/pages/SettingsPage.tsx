/**
 * Settings frame: the Settings heading, a tab strip of sections (owner-only
 * sections hidden from members), and the selected section below.
 */
import * as stylex from '@stylexjs/stylex';
import {Tab, TabList} from '@astryxdesign/core/TabList';
import {Navigate, Outlet, useLocation} from 'react-router';
import {useAuth} from '../auth/AuthProvider';
import {PageHeader} from '../components/PageHeader';

export interface SettingsSection {
  path: string;
  label: string;
  ownerOnly: boolean;
}

export const SETTINGS_SECTIONS: SettingsSection[] = [
  {path: 'vpn', label: 'VPN', ownerOnly: true},
  {path: 'devices', label: 'Devices', ownerOnly: false},
  {path: 'people', label: 'People', ownerOnly: true},
  {path: 'security', label: 'Security', ownerOnly: false},
  {path: 'library', label: 'Library', ownerOnly: true},
  {path: 'system', label: 'System', ownerOnly: true},
];

const styles = stylex.create({
  tabs: {
    marginBottom: {default: 32, '@media (max-width: 520px)': 24},
  },
});

export function useSettingsSections(): SettingsSection[] {
  const {isOwner} = useAuth();
  return SETTINGS_SECTIONS.filter(section => isOwner || !section.ownerOnly);
}

/** `/settings` opens the first section this person can use. */
export function SettingsIndex() {
  const sections = useSettingsSections();
  return <Navigate to={sections[0]?.path ?? 'security'} replace />;
}

export default function SettingsPage() {
  const sections = useSettingsSections();
  const {pathname} = useLocation();
  const current = pathname.split('/')[2] ?? '';

  // No subtitle: the tabs right below already list what Settings holds, and
  // each section opens with one sentence of its own.
  return (
    <>
      <PageHeader title="Settings" />
      <TabList value={current} onChange={() => undefined} hasDivider xstyle={styles.tabs}>
        {sections.map(section => (
          <Tab key={section.path} value={section.path} label={section.label} href={`/settings/${section.path}`} />
        ))}
      </TabList>
      <Outlet />
    </>
  );
}

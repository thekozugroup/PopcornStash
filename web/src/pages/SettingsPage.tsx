/**
 * Settings frame: the Settings heading, the section navigation (owner-only
 * sections hidden from members), and the selected section below. Each
 * section has its own address (`/settings/vpn`, `/settings/system`).
 *
 * The navigation is a tab strip. On phones an owner's six tabs would scroll
 * sideways and hide Libraries and System, so below 640px it becomes one
 * full-width section picker that lists every section; a member's two tabs
 * fit and stay tabs. Both are rendered and swapped by a media query, so the
 * right one is there on the first paint.
 */
import * as stylex from '@stylexjs/stylex';
import {Selector} from '@astryxdesign/core/Selector';
import {Tab, TabList} from '@astryxdesign/core/TabList';
import {Navigate, Outlet, useLocation, useNavigate} from 'react-router';
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
  {path: 'libraries', label: 'Libraries', ownerOnly: true},
  {path: 'system', label: 'System', ownerOnly: true},
];

/** A strip this short fits on any phone, so it stays tabs. */
const TABS_FIT_ON_PHONE = 3;

const styles = stylex.create({
  nav: {
    marginBottom: {default: 32, '@media (max-width: 520px)': 24},
  },
  // Below 640px an owner's tab strip no longer fits.
  tabsOnWide: {
    display: {default: 'block', '@media (max-width: 639px)': 'none'},
  },
  pickerOnPhone: {
    display: {default: 'none', '@media (max-width: 639px)': 'block'},
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
  const navigate = useNavigate();
  const {pathname} = useLocation();
  const current = pathname.split('/')[2] ?? '';
  const hasPicker = sections.length > TABS_FIT_ON_PHONE;

  const tabs = (
    <TabList value={current} onChange={() => undefined} hasDivider>
      {sections.map(section => (
        <Tab key={section.path} value={section.path} label={section.label} href={`/settings/${section.path}`} />
      ))}
    </TabList>
  );

  // No subtitle: the navigation right below already lists what Settings
  // holds, and each section opens with one sentence of its own.
  return (
    <>
      <PageHeader title="Settings" />
      {hasPicker ? (
        <div {...stylex.props(styles.nav)}>
          <div {...stylex.props(styles.tabsOnWide)}>{tabs}</div>
          <nav aria-label="Settings sections" {...stylex.props(styles.pickerOnPhone)}>
            <Selector
              label="Section"
              isLabelHidden
              options={sections.map(section => ({value: section.path, label: section.label}))}
              value={current}
              onChange={value => {
                if (value !== current) navigate(`/settings/${value}`);
              }}
              width="100%"
              presentation="adaptive"
            />
          </nav>
        </div>
      ) : (
        <div {...stylex.props(styles.nav)}>{tabs}</div>
      )}
      <Outlet />
    </>
  );
}

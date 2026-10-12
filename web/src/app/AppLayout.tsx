/**
 * The signed-in app shell: Astryx AppShell with a sticky 228px SideNav on
 * desktop; under 800px a compact top bar with a menu drawer, plus the five
 * most used destinations one tap away. Owns route-change focus, the skip
 * link (AppShell's), and the window-wide file drop overlay.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AppShell} from '@astryxdesign/core/AppShell';
import {useAppShellMobile} from '@astryxdesign/core/AppShell';
import {Badge} from '@astryxdesign/core/Badge';
import {Button} from '@astryxdesign/core/Button';
import {SideNav, SideNavItem, SideNavSection} from '@astryxdesign/core/SideNav';
import {Text} from '@astryxdesign/core/Text';
import {useCallback, useEffect, useRef} from 'react';
import {Link, Outlet, useLocation, useNavigate} from 'react-router';
import {useAuth} from '../auth/AuthProvider';
import {Brand} from '../components/Brand';
import {FileDropOverlay} from '../components/FileDropOverlay';
import {VpnStatusTag} from '../components/VpnStatusTag';
import {useFileDrop} from '../hooks/useFileDrop';
import {useInboxCount} from '../hooks/useInboxCount';
import {NAV_GROUPS, QUICK_DESTINATIONS, isActive} from './navigation';

const MAIN_ID = 'astryx-app-shell-main';

const styles = stylex.create({
  sideNav: {
    width: 228,
  },
  brandRow: {
    display: 'flex',
    alignItems: 'center',
    paddingInline: 6,
    paddingBlockStart: {default: 20, '@media (max-width: 799px)': 0},
    paddingBlockEnd: {default: 20, '@media (max-width: 799px)': 0},
  },
  footer: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-start',
    gap: 12,
    paddingInline: 6,
    paddingBlockStart: 16,
    paddingBlockEnd: 12,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  account: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
    width: '100%',
    minWidth: 0,
  },
  accountName: {
    minWidth: 0,
  },
  main: {
    width: '100%',
    maxWidth: 1320,
    marginInline: 'auto',
    paddingBlockStart: {default: 'clamp(24px, 5vw, 64px)', '@media (max-width: 799px)': 20},
    paddingBlockEnd: {default: 'clamp(40px, 6vw, 80px)', '@media (max-width: 799px)': 48},
    paddingInline: {default: 'clamp(24px, 5vw, 64px)', '@media (max-width: 799px)': 20},
    minWidth: 0,
  },
  quickNav: {
    display: 'grid',
    gridTemplateColumns: 'repeat(5, minmax(0, 1fr))',
    maxWidth: 560,
    marginBottom: 24,
    marginInline: -8,
  },
  quickLink: {
    display: 'grid',
    placeItems: 'center',
    minHeight: 44,
    paddingInline: 2,
    borderRadius: 8,
    color: colorVars['--color-text-secondary'],
    fontSize: 12,
    fontWeight: 500,
    textDecoration: 'none',
    textAlign: 'center',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    backgroundColor: {
      default: 'transparent',
      ':hover': colorVars['--color-neutral'],
    },
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: -2,
  },
  quickLinkActive: {
    color: colorVars['--color-text-primary'],
    backgroundColor: colorVars['--color-neutral'],
  },
});

function QuickNav() {
  const {isMobile} = useAppShellMobile();
  const {pathname} = useLocation();
  if (!isMobile) return null;
  return (
    <nav aria-label="Quick links" {...stylex.props(styles.quickNav)}>
      {QUICK_DESTINATIONS.map(destination => {
        const active = isActive(destination, pathname);
        return (
          <Link
            key={destination.path}
            to={destination.path}
            aria-current={active ? 'page' : undefined}
            {...stylex.props(styles.quickLink, active && styles.quickLinkActive)}>
            {destination.label}
          </Link>
        );
      })}
    </nav>
  );
}

function PopcornSideNav() {
  const {pathname} = useLocation();
  const {isMobile} = useAppShellMobile();
  const {user, signOut} = useAuth();
  const inboxCount = useInboxCount();

  return (
    <SideNav
      aria-label="Main navigation"
      xstyle={isMobile ? undefined : styles.sideNav}
      header={
        <div {...stylex.props(styles.brandRow)}>
          <Brand />
        </div>
      }
      footer={
        <div {...stylex.props(styles.footer)}>
          <VpnStatusTag />
          <div {...stylex.props(styles.account)}>
            <Text type="supporting" color="secondary" maxLines={1} xstyle={styles.accountName}>
              {user?.display_name ?? ''}
            </Text>
            <Button label="Sign out" variant="ghost" size="sm" onClick={() => void signOut()} />
          </div>
        </div>
      }>
      {NAV_GROUPS.map(group => (
        <SideNavSection key={group.title} title={group.title} isHeaderHidden>
          {group.items.map(item => (
            <SideNavItem
              key={item.path}
              label={item.label}
              href={item.path}
              isSelected={isActive(item, pathname)}
              endContent={
                item.path === '/inbox' && inboxCount ? (
                  <Badge variant="neutral" label={String(inboxCount)} />
                ) : undefined
              }
            />
          ))}
        </SideNavSection>
      ))}
    </SideNav>
  );
}

export function AppLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const lastPath = useRef(location.pathname);

  // Move focus to the main region on every in-app navigation so keyboard and
  // screen reader users start at the new page, not in the sidebar.
  useEffect(() => {
    if (lastPath.current === location.pathname) return;
    lastPath.current = location.pathname;
    window.scrollTo({top: 0});
    document.getElementById(MAIN_ID)?.focus({preventScroll: true});
  }, [location.pathname]);

  const onFiles = useCallback(
    (files: File[]) => navigate('/downloads', {state: {droppedFiles: files}}),
    [navigate],
  );
  const {isDragging} = useFileDrop({onFiles});

  return (
    <>
      <AppShell height="auto" variant="section" sideNav={<PopcornSideNav />}>
        <div {...stylex.props(styles.main)}>
          <QuickNav />
          <Outlet />
        </div>
      </AppShell>
      <FileDropOverlay isVisible={isDragging} />
    </>
  );
}

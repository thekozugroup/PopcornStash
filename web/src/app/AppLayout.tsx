/**
 * The signed-in app shell: Astryx AppShell with a sticky 228px SideNav on
 * desktop (Home, every library the person can see with its own icon, Other
 * files, Inbox, Downloads, Settings); under 800px a compact top bar with a
 * menu drawer, plus Home, the first three libraries and Downloads one tap
 * away. Owns the libraries list for every page below it, route-change focus,
 * the skip link (AppShell's), and the window-wide file drop overlay.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AppShell} from '@astryxdesign/core/AppShell';
import {useAppShellMobile} from '@astryxdesign/core/AppShell';
import {Badge} from '@astryxdesign/core/Badge';
import {Button} from '@astryxdesign/core/Button';
import {SideNav, SideNavItem, SideNavSection} from '@astryxdesign/core/SideNav';
import {Text} from '@astryxdesign/core/Text';
import type {LucideIcon} from 'lucide-react';
import {useCallback, useEffect, useRef} from 'react';
import {Link, Outlet, useLocation, useNavigate} from 'react-router';
import {useAuth} from '../auth/AuthProvider';
import {Brand} from '../components/Brand';
import {FileDropOverlay} from '../components/FileDropOverlay';
import {VpnStatusTag} from '../components/VpnStatusTag';
import {useFileDrop} from '../hooks/useFileDrop';
import {useInboxCount} from '../hooks/useInboxCount';
import {LibrariesProvider, useLibraries} from './LibrariesProvider';
import {NAV_ICON_SIZE, NAV_ICON_STROKE, isActive, navGroups, quickDestinations} from './navigation';

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
    // Room between cells so a long label's pill ("Downloads") never touches its neighbour.
    columnGap: 4,
    maxWidth: 560,
    marginBottom: 24,
    // The current link's pill lines up with the page gutter, like the content below it.
    marginInline: 0,
  },
  // Icon above its label, like the sidebar's icon before its label; the
  // current link's fill covers both.
  quickLink: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 4,
    minWidth: 0,
    minHeight: 52,
    // No side padding: "Downloads" needs the whole cell on a 390px phone.
    paddingInline: 0,
    paddingBlock: 6,
    borderRadius: 8,
    color: colorVars['--color-text-secondary'],
    fontSize: 12,
    fontWeight: 500,
    lineHeight: '16px',
    textDecoration: 'none',
    textAlign: 'center',
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
  quickLabel: {
    maxWidth: '100%',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  navIcon: {
    flexShrink: 0,
  },
});

const columnStyles = stylex.create({
  // Fewer libraries, fewer cells: the row always fills its width.
  columns: (count: number) => ({
    gridTemplateColumns: `repeat(${count}, minmax(0, 1fr))`,
  }),
});

/** A destination's line icon; it takes the colour of the label beside it. */
function NavIcon({icon: Icon}: {icon: LucideIcon}) {
  return (
    <Icon
      size={NAV_ICON_SIZE}
      strokeWidth={NAV_ICON_STROKE}
      aria-hidden="true"
      {...stylex.props(styles.navIcon)}
    />
  );
}

function QuickNav() {
  const {isMobile} = useAppShellMobile();
  const {pathname} = useLocation();
  const {libraries} = useLibraries();
  if (!isMobile) return null;
  const destinations = quickDestinations(libraries ?? []);
  return (
    <nav aria-label="Quick links" {...stylex.props(styles.quickNav, columnStyles.columns(destinations.length))}>
      {destinations.map(destination => {
        const active = isActive(destination, pathname);
        return (
          <Link
            key={destination.path}
            to={destination.path}
            title={destination.libraryId ? destination.label : undefined}
            aria-current={active ? 'page' : undefined}
            {...stylex.props(styles.quickLink, active && styles.quickLinkActive)}>
            <NavIcon icon={destination.icon} />
            <span {...stylex.props(styles.quickLabel)}>{destination.label}</span>
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
  const {libraries} = useLibraries();

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
      {navGroups(libraries ?? []).map(group => (
        <SideNavSection key={group.title} title={group.title} isHeaderHidden>
          {group.items.map(item => (
            <SideNavItem
              key={item.path}
              label={item.label}
              // A long library name ends in an ellipsis; the whole name stays in the
              // native tooltip (SideNavItem passes other attributes to its link).
              {...(item.libraryId ? {title: item.label} : {})}
              icon={<NavIcon icon={item.icon} />}
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
    <LibrariesProvider>
      <AppShell height="auto" variant="section" sideNav={<PopcornSideNav />}>
        <div {...stylex.props(styles.main)}>
          <QuickNav />
          <Outlet />
        </div>
      </AppShell>
      <FileDropOverlay isVisible={isDragging} />
    </LibrariesProvider>
  );
}

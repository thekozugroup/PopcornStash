/**
 * Settings, VPN (owner). The VPN status in words, NordVPN quick setup as the
 * main path, WireGuard and OpenVPN files as the alternatives, Reconnect and
 * Remove, then the torrent and web download settings.
 */
import * as stylex from '@stylexjs/stylex';
import {Collapsible} from '@astryxdesign/core/Collapsible';
import {Text} from '@astryxdesign/core/Text';
import {useRef} from 'react';
import type {VpnStatus} from '../../api/types';
import {LoadingRows} from '../../components/settings/SectionStates';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {SettingsSectionHeader} from './SettingsSectionHeader';
import {NordVpnSetup} from './vpn/NordVpnSetup';
import {TorrentSettings} from './vpn/TorrentSettings';
import {useLiveVpn} from './vpn/useLiveVpn';
import {VpnFileSetup} from './vpn/VpnFileSetup';
import {VpnStatusPanel} from './vpn/VpnStatusPanel';
import {vpnView} from './vpn/vpnView';

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
    marginTop: 32,
  },
  disclosure: {
    maxWidth: 560,
  },
  disclosureBody: {
    paddingTop: 16,
  },
  note: {
    lineHeight: 1.55,
    maxWidth: '62ch',
    textWrap: 'pretty',
  },
});

function VpnSettings() {
  const live = useLiveVpn();
  const status = live.status;
  const isConfigured = Boolean(status?.configured);
  const view = status === null ? null : vpnView(status, false);
  const isDegraded = view?.isDegraded === true;
  // STASH_ALLOW_NO_VPN: a VPN set up here would not be used, so the forms are not offered.
  const isNoVpn = view?.isNoVpn === true;
  const panelRef = useRef<HTMLElement>(null);

  // After a setup the form collapses; bring the live status into view and
  // move focus there so the "connecting" state is seen and announced.
  const onConnected = (next: VpnStatus) => {
    live.accept(next);
    window.requestAnimationFrame(() => {
      const panel = panelRef.current;
      if (!panel) return;
      const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      panel.scrollIntoView({behavior: reduceMotion ? 'auto' : 'smooth', block: 'start'});
      panel.focus({preventScroll: true});
    });
  };

  return (
    <>
      <SettingsSectionHeader
        title="VPN"
        subtitle={
          isNoVpn
            ? 'Where torrent traffic goes on this server.'
            : 'Torrents only run through this VPN. Set it up once and PopcornStash keeps it connected.'
        }
      />
      <VpnStatusPanel
        status={status}
        error={live.error}
        isLoading={live.isLoading}
        onReload={live.reload}
        onChange={live.accept}
        panelRef={panelRef}
      />

      <div {...stylex.props(styles.groups)}>
        {status === null ? (
          <SettingsGroup title="Set up the VPN" description="Checking what is set up now.">
            <LoadingRows rows={2} label="Checking the VPN" />
          </SettingsGroup>
        ) : isNoVpn ? (
          <SettingsGroup title="Set up the VPN" description="Not used while STASH_ALLOW_NO_VPN is on.">
            <Text as="p" color="secondary" xstyle={styles.note}>
              To send torrents through a VPN, remove STASH_ALLOW_NO_VPN from the container settings and restart the
              container. The NordVPN and file setup then appear here.
            </Text>
          </SettingsGroup>
        ) : (
          <SettingsGroup
            title={isConfigured ? 'Use a different VPN' : 'Connect NordVPN'}
            description={
              isDegraded
                ? 'You can set it up now. It connects once torrents can run on this server.'
                : isConfigured
                  ? 'Setting up a new VPN replaces the current one. Torrents pause while it connects.'
                  : 'The quickest way to protect torrents. You need a NordVPN plan and two minutes.'
            }>
            {isConfigured ? (
              <Collapsible
                trigger={<Text weight="semibold">Connect NordVPN with a new token</Text>}
                defaultIsOpen={false}
                chevronPosition="start"
                xstyle={styles.disclosure}>
                <div {...stylex.props(styles.disclosureBody)}>
                  <NordVpnSetup isPrimary={false} onConnected={onConnected} />
                </div>
              </Collapsible>
            ) : (
              <NordVpnSetup isPrimary onConnected={onConnected} />
            )}
            <Collapsible
              trigger={<Text weight="semibold">Use a WireGuard or OpenVPN file instead</Text>}
              defaultIsOpen={false}
              chevronPosition="start"
              xstyle={styles.disclosure}>
              <div {...stylex.props(styles.disclosureBody)}>
                <VpnFileSetup onConnected={onConnected} />
              </div>
            </Collapsible>
          </SettingsGroup>
        )}

        <TorrentSettings isPrimary={isConfigured} isNoVpn={isNoVpn} />
      </div>
    </>
  );
}

export default function VpnSettingsPage() {
  return (
    <OwnerOnly>
      <VpnSettings />
    </OwnerOnly>
  );
}

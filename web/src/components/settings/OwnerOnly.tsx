/**
 * Guards an owner-only settings section. Members never see these tabs, but a
 * shared or bookmarked link can still land here, so say why plainly.
 */
import {Button} from '@astryxdesign/core/Button';
import type {ReactNode} from 'react';
import {useAuth} from '../../auth/AuthProvider';
import {EmptyPanel} from '../EmptyPanel';
import {useDocumentTitle} from '../PageHeader';

function OwnerOnlyNotice() {
  useDocumentTitle('Settings');
  return (
    <EmptyPanel
      title="Only the owner can change this"
      description="Your own passkeys, browsers and connected devices are under Security and Devices."
      actions={<Button label="Open Security" variant="secondary" href="/settings/security" />}
    />
  );
}

export function OwnerOnly({children}: {children: ReactNode}) {
  const {isOwner} = useAuth();
  return isOwner ? children : <OwnerOnlyNotice />;
}

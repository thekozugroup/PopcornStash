/**
 * Explains, in place, why passkeys cannot be used on this page and how to
 * fix it. Renders nothing when passkeys are available.
 */
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {publicUrlForCurrentPage, type PasskeyAvailability} from '../auth/passkeyAvailability';

export function PasskeyNotice({availability}: {availability: PasskeyAvailability}) {
  if (availability.ok) return null;
  const canJump =
    availability.publicUrl !== null &&
    availability.publicUrl !== window.location.origin &&
    (availability.blocker === 'insecure-context' || availability.blocker === 'wrong-address');

  return (
    <Banner
      status="warning"
      title={availability.title}
      description={availability.description}
      collapsible={false}
      endContent={
        canJump && availability.publicUrl ? (
          <Button
            label={`Open ${new URL(availability.publicUrl).host}`}
            href={publicUrlForCurrentPage(availability.publicUrl)}
            variant="secondary"
          />
        ) : undefined
      }
    />
  );
}

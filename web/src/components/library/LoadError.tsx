/**
 * A page section that could not load: what failed, the server's own reason,
 * and a way to try again. Never blames the person.
 */
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {errorMessage} from '../../lib/errors';

export function LoadError({title, error, onRetry}: {title: string; error: unknown; onRetry: () => void}) {
  return (
    <Banner
      status="error"
      title={title}
      description={errorMessage(error, 'PopcornStash did not answer. Check the connection and try again.')}
      collapsible={false}
      endContent={<Button label="Try again" variant="secondary" onClick={onRetry} />}
    />
  );
}

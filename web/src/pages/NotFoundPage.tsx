/**
 * Unknown address inside the app.
 */
import {Button} from '@astryxdesign/core/Button';
import {EmptyPanel} from '../components/EmptyPanel';
import {useDocumentTitle} from '../components/PageHeader';

export default function NotFoundPage() {
  useDocumentTitle('Page not found');
  return (
    <EmptyPanel
      headingLevel={2}
      title="Page not found"
      description="That address does not match anything in PopcornStash."
      actions={<Button label="Go to Home" href="/" variant="secondary" />}
    />
  );
}

/**
 * Unknown address inside the app: say so plainly and offer the way back.
 * The sidebar (or the quick links on phones) stays available for anywhere else.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Text} from '@astryxdesign/core/Text';
import {useLocation} from 'react-router';
import {PageHeader} from '../components/PageHeader';

const styles = stylex.create({
  path: {
    overflowWrap: 'anywhere',
  },
});

export default function NotFoundPage() {
  const {pathname} = useLocation();
  return (
    <PageHeader
      title="Page not found"
      subtitle={
        <Text as="span" color="secondary" xstyle={styles.path}>
          {`Nothing in PopcornStash lives at ${pathname}. The link may be old or mistyped.`}
        </Text>
      }
      actions={<Button label="Go to Home" variant="primary" href="/" />}
    />
  );
}

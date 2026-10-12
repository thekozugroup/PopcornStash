/**
 * One show (`/shows/:name`): stub. Heading and subtitle only.
 */
import {useParams} from 'react-router';
import {PageHeader} from '../components/PageHeader';

export default function ShowPage() {
  const {name = ''} = useParams();
  return <PageHeader title={name || 'Show'} subtitle="Seasons and episodes, in order." />;
}

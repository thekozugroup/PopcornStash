/**
 * TEMPORARY visual harness for MediaGrid/MediaTile screenshots. Reverted after capture.
 */
import {PageHeader} from '../components/PageHeader';
import {MediaGrid, MediaGridSkeleton} from '../components/MediaGrid';
import {MediaTile} from '../components/MediaTile';
import {Heading} from '@astryxdesign/core/Heading';

const titles = ['The Long Summer Road', 'Northern Lights Over Tromsø, the Extended Edition', 'Paper Boats', 'Kestrel', 'A Quiet Harbour', 'Salt and Cedar', 'Midnight Bakery', 'Ferris'];

export default function MoviesPage() {
  return (
    <>
      <PageHeader title="Movies" subtitle="Every film in your library, newest first." />
      <MediaGrid shape="poster" label="Movies">
        {titles.map((title, i) => (
          <MediaTile key={title} href={`/item/${i}`} title={title} meta={`${2009 + i} · 1 h ${40 + i} min`} shape="poster" imageSrc={i === 3 ? null : `/api/items/${i}/thumb?size=sm`} progress={i === 1 ? 0.42 : null} />
        ))}
      </MediaGrid>
      <div style={{height: 40}} />
      <Heading level={2}>Home Videos</Heading>
      <div style={{height: 16}} />
      <MediaGrid shape="frame" label="Home Videos">
        {titles.slice(0, 4).map((title, i) => (
          <MediaTile key={title} href={`/item/v${i}`} title={`Beach day ${i + 1}`} meta="13 Jun 2024 · 2 min" shape="frame" imageSrc={`/api/items/f${i}/thumb`} />
        ))}
      </MediaGrid>
      <div style={{height: 40}} />
      <Heading level={2}>Photos</Heading>
      <div style={{height: 16}} />
      <MediaGrid shape="square" label="Photos">
        {Array.from({length: 14}, (_, i) => (
          <MediaTile key={i} href={`/item/p${i}`} title={`IMG_${4000 + i}.HEIC`} meta="13 Jun 2024" shape="square" isTextHidden imageSrc={`/api/items/p${i}/thumb`} />
        ))}
      </MediaGrid>
      <div style={{height: 40}} />
      <MediaGridSkeleton shape="poster" count={4} />
    </>
  );
}

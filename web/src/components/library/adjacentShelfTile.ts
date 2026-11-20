// Compact sections share a top edge even when a wrapping heading pushes its tiles down.
const sameRowTolerance = 1;
const sectionTop = (row: HTMLUListElement) => (row.closest('section') ?? row).getBoundingClientRect().top;

export function adjacentShelfTile(row: HTMLUListElement, current: HTMLAnchorElement, direction: 'ArrowUp' | 'ArrowDown') {
  const doc = row.ownerDocument;
  const origin = sectionTop(row);
  const sign = direction === 'ArrowDown' ? 1 : -1;
  const shelves = Array.from(doc.querySelectorAll<HTMLUListElement>('ul[data-media-shelf]'), shelf => ({
    shelf, offset: (sectionTop(shelf) - origin) * sign,
  })).filter(({offset}) => offset > sameRowTolerance).sort((a, b) => a.offset - b.offset);

  const tile = current.getBoundingClientRect();
  const center = tile.left + tile.width / 2;
  let nearest: HTMLAnchorElement | undefined;
  let rowOffset: number | undefined;
  let distance = Infinity;
  for (const {shelf, offset} of shelves) {
    if (rowOffset !== undefined && offset > rowOffset + sameRowTolerance) break;
    const bounds = shelf.getBoundingClientRect();
    const left = Math.max(0, bounds.left);
    const right = Math.min(doc.documentElement.clientWidth, bounds.right);
    if (left >= right || !bounds.height) continue;
    for (const link of shelf.querySelectorAll<HTMLAnchorElement>('li > a')) {
      const box = link.getBoundingClientRect();
      if (!box.width || !box.height || box.right <= left || box.left >= right) continue;
      if (!link.checkVisibility({visibilityProperty: true})) continue;
      rowOffset ??= offset;
      const delta = Math.abs(box.left + box.width / 2 - center);
      if (delta < distance) {
        distance = delta;
        nearest = link;
      }
    }
  }
  return nearest;
}

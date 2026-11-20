import {describe, expect, it} from 'vitest';
import {adjacentShelfTile} from './adjacentShelfTile';

function tile(x: number, visible = true) {
  return {
    getBoundingClientRect: () => ({left: x, right: x + 80, width: 80, height: 60}),
    checkVisibility: () => visible,
  } as unknown as HTMLAnchorElement;
}

// Section and scroller geometry are independent: a long heading can wrap in one compact shelf.
function layout() {
  const rows: HTMLUListElement[] = [];
  const doc = {documentElement: {clientWidth: 800}, querySelectorAll: () => rows};
  return (top: number, left: number, right: number, tiles: HTMLAnchorElement[], headerHeight = 36) => {
    const row = {
      ownerDocument: doc,
      closest: () => ({getBoundingClientRect: () => ({top})}),
      getBoundingClientRect: () => ({top: top + headerHeight, left, right, height: 90}),
      querySelectorAll: () => tiles,
    } as unknown as HTMLUListElement;
    rows.push(row);
    return row;
  };
}

describe('vertical shelf focus', () => {
  it('skips compact siblings on the same row in both directions, including below a wrapped heading', () => {
    const shelf = layout();
    const above = tile(40), middleLeft = tile(30), middleRight = tile(470), below = tile(480);
    shelf(0, 0, 800, [above]);
    const left = shelf(200, 0, 240, [middleLeft], 80);
    const right = shelf(200.5, 420, 760, [middleRight]);
    shelf(440, 0, 800, [below]);
    expect(adjacentShelfTile(left, middleLeft, 'ArrowDown')).toBe(below);
    expect(adjacentShelfTile(right, middleRight, 'ArrowUp')).toBe(above);
  });

  it('compares tiles across all shelves in the adjacent row before considering a closer column farther away', () => {
    const shelf = layout();
    const source = tile(520), other = tile(100), target = tile(490);
    const upper = shelf(0, 0, 800, [source]);
    shelf(200, 0, 260, [other]);
    shelf(200, 430, 800, [target]);
    const exact = tile(520);
    const lower = shelf(400, 0, 800, [exact]);
    expect(adjacentShelfTile(upper, source, 'ArrowDown')).toBe(target);
    expect(adjacentShelfTile(lower, exact, 'ArrowUp')).toBe(target);
  });

  it('skips empty or hidden rows and ignores tiles clipped by the carousel or screen', () => {
    const shelf = layout();
    const source = tile(680), visible = tile(455);
    const upper = shelf(0, 0, 800, [source]);
    shelf(150, 0, 800, []);
    shelf(200, 0, 800, [tile(680, false)]);
    shelf(350, 0, 520, [visible, tile(680)]);
    shelf(350, 540, 950, [tile(801)]);
    expect(adjacentShelfTile(upper, source, 'ArrowDown')).toBe(visible);
  });

  it('leaves focus alone when only a same-row sibling exists', () => {
    const shelf = layout();
    const a = tile(40), b = tile(430);
    const left = shelf(200, 0, 210, [a]);
    const right = shelf(200, 400, 640, [b]);
    expect(adjacentShelfTile(left, a, 'ArrowDown')).toBeUndefined();
    expect(adjacentShelfTile(right, b, 'ArrowUp')).toBeUndefined();
  });
});

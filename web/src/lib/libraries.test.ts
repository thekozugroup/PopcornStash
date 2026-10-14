import {describe, expect, it} from 'vitest';
import {holdsLine, kindLine} from './libraries';

describe('holdsLine', () => {
  it('says what a library with files holds, as a fact', () => {
    expect(holdsLine('video', 'one_offs')).toBe('Videos, arranged as one-offs');
    expect(holdsLine('video', 'episodes')).toBe('Videos, arranged as series');
    expect(holdsLine('photos', null)).toBe('Photos');
    expect(kindLine({content_type: 'video', structure: 'episodes'})).toBe('Series');
  });
});

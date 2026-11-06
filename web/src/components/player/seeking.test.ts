import {describe, expect, it} from 'vitest';
import {positionAt, previewSecond} from './seeking';

describe('seek previews', () => {
  it('samples bounded frames, including the end of short and long videos', () => {
    expect(previewSecond(7.9, 60)).toBe(6);
    expect(previewSecond(999, 60)).toBe(58);
    expect(previewSecond(0, 0.5)).toBe(0);
    expect(previewSecond(3600, 7200)).toBe(3600);
    expect(previewSecond(-10, 60)).toBe(0);
    expect(previewSecond(Infinity, 60)).toBe(0);
    expect(previewSecond(10, NaN)).toBe(0);
  });
  it('matches the slider thumb inset and locale direction at both edges', () => {
    expect(positionAt(10, 0, 120, 60, false)).toBe(0);
    expect(positionAt(110, 0, 120, 60, false)).toBe(60);
    expect(positionAt(60, 0, 120, 60, false)).toBe(30);
    expect(positionAt(-20, 0, 120, 60, true)).toBe(60);
    expect(positionAt(999, 0, 120, 60, true)).toBe(0);
  });
});

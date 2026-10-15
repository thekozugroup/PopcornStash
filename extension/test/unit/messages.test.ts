import {describe, expect, it} from 'vitest';
import {isExtensionMessage} from '../../src/lib/messages';

describe('destination messages', () => {
  it('accepts automatic and a chosen library without accepting malformed destinations', () => {
    const send = {type: 'popcornstash:send', url: 'https://example.org/film.mp4'};
    expect(isExtensionMessage(send)).toBe(true);
    expect(isExtensionMessage({...send, libraryId: 'visible-library'})).toBe(true);
    for (const libraryId of [null, 42, {}, 'a'.repeat(257)]) {
      expect(isExtensionMessage({...send, libraryId})).toBe(false);
    }
  });
});

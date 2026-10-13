/**
 * The right-click menu: "Send link / video / audio / image to PopcornStash".
 * A click sends the link or the media's own address, with the page it was on,
 * and reports the result; anything else is left alone.
 */
import {describe, expect, it} from 'vitest';
import {MENU, MENU_ITEMS, menuRequest, onMenuClicked} from '../../src/lib/context-menu';
import {NOT_PAIRED} from '../../src/lib/errors';
import type {SendRequest, SendResult} from '../../src/lib/send';

const PAGE = 'https://videos.example.org/watch/42';
const FRAME = 'https://player.example.net/embed/42';
const LINK = 'https://files.example.org/Sunset.Drive.2020.mp4';
const MEDIA = 'https://cdn.example.net/clip.webm';

describe('context menu items', () => {
  it('offers one item per kind of thing, each in its own context', () => {
    expect(MENU_ITEMS.map(item => [item.id, item.title, item.contexts])).toEqual([
      [MENU.link, 'Send link to PopcornStash', ['link']],
      [MENU.video, 'Send video to PopcornStash', ['video']],
      [MENU.audio, 'Send audio to PopcornStash', ['audio']],
      [MENU.image, 'Send image to PopcornStash', ['image']],
    ]);
  });
});

describe('menuRequest', () => {
  it('sends the link for "Send link", with the page it was on', () => {
    expect(menuRequest({menuItemId: MENU.link, linkUrl: LINK, srcUrl: MEDIA, pageUrl: PAGE})).toEqual({
      url: LINK,
      pageUrl: PAGE,
    });
  });

  it('sends the media itself for video, audio and image, preferring the frame it plays in', () => {
    for (const id of [MENU.video, MENU.audio, MENU.image]) {
      expect(menuRequest({menuItemId: id, srcUrl: MEDIA, linkUrl: LINK, frameUrl: FRAME, pageUrl: PAGE})).toEqual({
        url: MEDIA,
        pageUrl: FRAME,
      });
    }
  });

  it('falls back to the tab address when Chrome gives no page', () => {
    expect(menuRequest({menuItemId: MENU.image, srcUrl: MEDIA}, PAGE)).toEqual({url: MEDIA, pageUrl: PAGE});
    expect(menuRequest({menuItemId: MENU.image, srcUrl: MEDIA})).toEqual({url: MEDIA, pageUrl: undefined});
  });

  it('sends nothing without an address, or for a menu item that is not ours', () => {
    expect(menuRequest({menuItemId: MENU.link, srcUrl: MEDIA, pageUrl: PAGE})).toBeNull();
    expect(menuRequest({menuItemId: MENU.video, linkUrl: LINK, pageUrl: PAGE})).toBeNull();
    expect(menuRequest({menuItemId: 'another-extension-item', srcUrl: MEDIA, linkUrl: LINK})).toBeNull();
  });
});

describe('onMenuClicked', () => {
  it('sends what was clicked and reports the result for that address', async () => {
    const sent: SendRequest[] = [];
    const reported: [SendResult, string][] = [];
    const result: SendResult = {ok: false, problem: NOT_PAIRED, server: null};
    await onMenuClicked({menuItemId: MENU.video, srcUrl: MEDIA, pageUrl: PAGE}, {url: PAGE}, {
      send: async request => {
        sent.push(request);
        return result;
      },
      report: (outcome, url) => reported.push([outcome, url]),
    });
    expect(sent).toEqual([{url: MEDIA, pageUrl: PAGE}]);
    expect(reported).toEqual([[result, MEDIA]]);
  });

  it('does nothing when there is nothing to send', async () => {
    let calls = 0;
    await onMenuClicked({menuItemId: MENU.link, pageUrl: PAGE}, undefined, {
      send: async () => {
        calls += 1;
        throw new Error('not sent');
      },
      report: () => (calls += 1),
    });
    expect(calls).toBe(0);
  });
});

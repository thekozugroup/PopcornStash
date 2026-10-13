/**
 * The right-click menu: "Send link / video / audio / image to PopcornStash".
 * Kept apart from the service worker so what a click sends can be tested
 * without Chrome.
 */

import type {SendRequest, SendResult} from './send';

export const MENU = {
  link: 'popcornstash-send-link',
  video: 'popcornstash-send-video',
  audio: 'popcornstash-send-audio',
  image: 'popcornstash-send-image',
} as const;

type MenuId = (typeof MENU)[keyof typeof MENU];
const MENU_IDS: ReadonlySet<string> = new Set(Object.values(MENU));

type MenuContext = `${chrome.contextMenus.ContextType}`;

export const MENU_ITEMS: ReadonlyArray<{id: MenuId; title: string; contexts: [MenuContext]}> = [
  {id: MENU.link, title: 'Send link to PopcornStash', contexts: ['link']},
  {id: MENU.video, title: 'Send video to PopcornStash', contexts: ['video']},
  {id: MENU.audio, title: 'Send audio to PopcornStash', contexts: ['audio']},
  {id: MENU.image, title: 'Send image to PopcornStash', contexts: ['image']},
];

/** The parts of Chrome's `OnClickData` a click is judged by. */
export interface MenuClick {
  menuItemId: string | number;
  linkUrl?: string | undefined;
  srcUrl?: string | undefined;
  frameUrl?: string | undefined;
  pageUrl?: string | undefined;
}

/**
 * What a click sends: the link for "Send link", the media's own address for
 * video, audio and image, with the page it was on (the frame it plays in, else
 * the page, else the tab) for the Referer. Null when there is nothing to send
 * or the item is not one of ours.
 */
export function menuRequest(info: MenuClick, tabUrl?: string): SendRequest | null {
  if (!MENU_IDS.has(String(info.menuItemId))) return null;
  const url = info.menuItemId === MENU.link ? info.linkUrl : info.srcUrl;
  if (!url) return null;
  return {url, pageUrl: info.frameUrl ?? info.pageUrl ?? tabUrl};
}

export interface MenuDeps {
  send: (request: SendRequest) => Promise<SendResult>;
  /** Tell the person how it went (a notification). */
  report: (result: SendResult, url: string) => void;
}

/** Handle a menu click: send what it points at, then report the result. */
export async function onMenuClicked(
  info: MenuClick,
  tab: {url?: string | undefined} | undefined,
  deps: MenuDeps,
): Promise<void> {
  const request = menuRequest(info, tab?.url);
  if (!request) return;
  deps.report(await deps.send(request), request.url);
}

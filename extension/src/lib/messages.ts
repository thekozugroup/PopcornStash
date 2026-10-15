/** Messages from extension pages to the background service worker. */

import type {SendResult} from './send';

export interface SendMessage {
  type: 'popcornstash:send';
  libraryId?: string | undefined;
  url: string;
}

export type ExtensionMessage = SendMessage;

export function isExtensionMessage(value: unknown): value is ExtensionMessage {
  if (typeof value !== 'object' || value === null) return false;
  const message = value as {type?: unknown; url?: unknown; libraryId?: unknown};
  return message.type === 'popcornstash:send' && typeof message.url === 'string' &&
    (message.libraryId === undefined || (typeof message.libraryId === 'string' && message.libraryId.length <= 256));
}

/** Ask the background to send a pasted link. */
export async function sendFromPage(url: string, libraryId?: string): Promise<SendResult> {
  const message: SendMessage = {type: 'popcornstash:send', url, ...(libraryId ? {libraryId} : {})};
  return (await chrome.runtime.sendMessage(message)) as SendResult;
}

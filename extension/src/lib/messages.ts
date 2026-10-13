/** Messages from extension pages to the background service worker. */

import type {SendResult} from './send';

export interface SendMessage {
  type: 'popcornstash:send';
  url: string;
}

export type ExtensionMessage = SendMessage;

export function isExtensionMessage(value: unknown): value is ExtensionMessage {
  if (typeof value !== 'object' || value === null) return false;
  const message = value as {type?: unknown; url?: unknown};
  return message.type === 'popcornstash:send' && typeof message.url === 'string';
}

/** Ask the background to send a pasted link. */
export async function sendFromPage(url: string): Promise<SendResult> {
  const message: SendMessage = {type: 'popcornstash:send', url};
  return (await chrome.runtime.sendMessage(message)) as SendResult;
}

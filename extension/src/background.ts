/**
 * Background service worker: context menu items, notifications, sending
 * pasted links for the popup, and (when turned on) taking over Chrome's own
 * downloads. Never logs tokens, cookies or full download URLs.
 */

import {listDownloads, type Connection} from './lib/api';
import {MENU_ITEMS, onMenuClicked} from './lib/context-menu';
import type {Problem} from './lib/errors';
import {chromeCopyOutcome, serverProgress, shouldIntercept, type ServerProgress} from './lib/intercept';
import {linkName} from './lib/links';
import {isExtensionMessage} from './lib/messages';
import {stashIdentity} from './lib/payload';
import {sendToStash, usableConnection, type SendResult} from './lib/send';
import {getConnection, getPrefs} from './lib/storage';

const ICON = 'icons/icon-128.png';

// ---------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------

function createMenus(): void {
  chrome.contextMenus.removeAll(() => {
    for (const item of MENU_ITEMS) chrome.contextMenus.create(item);
  });
}

chrome.runtime.onInstalled.addListener(details => {
  createMenus();
  if (details.reason === 'install') void chrome.runtime.openOptionsPage();
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  void onMenuClicked(info, tab, {send: sendToStash, report: notifyResult});
});

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/** What a click on a notification opens, encoded in its id so it survives a worker restart. */
type ClickTarget = 'downloads' | 'options' | 'none';
let notificationCount = 0;

function notify(target: ClickTarget, title: string, message: string, contextMessage?: string): void {
  notificationCount += 1;
  const id = `${target}:${Date.now()}:${notificationCount}`;
  const options: chrome.notifications.NotificationCreateOptions = {
    type: 'basic',
    iconUrl: ICON,
    title,
    message,
    priority: 0,
  };
  if (contextMessage) options.contextMessage = contextMessage;
  chrome.notifications.create(id, options, () => void chrome.runtime.lastError);
}

function notifyProblem(problem: Problem, prefix?: string): void {
  const target: ClickTarget = problem.repair ? 'options' : 'none';
  notify(target, prefix ? `${prefix}: ${problem.title}` : problem.title, problem.message);
}

function notifyResult(result: SendResult, url: string): void {
  if (result.ok) {
    notify('downloads', 'Sent to PopcornStash', result.name || linkName(url), result.note ?? undefined);
  } else {
    notifyProblem(result.problem);
  }
}

chrome.notifications.onClicked.addListener(id => {
  const target = id.split(':', 1)[0] as ClickTarget;
  chrome.notifications.clear(id);
  if (target === 'options') {
    void chrome.runtime.openOptionsPage();
  } else if (target === 'downloads') {
    void getConnection().then(connection => {
      if (connection) void chrome.tabs.create({url: `${connection.server}/downloads`});
    });
  }
});

// ---------------------------------------------------------------------------
// Popup: send a pasted link
// ---------------------------------------------------------------------------

chrome.runtime.onMessage.addListener((message: unknown, sender, sendResponse) => {
  // Only this extension's own pages may ask (never web pages or other extensions).
  if (sender.id !== chrome.runtime.id || !isExtensionMessage(message)) return false;
  void sendToStash({url: message.url, libraryId: message.libraryId}).then(sendResponse);
  return true;
});

// ---------------------------------------------------------------------------
// Download interception ("Send all my browser downloads to PopcornStash")
// ---------------------------------------------------------------------------

/** Downloads already being handled by this worker. */
const handling = new Set<number>();
const CONFIRM_TIMEOUT_MS = 10_000;
const CONFIRM_POLL_MS = 1_000;

async function settle(promise: Promise<unknown>): Promise<void> {
  try {
    await promise;
  } catch {
    // The download may have finished or been removed in the meantime.
  }
}

/** Wait until the server starts fetching the download it accepted, fails, or the timeout passes. */
async function confirmServerStarted(connection: Connection, id: string): Promise<ServerProgress> {
  const deadline = Date.now() + CONFIRM_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try {
      const items = await listDownloads(connection, 20);
      const state = serverProgress(items.find(item => item.id === id));
      if (state !== 'waiting') return state;
    } catch {
      // Listing is only a confirmation; the server already accepted the link.
      return 'started';
    }
    await new Promise(resolve => setTimeout(resolve, CONFIRM_POLL_MS));
  }
  // Still queued behind other downloads: the server has it.
  return 'started';
}

async function onDownloadCreated(item: chrome.downloads.DownloadItem): Promise<void> {
  if (handling.has(item.id)) return;
  const prefs = await getPrefs();
  if (!prefs.interceptDownloads) return;
  const stored = await getConnection();
  if (!stored) return;
  const decision = shouldIntercept(item, stashIdentity(stored.server, stored.serverName));
  if (!decision.intercept) return;

  handling.add(item.id);
  try {
    // Hold Chrome's copy while PopcornStash takes over.
    await settle(chrome.downloads.pause(item.id));
    const result = await sendToStash({
      url: decision.url,
      pageUrl: item.referrer || undefined,
      filename: item.filename || undefined,
    });

    if (!result.ok) {
      await settle(chrome.downloads.resume(item.id));
      notifyProblem(result.problem, 'Kept in Chrome');
      return;
    }

    const usable = await usableConnection();
    const progress = 'connection' in usable ? await confirmServerStarted(usable.connection, result.download.id) : 'started';
    if (progress === 'failed') {
      await settle(chrome.downloads.resume(item.id));
      notify(
        'downloads',
        'Kept in Chrome',
        `PopcornStash could not fetch ${result.name}, so Chrome is downloading it as usual.`,
      );
      return;
    }

    const [current] = await chrome.downloads.search({id: item.id});
    const outcome = chromeCopyOutcome(current?.state);
    if (outcome.cancel) {
      await settle(chrome.downloads.cancel(item.id));
      await settle(chrome.downloads.erase({id: item.id}));
    }
    notify('downloads', 'Sent to PopcornStash', result.name, (outcome.cancel ? result.note : null) ?? outcome.note);
  } finally {
    handling.delete(item.id);
  }
}

function listenToDownloads(): void {
  // chrome.downloads only exists once the optional "downloads" permission is granted.
  if (!chrome.downloads?.onCreated) return;
  if (chrome.downloads.onCreated.hasListener(handleCreated)) return;
  chrome.downloads.onCreated.addListener(handleCreated);
}

function handleCreated(item: chrome.downloads.DownloadItem): void {
  void onDownloadCreated(item);
}

listenToDownloads();
chrome.permissions.onAdded.addListener(listenToDownloads);

/**
 * Send one link to PopcornStash: the shared path behind the context menu,
 * the popup and download interception. Runs in the background service worker.
 */

import {addDownload, type Connection} from './api';
import {NOT_PAIRED, noHostAccess, toProblem, type Problem} from './errors';
import {checkLink, linkName} from './links';
import {buildDownloadPayload, cookiesAllowedFor, type CookieLike} from './payload';
import {displayHost} from './server-url';
import {forgetConnection, getConnection, getPrefs} from './storage';
import type {Download} from './types';

export interface SendRequest {
  url: string;
  /** Page the link or media was on (used for the Referer). */
  pageUrl?: string | undefined;
  filename?: string | undefined;
}

export type SendResult =
  | {ok: true; download: Download; name: string; note: string | null; server: string}
  | {ok: false; problem: Problem; server: string | null};

/** Does Chrome still let the extension reach the paired server? */
export async function hasHostAccess(server: string): Promise<boolean> {
  try {
    return await chrome.permissions.contains({origins: [`${server}/*`]});
  } catch {
    return false;
  }
}

/** Cookies for `url`, or null unless the option is on and Chrome granted cookie access for that site. */
async function cookiesFor(url: string, allow: boolean, server: string): Promise<CookieLike[] | null> {
  if (!cookiesAllowedFor(url, allow, server) || !chrome.cookies) return null;
  try {
    const origin = new URL(url).origin;
    const granted = await chrome.permissions.contains({permissions: ['cookies'], origins: [`${origin}/*`]});
    if (!granted) return null;
    const cookies = await chrome.cookies.getAll({url});
    return cookies.map(c => ({name: c.name, value: c.value}));
  } catch {
    return null;
  }
}

/** The connection, or the problem that stops the extension from using it. */
export async function usableConnection(): Promise<{connection: Connection} | {problem: Problem; server: string | null}> {
  const stored = await getConnection();
  if (!stored) return {problem: NOT_PAIRED, server: null};
  if (!(await hasHostAccess(stored.server))) {
    return {problem: noHostAccess(displayHost(stored.server)), server: stored.server};
  }
  return {connection: {server: stored.server, token: stored.token}};
}

/** After a 401 the token is useless: forget it and remember why. */
export async function handleProblem(problem: Problem, server: string | null): Promise<void> {
  if (problem.kind === 'token_revoked' && server) {
    await forgetConnection({kind: 'revoked', server, at: Date.now()});
  }
}

export async function sendToStash(request: SendRequest): Promise<SendResult> {
  const check = checkLink(request.url);
  if (!check.ok) {
    return {
      ok: false,
      problem: {kind: 'rejected', title: 'PopcornStash cannot take this link', message: check.reason, repair: null},
      server: null,
    };
  }

  const usable = await usableConnection();
  if ('problem' in usable) return {ok: false, problem: usable.problem, server: usable.server};
  const {connection} = usable;

  const prefs = await getPrefs();
  const magnet = check.kind === 'magnet';
  const cookies = magnet ? null : await cookiesFor(check.url, prefs.includeCookies, connection.server);
  const payload = buildDownloadPayload({
    url: check.url,
    filename: request.filename,
    pageUrl: request.pageUrl,
    userAgent: magnet ? null : navigator.userAgent,
    cookies,
    allowCookies: cookies !== null,
    serverOrigin: connection.server,
  });

  try {
    const download = await addDownload(connection, payload, magnet);
    const waitingForVpn = download.kind === 'torrent' && download.state === 'queued';
    return {
      ok: true,
      download,
      name: download.name || linkName(check.url),
      note: waitingForVpn ? (download.error ?? 'Waiting for the VPN to connect.') : null,
      server: connection.server,
    };
  } catch (error) {
    const problem = toProblem(error, displayHost(connection.server));
    await handleProblem(problem, connection.server);
    return {ok: false, problem, server: connection.server};
  }
}

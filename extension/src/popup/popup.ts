/**
 * Toolbar popup: connection and VPN state, "Paste a link or magnet", the five
 * most recent downloads, and a way into PopcornStash. Not paired: one call to
 * action that opens the options page.
 */

import {listDownloads, vpnStatus, type Connection} from '../lib/api';
import {byId, h, mount} from '../lib/dom';
import {downloadRow, percent} from '../lib/downloads-view';
import {toProblem, type Problem} from '../lib/errors';
import {checkLink} from '../lib/links';
import {sendFromPage} from '../lib/messages';
import {handleProblem, hasHostAccess} from '../lib/send';
import {displayHost} from '../lib/server-url';
import {getConnection, getNotice, type ConnectionNotice} from '../lib/storage';
import type {Download, VpnStatus} from '../lib/types';
import {describeVpn} from '../lib/vpn';

const RECENT_LIMIT = 5;
const DOWNLOADS_REFRESH_MS = 3_000;
const VPN_REFRESH_MS = 10_000;

const view = byId('view');
const hostLine = byId('server-host');
const optionsButton = byId<HTMLButtonElement>('open-options');

const timers: number[] = [];

function stopTimers(): void {
  while (timers.length > 0) window.clearInterval(timers.pop());
}

function openOptions(): void {
  void chrome.runtime.openOptionsPage();
  window.close();
}

function openTab(url: string): void {
  void chrome.tabs.create({url});
  window.close();
}

optionsButton.addEventListener('click', openOptions);

// ---------------------------------------------------------------------------
// Not connected
// ---------------------------------------------------------------------------

function renderUnpaired(notice: ConnectionNotice): void {
  stopTimers();
  hostLine.hidden = true;
  optionsButton.hidden = true;
  const revoked = notice?.kind === 'revoked';
  mount(
    view,
    h(
      'section',
      {class: 'intro', 'aria-labelledby': 'intro-title'},
      h('h1', {class: 'intro-title', id: 'intro-title'}, revoked ? 'This browser was disconnected' : 'Send links to your stash'),
      h(
        'p',
        {class: 'intro-text'},
        revoked
          ? `PopcornStash at ${displayHost(notice.server)} no longer accepts this browser. Pair it again to keep sending links.`
          : 'Connect this browser to your PopcornStash server once. Then send links, videos and magnet links from any page.',
      ),
      h(
        'button',
        {class: 'button button-primary button-block', type: 'button', onclick: openOptions},
        revoked ? 'Pair again' : 'Connect to PopcornStash',
      ),
    ),
  );
}

function renderNoAccess(server: string): void {
  stopTimers();
  hostLine.hidden = false;
  hostLine.textContent = displayHost(server);
  optionsButton.hidden = true;
  mount(
    view,
    h(
      'section',
      {class: 'intro', 'aria-labelledby': 'intro-title'},
      h('h1', {class: 'intro-title', id: 'intro-title'}, 'Allow access to your server'),
      h(
        'p',
        {class: 'intro-text'},
        `Chrome no longer lets this extension reach ${displayHost(server)}. Allow it again in the extension options.`,
      ),
      h('button', {class: 'button button-primary button-block', type: 'button', onclick: openOptions}, 'Open options'),
    ),
  );
}

// ---------------------------------------------------------------------------
// Connected
// ---------------------------------------------------------------------------

function vpnTag(status: VpnStatus | null, unavailable: boolean): HTMLElement {
  const summary = describeVpn(status, unavailable);
  const quiet = summary.tone === 'connected' || (summary.tone === 'neutral' && !unavailable);
  return h(
    'div',
    {class: 'vpn'},
    h(
      'span',
      summary.detail ? {class: 'tag', title: summary.detail} : {class: 'tag'},
      h('span', {class: `dot dot-${summary.tone}`, 'aria-hidden': 'true'}),
      h('span', {class: 'tag-label'}, summary.label),
    ),
    quiet ? null : h('p', {class: 'vpn-detail'}, summary.detail),
  );
}

function rowElement(download: Download): HTMLElement {
  const row = downloadRow(download);
  const bar =
    row.bar === null
      ? null
      : h(
          'div',
          {class: 'progress', 'aria-hidden': 'true'},
          h('div', {class: 'progress-fill'}),
        );
  const fill = bar?.firstElementChild as HTMLElement | null | undefined;
  if (fill && row.bar !== null) fill.style.transform = `scaleX(${row.bar / 100})`;
  return h(
    'li',
    {class: 'download', title: `${row.name}\n${row.description}`},
    h(
      'div',
      {class: 'download-line'},
      h('p', {class: 'download-name'}, row.name),
      h('p', {class: row.error ? 'download-meta download-meta-error' : 'download-meta', 'aria-hidden': 'true'}, row.meta),
    ),
    h('p', {class: 'visually-hidden'}, row.description),
    row.detail ? h('p', {class: 'download-detail', 'aria-hidden': 'true'}, row.detail) : null,
    bar,
  );
}

function skeletonRows(): HTMLElement {
  return h(
    'ul',
    {class: 'downloads', 'aria-hidden': 'true'},
    [0, 1, 2].map(() =>
      h('li', {class: 'download download-skeleton'}, h('span', {class: 'skeleton skeleton-name'}), h('span', {class: 'skeleton skeleton-state'})),
    ),
  );
}

function problemBlock(problem: Problem, retry: () => void): HTMLElement {
  return h(
    'div',
    {class: 'notice notice-error', role: 'alert'},
    h('p', {class: 'notice-title'}, problem.title),
    h('p', {class: 'notice-text'}, problem.message),
    h('button', {class: 'button button-secondary button-small', type: 'button', onclick: retry}, 'Try again'),
  );
}

function renderPaired(connection: Connection): void {
  stopTimers();
  hostLine.hidden = false;
  hostLine.textContent = displayHost(connection.server);
  optionsButton.hidden = false;

  const vpnSlot = h('div', {class: 'vpn-slot', 'aria-live': 'polite'}, vpnTag(null, false));

  const input = h('input', {
    id: 'link',
    class: 'input',
    type: 'text',
    inputmode: 'url',
    autocomplete: 'off',
    autocapitalize: 'off',
    spellcheck: 'false',
    placeholder: 'https://… or magnet:?…',
    'aria-describedby': 'send-message',
  });
  const sendButton = h('button', {class: 'button button-primary', type: 'submit'}, 'Send');
  const message = h('p', {class: 'field-message', id: 'send-message', role: 'status'});
  const form = h(
    'form',
    {class: 'send', novalidate: true},
    h('label', {class: 'label', for: 'link'}, 'Paste a link or magnet'),
    h('div', {class: 'send-row'}, input, sendButton),
    message,
  );

  const listSlot = h('div', {class: 'list-slot'}, skeletonRows());
  const recent = h(
    'section',
    {class: 'recent', 'aria-labelledby': 'recent-title'},
    h(
      'div',
      {class: 'section-head'},
      h('h2', {class: 'section-title', id: 'recent-title'}, 'Recent downloads'),
      h('button', {class: 'link-button', type: 'button', onclick: () => openTab(`${connection.server}/downloads`)}, 'All downloads'),
    ),
    listSlot,
  );

  const footer = h(
    'footer',
    {class: 'popup-footer'},
    h('button', {class: 'button button-secondary button-block', type: 'button', onclick: () => openTab(connection.server)}, 'Open PopcornStash'),
  );

  mount(view, vpnSlot, form, recent, footer);
  input.focus();

  let stopped = false;
  // Re-render only on change, so the live VPN line is not re-announced every refresh.
  let lastVpn = '';
  let lastList = '';
  const fail = async (problem: Problem): Promise<boolean> => {
    await handleProblem(problem, connection.server);
    if (problem.kind === 'token_revoked') {
      stopped = true;
      renderUnpaired(await getNotice());
      return true;
    }
    return false;
  };

  const loadDownloads = async (): Promise<void> => {
    try {
      const items = await listDownloads(connection, RECENT_LIMIT);
      if (stopped) return;
      const key = JSON.stringify(items.map(item => [item.id, item.name, item.state, percent(item), item.peers, item.error]));
      if (key === lastList) return;
      lastList = key;
      mount(
        listSlot,
        items.length === 0
          ? h('p', {class: 'empty'}, 'Nothing here yet. Links you send appear here with their progress.')
          : h('ul', {class: 'downloads'}, items.slice(0, RECENT_LIMIT).map(rowElement)),
      );
    } catch (error) {
      const problem = toProblem(error, displayHost(connection.server));
      if (await fail(problem)) return;
      const key = `problem:${problem.kind}`;
      if (key === lastList) return;
      lastList = key;
      mount(
        listSlot,
        problemBlock(problem, () => {
          lastList = '';
          mount(listSlot, skeletonRows());
          void loadDownloads();
        }),
      );
    }
  };

  const loadVpn = async (): Promise<void> => {
    try {
      const status = await vpnStatus(connection);
      const key = describeVpn(status).label;
      if (!stopped && key !== lastVpn) {
        lastVpn = key;
        mount(vpnSlot, vpnTag(status, false));
      }
    } catch (error) {
      const problem = toProblem(error, displayHost(connection.server));
      if (await fail(problem)) return;
      if (lastVpn !== 'unavailable') {
        lastVpn = 'unavailable';
        mount(vpnSlot, vpnTag(null, true));
      }
    }
  };

  form.addEventListener('submit', event => {
    event.preventDefault();
    void (async () => {
      const check = checkLink(input.value);
      if (!check.ok) {
        input.setAttribute('aria-invalid', 'true');
        message.className = 'field-message field-message-error';
        message.textContent = check.reason;
        input.focus();
        return;
      }
      input.removeAttribute('aria-invalid');
      sendButton.disabled = true;
      sendButton.textContent = 'Sending';
      input.readOnly = true;
      message.className = 'field-message';
      message.textContent = '';
      try {
        const result = await sendFromPage(check.url);
        if (result.ok) {
          input.value = '';
          message.className = 'field-message field-message-success';
          message.textContent = result.note ? `Sent: ${result.name}. ${result.note}` : `Sent: ${result.name}`;
          void loadDownloads();
        } else {
          if (await fail(result.problem)) return;
          if (result.problem.repair === 'grant') {
            renderNoAccess(connection.server);
            return;
          }
          input.setAttribute('aria-invalid', 'true');
          message.className = 'field-message field-message-error';
          message.textContent = `${result.problem.title}. ${result.problem.message}`;
        }
      } catch {
        message.className = 'field-message field-message-error';
        message.textContent = 'The extension could not send that. Close this popup and try again.';
      } finally {
        sendButton.disabled = false;
        sendButton.textContent = 'Send';
        input.readOnly = false;
        input.focus();
      }
    })();
  });

  input.addEventListener('input', () => {
    if (input.getAttribute('aria-invalid') === 'true') {
      input.removeAttribute('aria-invalid');
      message.className = 'field-message';
      message.textContent = '';
    }
  });

  void loadVpn();
  void loadDownloads();
  timers.push(
    window.setInterval(() => {
      if (!document.hidden) void loadDownloads();
    }, DOWNLOADS_REFRESH_MS),
    window.setInterval(() => {
      if (!document.hidden) void loadVpn();
    }, VPN_REFRESH_MS),
  );
}

async function start(): Promise<void> {
  const stored = await getConnection();
  if (!stored) {
    renderUnpaired(await getNotice());
    return;
  }
  if (!(await hasHostAccess(stored.server))) {
    renderNoAccess(stored.server);
    return;
  }
  renderPaired({server: stored.server, token: stored.token});
}

void start();

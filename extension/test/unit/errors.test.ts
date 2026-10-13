import {describe, expect, it} from 'vitest';
import {parseErrorBody, problemFromNetwork, problemFromResponse, toProblem, ProblemError, NOT_PAIRED} from '../../src/lib/errors';

const body = (code: string, message = '') => ({error: {code, message}});

describe('parseErrorBody', () => {
  it('reads the API error shape', () => {
    expect(parseErrorBody(body('not_found', 'Nothing here.'))).toEqual({code: 'not_found', message: 'Nothing here.'});
  });

  it('returns null for anything else', () => {
    expect(parseErrorBody(null)).toBeNull();
    expect(parseErrorBody('oops')).toBeNull();
    expect(parseErrorBody({error: 'nope'})).toBeNull();
    expect(parseErrorBody({error: {message: 'no code'}})).toBeNull();
    expect(parseErrorBody({status: 'pending'})).toBeNull();
  });
});

describe('problemFromResponse', () => {
  it('maps 401 to "pair again"', () => {
    const problem = problemFromResponse(401, body('unauthorized', 'Sign in again.'), {action: 'send'});
    expect(problem.kind).toBe('token_revoked');
    expect(problem.repair).toBe('pair');
    expect(problem.message).toMatch(/pair again/);
  });

  it('maps 401 without a body to "pair again" too', () => {
    expect(problemFromResponse(401, null, {action: 'list'}).kind).toBe('token_revoked');
  });

  it('explains that torrents need the VPN when a magnet is refused', () => {
    const problem = problemFromResponse(503, body('vpn_unavailable', 'Torrents are disabled.'), {action: 'send', magnet: true});
    expect(problem.kind).toBe('vpn_unavailable');
    expect(problem.title).toMatch(/Torrents are off/);
    expect(problem.message).toMatch(/only downloads torrents through its VPN/);
    expect(problem.message).toMatch(/Web links still work/);
  });

  it('keeps the server sentence for rejected links', () => {
    const problem = problemFromResponse(400, body('bad_request', 'That link is not valid.'), {action: 'send'});
    expect(problem).toEqual({
      kind: 'rejected',
      title: 'PopcornStash did not take that link',
      message: 'That link is not valid.',
      repair: null,
    });
  });

  it('maps rate limits, forbidden and setup', () => {
    expect(problemFromResponse(429, body('rate_limited', 'x'), {action: 'poll'}).kind).toBe('rate_limited');
    expect(problemFromResponse(403, body('forbidden', 'Owners only.'), {action: 'send'}).message).toBe('Owners only.');
    expect(problemFromResponse(409, body('setup_required', 'x'), {action: 'pair'}).kind).toBe('setup_required');
  });

  it('treats a 404 without the error shape as a different website', () => {
    const problem = problemFromResponse(404, null, {action: 'pair', host: 'example.com'});
    expect(problem.kind).toBe('not_popcornstash');
    expect(problem.message).toMatch(/example\.com/);
  });

  it('falls back to a server error for 5xx', () => {
    const problem = problemFromResponse(502, null, {action: 'list'});
    expect(problem.kind).toBe('server_error');
    expect(problem.message).toMatch(/Try again/);
    expect(problemFromResponse(500, body('internal', 'Disk full.'), {action: 'send'}).message).toBe('Disk full.');
  });
});

describe('network failures', () => {
  it('says the server cannot be reached', () => {
    const problem = problemFromNetwork(new TypeError('Failed to fetch'), 'stash.example.com');
    expect(problem.kind).toBe('unreachable');
    expect(problem.message).toMatch(/stash\.example\.com/);
  });

  it('says the server did not answer after a timeout', () => {
    const problem = problemFromNetwork(new DOMException('timed out', 'TimeoutError'), 'stash.example.com');
    expect(problem.title).toBe('PopcornStash did not answer');
  });

  it('unwraps ProblemError', () => {
    expect(toProblem(new ProblemError(NOT_PAIRED))).toBe(NOT_PAIRED);
    expect(toProblem(new Error('boom')).kind).toBe('unreachable');
  });
});

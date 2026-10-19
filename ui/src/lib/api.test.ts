import { describe, expect, it } from 'vitest';
import { request, sessionToken } from './api';

describe('sessionToken', () => {
  it('reads the token from the URL fragment', () => {
    expect(sessionToken('#9f8c7b')).toBe('9f8c7b');
  });

  it('returns an empty token when the fragment is absent', () => {
    expect(sessionToken('')).toBe('');
  });
});

describe('request', () => {
  it('posts the request body to the endpoint with the session token', async () => {
    let seen: { url: string; init?: RequestInit } | undefined;
    const fetchImpl: typeof fetch = async (input, init) => {
      seen = { url: input.toString(), init };
      return new Response('{"result":{}}', { status: 200 });
    };
    const response = await request({ op: 'capabilities' }, 'secret-token', fetchImpl);
    expect(seen?.url).toBe('/api');
    expect(seen?.init?.method).toBe('POST');
    expect(seen?.init?.headers).toMatchObject({
      'content-type': 'application/json',
      authorization: 'Bearer secret-token',
    });
    expect(seen?.init?.body).toBe('{"op":"capabilities"}');
    expect(response).toEqual({ result: {} });
  });

  it('throws when the endpoint refuses the request', async () => {
    const fetchImpl: typeof fetch = async () => new Response('refused', { status: 403 });
    await expect(request({ op: 'capabilities' }, 'wrong', fetchImpl)).rejects.toThrow('403');
  });
});

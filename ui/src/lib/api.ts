import type { ToolRequest, ToolResponse } from '../generated/wright-agent-v1';

// ADR-0032 decision 4: the host puts the per-session token in the URL
// fragment it opens. The fragment never reaches the server, so every
// endpoint call carries the token in the Authorization header instead.
export function sessionToken(hash: string = window.location.hash): string {
  return hash.startsWith('#') ? hash.slice(1) : hash;
}

// The endpoint is a plain adapter (ADR-0032 decision 3): the body is a
// wright-agent/v1 request and the response is the service's body unchanged.
export async function request(
  body: ToolRequest,
  token: string = sessionToken(),
  fetchImpl: typeof fetch = fetch,
): Promise<ToolResponse> {
  const response = await fetchImpl('/api', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`endpoint refused the request: HTTP ${response.status}`);
  }
  return (await response.json()) as ToolResponse;
}

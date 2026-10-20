import type { ToolRequest, ToolResponse } from './types/generated/wright-agent-v1'

/**
 * The session token rides the URL fragment, which browsers never send back
 * to the server (ADR-0032 decision 4); endpoint calls carry it as a bearer
 * token instead.
 */
export const sessionToken = location.hash.replace(/^#/, '')

/**
 * One `wright-agent/v1` request to the loopback endpoint. The endpoint
 * answers the `ToolService` response body unchanged — `{result: …}` or
 * `{error: {code, message}}`; a non-200 status means the request was refused
 * before reaching the service.
 */
export async function call(request: ToolRequest): Promise<ToolResponse> {
  const response = await fetch('/api', {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(request),
  })
  if (!response.ok) {
    throw new Error(`the UI endpoint refused the request (HTTP ${response.status})`)
  }
  return (await response.json()) as ToolResponse
}

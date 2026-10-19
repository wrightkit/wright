/**
 * Display names come from `lookup` by identity — the page never spells
 * catalog names itself (ADR-0032: the UI owns no Workshop semantics). Each
 * identity resolves once per session and the result is cached; `warm`
 * resolves a rule's identities in one pass so rendering stays synchronous.
 */
import type { LookupEntry, LookupResult, ToolRequest, ToolResponse } from './types/wright-agent-v1'

export interface ResolvedName {
  /** The session-locale display name. */
  name: string
  /** The `lookup` entry kind — drives the syntax class. */
  kind: string
}

export class Names {
  /** identity → the exact-identity entries `lookup` returned for it. */
  private readonly pending = new Map<string, Promise<LookupEntry[]>>()
  private readonly resolved = new Map<string, LookupEntry[]>()

  constructor(private readonly api: (request: ToolRequest) => Promise<ToolResponse>) {}

  /**
   * All `lookup` entries whose identity matches exactly — `[]` when the id
   * is not a catalog identity (the documented reserved call ids are not
   * catalog entries) or the endpoint is unreachable.
   */
  entries(identity: string): Promise<LookupEntry[]> {
    const cached = this.resolved.get(identity)
    if (cached) return Promise.resolve(cached)
    let pending = this.pending.get(identity)
    if (!pending) {
      pending = this.fetch(identity).then((entries) => {
        this.resolved.set(identity, entries)
        return entries
      })
      this.pending.set(identity, pending)
    }
    return pending
  }

  /** Prefetch identities in parallel; `peek` then answers synchronously. */
  async warm(identities: string[]): Promise<void> {
    await Promise.all([...new Set(identities)].map((identity) => this.entries(identity)))
  }

  /**
   * The best name for an identity out of the warmed cache, preferring the
   * given entry kinds; `null` when the identity did not resolve.
   */
  peek(identity: string, kinds: string[] = []): ResolvedName | null {
    const exact = this.resolved.get(identity) ?? []
    const entry =
      kinds.map((kind) => exact.find((entry) => entry.kind === kind)).find(Boolean) ?? exact[0]
    return entry ? { name: entry.displayName, kind: entry.kind } : null
  }

  private async fetch(identity: string): Promise<LookupEntry[]> {
    try {
      const response = await this.api({ op: 'lookup', language: 'workshop', query: identity })
      const entries = (response.result as LookupResult | undefined)?.entries
      return Array.isArray(entries) ? entries.filter((entry) => entry.identity === identity) : []
    } catch {
      return []
    }
  }
}

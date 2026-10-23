import { createSubscriber } from 'svelte/reactivity'
import type { LookupEntry, LookupResult, ToolRequest, ToolResponse } from './types/generated/wright-agent-v1'

export interface ResolvedName {
  name: string
  kind: string
}

export class Names {
  private readonly pending = new Map<string, Promise<LookupEntry[]>>()
  private readonly resolved = new Map<string, LookupEntry[]>()
  private locale = 'en-US'
  private resolveReady!: () => void
  private readonly ready = new Promise<void>((resolve) => {
    this.resolveReady = resolve
  })
  private notify: (() => void) | undefined
  private readonly subscribe = createSubscriber((update) => {
    this.notify = update
    return () => {
      if (this.notify === update) this.notify = undefined
    }
  })

  constructor(private readonly api: (request: ToolRequest) => Promise<ToolResponse>) {}

  entries(identity: string): Promise<LookupEntry[]> {
    const cached = this.resolved.get(identity)
    if (cached) return Promise.resolve(cached)
    let pending = this.pending.get(identity)
    if (!pending) {
      pending = this.fetch(identity)
        .then(({ locale, entries }) => {
          if (locale === this.locale) {
            this.resolved.set(identity, entries)
            this.notify?.()
          }
          return entries
        })
        .finally(() => {
          if (this.pending.get(identity) === pending) this.pending.delete(identity)
        })
      this.pending.set(identity, pending)
    }
    return pending
  }

  async warm(identities: string[]): Promise<void> {
    await Promise.all([...new Set(identities)].map((identity) => this.entries(identity)))
  }

  peek(identity: string, kinds: string[] = []): ResolvedName | null {
    this.subscribe()
    const exact = this.resolved.get(identity) ?? []
    const entry =
      kinds.map((kind) => exact.find((entry) => entry.kind === kind)).find(Boolean) ?? exact[0]
    return entry ? { name: entry.displayName, kind: entry.kind } : null
  }

  async setLocale(locale: string): Promise<void> {
    this.resolveReady()
    if (locale === this.locale) return
    this.locale = locale
    const identities = new Set([...this.resolved.keys(), ...this.pending.keys()])
    await Promise.all([...identities].map((identity) => this.rewrite(identity)))
  }

  private async rewrite(identity: string): Promise<void> {
    const { locale, entries } = await this.fetch(identity)
    if (locale === this.locale) {
      this.resolved.set(identity, entries)
      this.notify?.()
    }
  }

  private async fetch(identity: string): Promise<{ locale: string; entries: LookupEntry[] }> {
    await this.ready
    const locale = this.locale
    try {
      const response = await this.api({
        op: 'lookup',
        language: 'workshop',
        query: identity,
        locale,
      })
      const entries = (response.result as LookupResult | undefined)?.entries
      return {
        locale,
        entries: Array.isArray(entries)
          ? entries.filter((entry) => entry.identity === identity)
          : [],
      }
    } catch {
      return { locale, entries: [] }
    }
  }
}

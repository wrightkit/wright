/**
 * Display names come from `lookup` by identity — the page never spells
 * catalog names itself (ADR-0032: the UI owns no Workshop semantics). Each
 * identity resolves once per session locale and the result is cached;
 * `warm` resolves a rule's identities in one pass so rendering stays
 * synchronous. Every request carries the session locale; `setLocale`
 * re-resolves every identity already on screen without clearing the
 * cached names first, so a locale switch never flashes a raw id.
 */
import { createSubscriber } from 'svelte/reactivity'
import type { LookupEntry, LookupResult, ToolRequest, ToolResponse } from './types/generated/wright-agent-v1'

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
  /** The session locale every `lookup` request carries. */
  private locale = 'en-US'
  private resolveReady!: () => void
  /**
   * Settled once the session locale is known (`setLocale`'s first call):
   * lookups park on it so the first batch already carries `origin.locale`
   * rather than issuing in the en-US default and resolving again.
   */
  private readonly ready = new Promise<void>((resolve) => {
    this.resolveReady = resolve
  })
  /**
   * Re-runs the effects a `peek` call subscribed while they were active;
   * `undefined` while nothing subscribes. This is how a `setLocale`
   * re-resolve updates the names already on screen.
   */
  private notify: (() => void) | undefined
  private readonly subscribe = createSubscriber((update) => {
    this.notify = update
    return () => {
      if (this.notify === update) this.notify = undefined
    }
  })

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
      pending = this.fetch(identity)
        .then(({ locale, entries }) => {
          // A lookup issued under a superseded locale never lands;
          // `setLocale`'s re-resolve already covers the identity.
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

  /** Prefetch identities in parallel; `peek` then answers synchronously. */
  async warm(identities: string[]): Promise<void> {
    await Promise.all([...new Set(identities)].map((identity) => this.entries(identity)))
  }

  /**
   * The best name for an identity out of the warmed cache, preferring the
   * given entry kinds; `null` when the identity did not resolve. Reading
   * `peek` inside an effect subscribes it: a re-resolve landing re-runs
   * the effect so the new locale's name shows.
   */
  peek(identity: string, kinds: string[] = []): ResolvedName | null {
    this.subscribe()
    const exact = this.resolved.get(identity) ?? []
    const entry =
      kinds.map((kind) => exact.find((entry) => entry.kind === kind)).find(Boolean) ?? exact[0]
    return entry ? { name: entry.displayName, kind: entry.kind } : null
  }

  /**
   * The session locale: the project's `origin.locale` on load, then the
   * toolbar selector's value. The first call unblocks the parked lookups;
   * a later change re-resolves every identity already on screen. Cached
   * entries keep their names until the new lookups land, so a switch
   * updates each name in place and never flashes a raw identity.
   */
  async setLocale(locale: string): Promise<void> {
    this.resolveReady()
    if (locale === this.locale) return
    this.locale = locale
    const identities = new Set([...this.resolved.keys(), ...this.pending.keys()])
    await Promise.all([...identities].map((identity) => this.rewrite(identity)))
  }

  /** Fetch one identity under the current locale and replace its cache entry. */
  private async rewrite(identity: string): Promise<void> {
    const { locale, entries } = await this.fetch(identity)
    if (locale === this.locale) {
      this.resolved.set(identity, entries)
      this.notify?.()
    }
  }

  /**
   * One `lookup` for an identity in the session locale. The answer carries
   * the locale it was issued under so a response that outlives a locale
   * switch never overwrites the newer locale's entry.
   */
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

<script lang="ts">
  /**
   * One placement's severity mark (docs/ui/diagnostics.md § Severity, §
   * In place): the mark of the group's most severe finding — a filled
   * circle for error, a filled triangle for warning, an outlined circle
   * for info, each in its severity colour — and, when `count` is set
   * (the rule bar's form), the number of findings at that level beside
   * it. Hovering the mark opens a dark card listing the findings there:
   * severity, code, and message. The mark sits inside the rule bar's
   * button, so it is a labelled graphic — never a nested interactive
   * element; the card opens on CSS hover.
   */
  import type { Finding } from './types/generated/wright-agent-v1'
  import { mostSevere } from './findings'
  import type { ChromeStrings } from './chrome'

  let {
    findings,
    count = false,
    t,
  }: {
    /** The findings at this place; never empty — callers render the mark only with findings. */
    findings: Finding[]
    /** Show the count at the mark's level beside it (the rule bar's form). */
    count?: boolean
    /** The session locale's chrome strings. */
    t: ChromeStrings
  } = $props()

  const severity = $derived(mostSevere(findings))
  // The bar's count is of the most severe level, not the whole group.
  const atLevel = $derived(findings.filter((finding) => finding.severity === severity).length)
</script>

<span
  class="finding-mark severity-{severity}"
  role="img"
  aria-label={t.findingMarkLabel(severity, findings.length)}
>
  <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
    {#if severity === 'error'}
      <circle cx="5" cy="5" r="4" />
    {:else if severity === 'warning'}
      <path d="M5 1 L9.33 8.5 L0.67 8.5 Z" />
    {:else}
      <circle cx="5" cy="5" r="3.5" fill="none" stroke-width="1.8" />
    {/if}
  </svg>
  {#if count}<span class="finding-count">{atLevel}</span>{/if}
  <span class="finding-card">
    {#each findings as finding, i (i)}
      <span class="card-row severity-{finding.severity}">
        <span class="card-sev">{finding.severity}</span>
        <code class="card-code">{finding.code}</code>
        <span class="card-msg">{finding.message}</span>
      </span>
    {/each}
  </span>
</span>

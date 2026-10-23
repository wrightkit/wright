<script lang="ts">
  import type { Finding } from './types/generated/wright-agent-v1'
  import { mostSevere } from './findings'
  import SeverityMark from './SeverityMark.svelte'
  import type { ChromeStrings } from './chrome'

  let {
    findings,
    count = false,
    t,
  }: {
    findings: Finding[]
    count?: boolean
    t: ChromeStrings
  } = $props()

  const severity = $derived(mostSevere(findings))
  const atLevel = $derived(findings.filter((finding) => finding.severity === severity).length)
</script>

<span
  class="finding-mark severity-{severity}"
  role="img"
  aria-label={t.findingMarkLabel(severity, findings.length)}
>
  <SeverityMark {severity} />
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

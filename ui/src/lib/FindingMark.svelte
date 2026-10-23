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

<style>
  .finding-mark {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    cursor: default;
  }

  .finding-count {
    font-size: 0.8rem;
    font-weight: 600;
  }

  .finding-mark.severity-error .finding-count {
    color: var(--error);
  }

  .finding-mark.severity-warning .finding-count {
    color: var(--warn);
  }

  .finding-mark.severity-info .finding-count {
    color: var(--muted);
  }

  .finding-card {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 5;
    display: none;
    min-width: 16rem;
    max-width: 26rem;
    padding: 0.4rem 0;
    border-radius: 3px;
    background: var(--dlg);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.45);
    font-weight: 400;
    text-align: left;
    white-space: normal;
  }

  .finding-mark:hover .finding-card {
    display: block;
  }

  .finding-card .card-row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 0.1rem 0.6rem;
    padding: 0.3rem 0.7rem;
    font-size: 0.8rem;
  }

  .finding-card .card-sev {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .finding-card .card-row.severity-error .card-sev {
    color: var(--error);
  }

  .finding-card .card-row.severity-warning .card-sev {
    color: var(--warn);
  }

  .finding-card .card-row.severity-info .card-sev {
    color: var(--muted);
  }

  .finding-card .card-code {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 0.72rem;
    color: var(--dim);
  }

  .finding-card .card-msg {
    grid-column: 1 / -1;
    color: var(--text);
  }
</style>

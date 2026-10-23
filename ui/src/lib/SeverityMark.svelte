<script lang="ts">
  import type { Severity } from './findings'

  let { severity }: { severity: Severity } = $props()
</script>

<svg
  class="severity-mark severity-{severity}"
  viewBox="0 0 10 10"
  width="10"
  height="10"
  aria-hidden="true"
>
  {#if severity === 'error'}
    <circle cx="5" cy="5" r="4" />
  {:else if severity === 'warning'}
    <path d="M5 1 L9.33 8.5 L0.67 8.5 Z" />
  {:else}
    <circle cx="5" cy="5" r="3.5" fill="none" stroke-width="1.8" />
  {/if}
</svg>

<script lang="ts">
  /**
   * The argument hover card (docs/ui/editor.md § Hover card): a navy
   * panel with a gold top border that names the argument under the
   * pointer, the slot it fills in the enclosing call, and that call's
   * parameter list with the hovered parameter marked.
   *
   * The card is passive — `pointer-events: none`, no focus, no role.
   * It is not a tooltip in the ARIA sense and takes no interactive
   * primitive; ADR-0036's Bits UI registry covers interactive widgets,
   * which this is not (like the `finding-card`, a passive hover panel).
   * Positioning is fixed below the anchor's bounding box.
   */
  import type { ChromeStrings } from './chrome'
  import type { Names } from './names'
  import { argName, enclosingCall, nodeAtPath, statementRoot, hoverSlot } from './hover'
  import type { Node } from './types/generated/rule-content-v1'

  let {
    /** The statement node whose `args` the hovered path addresses. */
    statement,
    /** The argument's index path into the statement's `args`. */
    path,
    /** The hovered token's anchor box, in viewport coordinates. */
    anchor,
    names,
    t,
  }: {
    statement: Node
    path: number[]
    anchor: { left: number; bottom: number }
    names: Names
    t: ChromeStrings
  } = $props()

  const root = $derived(statementRoot(statement))
  const node = $derived(root !== null ? nodeAtPath(root.args, path) : undefined)
  const enclosing = $derived(root !== null ? enclosingCall(root, path) : null)
  const named = $derived(node !== undefined ? argName(node, enclosing, names) : null)
  const slot = $derived(enclosing !== null ? hoverSlot(enclosing, names, t) : null)
</script>

{#if named !== null && slot !== null}
  <div
    class="arg-card"
    role="presentation"
    style="left: {anchor.left}px; top: {anchor.bottom + 6}px;"
  >
    <div class="arg-card-head">
      <span class="arg-card-name">{named.name}</span>
      {#if named.kind}
        <span class="arg-card-kind"
          >{named.scope ? `${t.argScope(named.scope)} ${named.kind}` : named.kind}</span
        >
      {/if}
    </div>
    <div class="arg-card-slot">{slot.slot}</div>
    {#if slot.params.length > 0}
      <ol class="arg-card-params">
        {#each slot.params as param, i (i)}
          <li class:marked={param.marked}>{param.label}</li>
        {/each}
      </ol>
    {/if}
  </div>
{/if}

<style>
  /* § Hover card: navy (`--card`) with a gold top border, no pointer
     events, square corners at the spec's 3 px radius. */
  .arg-card {
    position: fixed;
    z-index: 6;
    min-width: 12rem;
    max-width: 24rem;
    padding: 0.55rem 0.8rem 0.65rem;
    border-top: 3px solid var(--gold);
    border-radius: 0 0 3px 3px;
    background: var(--card);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.45);
    color: var(--text);
    font-size: 0.8rem;
    pointer-events: none;
    white-space: normal;
  }

  .arg-card-head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  .arg-card-name {
    font-weight: 600;
  }

  .arg-card-kind {
    color: var(--dim);
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .arg-card-slot {
    color: var(--muted);
    margin-top: 0.15rem;
  }

  .arg-card-params {
    list-style: none;
    margin: 0.4rem 0 0;
    padding: 0.35rem 0 0;
    border-top: 1px solid rgb(255 255 255 / 0.12);
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.9rem;
    color: var(--muted);
  }

  .arg-card-params li.marked {
    color: var(--gold);
    font-weight: 600;
  }
</style>

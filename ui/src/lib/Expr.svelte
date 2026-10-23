<script lang="ts">
  /**
   * One value node rendered as syntax-coloured text (docs/ui/design.md
   * § Statement text): the call name then its arguments in parentheses,
   * comparisons and arithmetic infix. Every token takes its spec table
   * class (`tok <class>`), so parentheses, commas, member dots, and index
   * brackets are `punctuation`; every name comes from the warmed `Names`
   * cache (lookup by identity). Reserved call forms get their documented
   * structural render — a global variable reads `Global.name` with the
   * scope word from the `global` structural entry, a player variable
   * reads `owner.name` — and an unresolvable call id renders verbatim;
   * the page invents no names.
   *
   * `path` is the node's index path into the statement's `args`: each
   * argument token carries it on an `.arg` wrapper as `data-path`, so
   * the statement row's hover card can name the argument and its slot
   * (docs/ui/editor.md § Hover card). When the caller passes `onarg`
   * the wrapper also reports `data-arg-path` and calls `onarg(path,
   * event)` on click — the statement editor uses it to open the form
   * at that argument. The statement's own root carries no path — it
   * is not its own argument.
   */
  import Expr from './Expr.svelte'
  import { enumIdentity, kindClass, nodeClass, VALUE_KINDS } from './nodes'
  import type { Names } from './names'
  import type { Node, VariableNode, SubroutineNode } from './types/generated/rule-content-v1'

  let {
    node,
    names,
    kinds = VALUE_KINDS,
    onarg,
    path = [],
  }: {
    node: Node
    names: Names
    kinds?: string[]
    onarg?: (path: number[], event: MouseEvent) => void
    path?: number[]
  } = $props()

  function callName(call: string): { name: string; kind: string | undefined } {
    const resolved = names.peek(call, kinds)
    return resolved ? { name: resolved.name, kind: resolved.kind } : { name: call, kind: undefined }
  }
</script>

{#snippet punct(text: string)}
  <span class="tok punctuation">{text}</span>
{/snippet}

{#snippet arg(child: Node, childPath: number[])}
  {#if onarg}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <span
      class="arg"
      data-path={childPath.join('.')}
      data-arg-path={childPath.join('.')}
      onclick={(event) => {
        event.stopPropagation()
        onarg(childPath, event)
      }}
      ><Expr node={child} {names} {onarg} path={childPath} /></span
    >
  {:else}
    <span class="arg" data-path={childPath.join('.')}
      ><Expr node={child} {names} path={childPath} /></span
    >
  {/if}
{/snippet}

{#if typeof node === 'number'}
  <span class="tok number">{node}</span>
{:else if node === null || typeof node === 'boolean'}
  <span class="tok literal">{String(node)}</span>
{:else if 'string' in node}
  <span class="tok string">"{node.string}"</span>
{:else if 'localizedString' in node}
  {@const resolved = names.peek(node.localizedString)}
  <span class="tok string">{resolved ? resolved.name : node.localizedString}</span>
{:else if 'enum' in node}
  {@const resolved = names.peek(enumIdentity(node), ['enumMember'])}
  <span class="tok enum">{resolved ? resolved.name : `${node.enum}.${node.member}`}</span>
{:else if 'variable' in node}
  <span class="tok variable">{(node as VariableNode).variable}</span>
{:else if 'subroutine' in node}
  <span class="tok variable">{(node as SubroutineNode).subroutine}</span>
{:else if 'ambiguous' in node}
  <span class="tok enum">{node.ambiguous}</span>
{:else if node.call === 'globalVariable'}
  {@const scope = names.peek('global', ['structural'])}
  {@const label = (scope?.name ?? 'Global').replace(/^\p{Ll}/u, (c) => c.toUpperCase())}
  <span class="tok {kindClass(scope?.kind)}">{label}</span>{@render punct('.')}{@render arg(
    node.args[0],
    [...path, 0],
  )}
{:else if node.call === 'playerVariable'}
  {@render arg(node.args[0], [...path, 0])}{@render punct('.')}{@render arg(
    node.args[1],
    [...path, 1],
  )}
{:else if node.call === 'memberAccess'}
  {@render arg(node.args[0], [...path, 0])}{@render punct('.')}{#if typeof node.args[1] === 'object' && node.args[1] !== null && 'string' in node.args[1]}{#if onarg}<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions --><span
        class="arg"
        data-path={[...path, 1].join('.')}
        data-arg-path={[...path, 1].join('.')}
        onclick={(event) => {
          event.stopPropagation()
          onarg([...path, 1], event)
        }}><span class="tok variable">{node.args[1].string}</span></span>{:else}<span
        class="arg"
        data-path={[...path, 1].join('.')}
        ><span class="tok variable">{node.args[1].string}</span></span
      >{/if}{:else}{@render arg(node.args[1], [...path, 1])}{/if}{#if node.args.length > 2}{@render punct(
      '[',
    )}{@render arg(node.args[2], [...path, 2])}{@render punct(']')}{/if}
{:else}
  {@const head = callName(node.call)}
  {@const cls = nodeClass(node, head.kind)}
  {#if head.kind === 'operator' && node.args.length === 2}
    {@render arg(node.args[0], [...path, 0])}
    <span class="tok {cls}">{head.name}</span>
    {@render arg(node.args[1], [...path, 1])}
  {:else if node.args.length === 0}
    <span class="tok {cls}">{head.name}</span>
  {:else}
    <span class="tok {cls}">{head.name}</span>{@render punct('(')}{#each node.args as argNode, i (i)}{#if i > 0}{@render punct(', ')}{/if}{@render arg(
        argNode,
        [...path, i],
      )}{/each}{@render punct(')')}
  {/if}
{/if}

<style>
  /* The token classes colour the statement text (docs/ui/design.md §
     Statement text); the `--tok-*` values come from `app.css`. */
  .tok {
    white-space: pre-wrap;
  }

  .tok.action {
    color: var(--tok-action);
    font-weight: 700;
  }

  .tok.value {
    color: var(--tok-value);
  }

  .tok.enum {
    color: var(--tok-enum);
  }

  .tok.number {
    color: var(--tok-number);
  }

  .tok.string {
    color: var(--tok-string);
  }

  .tok.variable {
    color: var(--tok-variable);
  }

  .tok.operator {
    color: var(--tok-operator);
  }

  .tok.literal {
    color: var(--tok-literal);
  }

  .tok.punctuation {
    color: var(--tok-punctuation);
  }

  /* docs/ui/editor.md § Form/§ Hover card: an argument token carries its
     `data-path`; hovering outlines the argument a click would open — the
     same `--gold` inset ring the editor's argument outline uses. */
  .arg {
    border-radius: 2px;
  }

  .arg:global(.arg-hovered) {
    box-shadow: 0 0 0 1px var(--gold);
  }
</style>

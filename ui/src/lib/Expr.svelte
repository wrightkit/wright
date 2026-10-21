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
   */
  import Expr from './Expr.svelte'
  import { enumIdentity, kindClass, nodeClass, VALUE_KINDS } from './nodes'
  import type { Names } from './names'
  import type { Node, VariableNode, SubroutineNode } from './types/generated/rule-content-v1'

  let {
    node,
    names,
    kinds = VALUE_KINDS,
  }: { node: Node; names: Names; kinds?: string[] } = $props()

  function callName(call: string): { name: string; kind: string | undefined } {
    const resolved = names.peek(call, kinds)
    return resolved ? { name: resolved.name, kind: resolved.kind } : { name: call, kind: undefined }
  }
</script>

{#snippet punct(text: string)}
  <span class="tok punctuation">{text}</span>
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
  <!-- The scope label's canonical display name is the lowercase `global`
       keyword; the editor shows the capitalized form (`Global.score`). -->
  {@const label = (scope?.name ?? 'Global').replace(/^\p{Ll}/u, (c) => c.toUpperCase())}
  <span class="tok {kindClass(scope?.kind)}">{label}</span>{@render punct('.')}<Expr
    node={node.args[0]}
    {names}
  />
{:else if node.call === 'playerVariable'}
  <Expr node={node.args[0]} {names} />{@render punct('.')}<Expr node={node.args[1]} {names} />
{:else if node.call === 'memberAccess'}
  <Expr node={node.args[0]} {names} />{@render punct('.')}{#if typeof node.args[1] === 'object' && node.args[1] !== null && 'string' in node.args[1]}<span
      class="tok variable">{node.args[1].string}</span
    >{:else}<Expr node={node.args[1]} {names} />{/if}{#if node.args.length > 2}{@render punct(
      '[',
    )}<Expr node={node.args[2]} {names} />{@render punct(']')}{/if}
{:else}
  {@const head = callName(node.call)}
  {@const cls = nodeClass(node, head.kind)}
  {#if head.kind === 'operator' && node.args.length === 2}
    <Expr node={node.args[0]} {names} />
    <span class="tok {cls}">{head.name}</span>
    <Expr node={node.args[1]} {names} />
  {:else if node.args.length === 0}
    <span class="tok {cls}">{head.name}</span>
  {:else}
    <span class="tok {cls}">{head.name}</span>{@render punct('(')}{#each node.args as arg, i (i)}{#if i > 0}{@render punct(', ')}{/if}<Expr node={arg} {names} />{/each}{@render punct(')')}
  {/if}
{/if}

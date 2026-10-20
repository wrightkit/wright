<script lang="ts">
  /**
   * One value node rendered as syntax-coloured text. Every name comes from
   * the warmed `Names` cache (lookup by identity); reserved call forms get
   * their documented structural render, and an unresolvable call id renders
   * verbatim — the page invents no names.
   */
  import Expr from './Expr.svelte'
  import { enumIdentity, nodeClass, VALUE_KINDS } from './nodes'
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
{:else if node.call === 'globalVariable'}
  <Expr node={node.args[0]} {names} />
{:else if node.call === 'playerVariable'}
  <Expr node={node.args[0]} {names} />.<Expr node={node.args[1]} {names} />
{:else if node.call === 'memberAccess'}
  <Expr node={node.args[0]} {names} />.{#if typeof node.args[1] === 'object' && node.args[1] !== null && 'string' in node.args[1]}<span
        class="tok variable">{node.args[1].string}</span
      >{:else}<Expr node={node.args[1]} {names} />{/if}{#if node.args.length > 2}[<Expr
      node={node.args[2]}
      {names}
    />]{/if}
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
    <span class="tok {cls}">{head.name}</span>({#each node.args as arg, i (i)}{#if i > 0}{', '}{/if}<Expr node={arg} {names} />{/each})
  {/if}
{/if}

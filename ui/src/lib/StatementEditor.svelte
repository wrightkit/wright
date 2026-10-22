<script lang="ts">
  /**
   * The statement editor (ADR-0032 editing layer): opened by clicking a
   * statement row. The disable toggle and the argument form stage a
   * `replace` edit through the pending-changes model; a variable,
   * modify-operation, or enum argument's choices are the project's own
   * symbols and what `lookup`/`listEntries` declare — the dialog composes
   * no Workshop text.
   */
  import { call } from './api'
  import {
    type ArgField,
    argAt,
    argFields,
    currentId,
    enumNode,
    operationNode,
    paramFacts,
    variableNode,
    withArg,
  } from './args'
  import { closeDialog, showModal } from './dialog'
  import Expr from './Expr.svelte'
  import type { ChromeStrings } from './chrome'
  import type { Names } from './names'
  import { STATEMENT_KINDS } from './nodes'
  import Picker from './Picker.svelte'
  import type { StatementNode } from './edit'
  import type { Action, Node } from './types/generated/rule-content-v1'
  import type {
    ListEntriesResult,
    LookupEntry,
    RuleContentStatement,
    Symbol,
  } from './types/generated/wright-agent-v1'
  import type { SpannedAction, SpannedCondition } from './view'

  let {
    target,
    names,
    disabled,
    staged,
    t,
    locale,
    onsave,
    onclose,
  }: {
    target: { list: 'conditions' | 'actions'; index: number; node: StatementNode } | null
    names: Names
    /** The statement's effective disabled state, pending edits included. */
    disabled: boolean
    /** The pending `change` staged at this statement, when one exists. */
    staged?: RuleContentStatement
    /** The session locale's chrome strings. */
    t: ChromeStrings
    /** The session locale, for `listEntries`. */
    locale: string
    onsave: (disabled: boolean, args?: Node[]) => void
    onclose: () => void
  } = $props()

  let dialogEl: HTMLDialogElement | undefined = $state()
  let checked = $state(false)
  let shownFor: unknown = null
  /** The callable's `lookup` entry, fetched when an action opens. */
  let callableEntry = $state<LookupEntry | null>(null)
  /** The form's staged args, replacing the statement's on save. */
  let editedArgs = $state<Node[] | null>(null)
  /** `listEntries` results per enum domain the form touches. */
  let enumOptions = $state<Record<string, { id: string; label: string }[]>>({})
  /** The project's declared variables, for `variable` fields. */
  let variableSymbols = $state<Symbol[] | null>(null)
  /** In-flight option fetches, so a `fields` recompute can't duplicate them. */
  const enumLoading = new Set<string>()
  let symbolsLoading = false
  /** The locale the option caches below were loaded under. */
  let optionsLocale = ''

  const actionNode = $derived(
    target?.list === 'actions' ? (target.node as SpannedAction) : null,
  )
  const shownAction = $derived(
    actionNode && editedArgs ? { ...actionNode, args: editedArgs } : actionNode,
  )
  const fields = $derived(
    shownAction ? argFields(paramFacts(callableEntry), shownAction) : [],
  )

  $effect(() => {
    if (!dialogEl) return
    if (target !== null) {
      if (shownFor !== target) {
        checked = disabled
        shownFor = target
        callableEntry = null
        // A staged `change` seeds the form, so reopening shows it.
        editedArgs = Array.isArray(staged?.args) ? (staged.args as Node[]) : null
        enumOptions = {}
        variableSymbols = null
        enumLoading.clear()
        symbolsLoading = false
        optionsLocale = locale
        const callId = target.list === 'actions' ? (target.node as SpannedAction).call : null
        if (callId !== null) {
          void names.entries(callId).then((entries) => {
            if (shownFor === target) {
              callableEntry =
                entries.find((entry) => STATEMENT_KINDS.includes(entry.kind)) ??
                entries[0] ??
                null
            }
          })
        }
      }
      if (!dialogEl.open) showModal(dialogEl)
    } else {
      shownFor = null
      if (dialogEl.open) closeDialog(dialogEl)
    }
  })

  // The form's option sources load once the callable's facts arrive:
  // operation spellings warm the names cache, enum domains list their
  // members, and a variable field pulls the project's declared symbols.
  // A locale switch invalidates the caches so the next pass refetches.
  $effect(() => {
    if (fields.length === 0) return
    if (optionsLocale !== locale) {
      enumOptions = {}
      variableSymbols = null
      enumLoading.clear()
      symbolsLoading = false
      optionsLocale = locale
    }
    const operationIds = fields.flatMap((field) => field.operations ?? [])
    if (operationIds.length > 0) void names.warm(operationIds)
    const domains = new Set(
      fields.map((field) => field.domain).filter((domain): domain is string => !!domain),
    )
    for (const domain of domains) {
      if (enumOptions[domain] || enumLoading.has(domain)) continue
      enumLoading.add(domain)
      // Capture the locale the request issues under; a response that
      // lands after a switch is dropped, not installed.
      const requestLocale = locale
      void call({
        op: 'listEntries',
        language: 'workshop',
        kind: 'enumMember',
        within: domain,
        locale: requestLocale,
        pageSize: 500,
      })
        .then((response) => {
          if ('result' in response && requestLocale === optionsLocale) {
            const entries = (response.result as ListEntriesResult).entries ?? []
            enumOptions = {
              ...enumOptions,
              [domain]: entries.map((entry) => ({
                id: entry.identity,
                label: entry.displayName,
              })),
            }
          }
        })
        .catch(() => {})
        .finally(() => enumLoading.delete(domain))
    }
    if (
      fields.some((field) => field.kind === 'variable') &&
      variableSymbols === null &&
      !symbolsLoading
    ) {
      symbolsLoading = true
      void call({ op: 'symbols', all: true })
        .then((response) => {
          if ('result' in response) {
            const result = response.result
            variableSymbols = (Array.isArray(result)
              ? result
              : ((result as { symbols?: Symbol[] }).symbols ?? [])) as Symbol[]
          }
        })
        .catch(() => {})
        .finally(() => {
          symbolsLoading = false
        })
    }
  })

  const title = $derived.by(() => {
    if (!target) return ''
    if (target.list === 'actions') {
      const action = target.node as SpannedAction
      return names.peek(action.call, STATEMENT_KINDS)?.name ?? action.call
    }
    return t.conditionTitle
  })

  const exprNode = $derived(
    target === null
      ? undefined
      : target.list === 'conditions'
        ? (target.node as SpannedCondition).value
        : (shownAction as Node),
  )

  function optionsFor(field: ArgField): { id: string; label: string }[] {
    if (field.kind === 'variable') {
      return (variableSymbols ?? [])
        .filter((symbol) => symbol.kind === field.scope)
        .map((symbol) => ({ id: symbol.name, label: symbol.name }))
    }
    if (field.kind === 'operation') {
      return (field.operations ?? []).map((id) => ({
        id,
        label: names.peek(id, ['operator'])?.name ?? id,
      }))
    }
    return enumOptions[field.domain ?? ''] ?? []
  }

  function choose(field: ArgField, id: string) {
    if (!shownAction) return
    const node =
      field.kind === 'variable'
        ? variableNode(id)
        : field.kind === 'operation'
          ? operationNode(id)
          : enumNode(id)
    editedArgs = withArg(shownAction, field.path, node).args
  }
</script>

<dialog bind:this={dialogEl} onclose={onclose} class="dialog editor">
  {#if target}
    <h2 class="editor-title">{title}</h2>
    <label class="editor-disable">
      <input type="checkbox" bind:checked={checked} data-testid="editor-disable" />
      {t.disabled}
    </label>
    <div class="editor-statement">
      {#if exprNode !== undefined}
        <Expr
          node={exprNode}
          {names}
          kinds={target.list === 'actions' ? STATEMENT_KINDS : undefined}
        />
      {/if}
    </div>
    {#if fields.length > 0}
      <div class="editor-args">
        {#each fields as field (field.label + field.path.join('.'))}
          {@const node = shownAction ? argAt(shownAction.args, field.path) : undefined}
          {#if field.kind !== 'other' || node !== undefined}
            <div class="editor-arg">
            <span class="editor-arg-label">{field.label}</span>
            {#if field.kind === 'other'}
              <span class="editor-arg-value">
                {#if node !== undefined}<Expr {node} {names} />{/if}
              </span>
            {:else}
              <Picker
                options={optionsFor(field)}
                selected={currentId(node)}
                placeholder={field.label}
                ariaLabel={field.label}
                onselect={(id) => choose(field, id)}
              />
            {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
    <div class="editor-foot">
      <button type="button" class="cancel" onclick={() => dialogEl && closeDialog(dialogEl)}
        >{t.cancel}</button
      >
      <button
        type="button"
        class="ok"
        onclick={() => {
          onsave(checked, editedArgs ?? undefined)
          if (dialogEl) closeDialog(dialogEl)
        }}>{t.ok}</button
      >
    </div>
  {/if}
</dialog>

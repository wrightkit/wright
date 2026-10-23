<script lang="ts">
  import { call } from './api'
  import {
    type ArgField,
    argAt,
    argFields,
    conditionFields,
    currentId,
    defaultArgs,
    enumNode,
    fillGaps,
    operationNode,
    paramFacts,
    slotSet,
    stringNode,
    unsetRequired,
    variableNode,
    withArg,
    type ParamFact,
  } from './args'
  import Dialog from './components/Dialog.svelte'
  import type { ChromeStrings } from './chrome'
  import type { Names } from './names'
  import { statementText, STATEMENT_KINDS, VALUE_KINDS } from './nodes'
  import Picker from './components/Picker.svelte'
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
    focusPath = null,
    isInsert = false,
    onsave,
    onclose,
  }: {
    target: { list: 'conditions' | 'actions'; index: number; node: StatementNode } | null
    names: Names
    disabled: boolean
    staged?: RuleContentStatement
    t: ChromeStrings
    locale: string
    focusPath?: number[] | null
    isInsert?: boolean
    onsave: (disabled: boolean, args?: Node[], value?: Node, call?: string) => void
    onclose: () => void
  } = $props()

  let checked = $state(false)
  let shownFor: unknown = null
  let callableEntry = $state<LookupEntry | null>(null)
  let valueEntry = $state<LookupEntry | null>(null)
  let editedArgs = $state<Node[] | null>(null)
  let enumOptions = $state<Record<string, { id: string; label: string }[]>>({})
  let variableSymbols = $state<Symbol[] | null>(null)
  let nestedEntries = $state<Record<string, LookupEntry | null>>({})
  const enumLoading = new Set<string>()
  const nestedLoading = new Set<string>()
  let symbolsLoading = false
  let actionOptions = $state<{ id: string; label: string }[] | null>(null)
  let chosenCall = $state<string | null>(null)
  let actionsLoading = false
  let optionsLocale = ''
  let root: HTMLElement | undefined = $state()

  const isCondition = $derived(target?.list === 'conditions')

  const sourceAction = $derived.by((): Action | null => {
    if (target === null) return null
    if (!isCondition) {
      const action = target.node as SpannedAction
      return chosenCall !== null ? { ...action, call: chosenCall } : action
    }
    const value = (target.node as SpannedCondition).value
    if (value !== null && typeof value === 'object' && 'call' in value) {
      return { call: value.call as string, args: (value.args as Node[]) ?? [] }
    }
    return { call: '', args: value === undefined ? [] : [value] }
  })
  const shownAction = $derived(
    sourceAction && editedArgs ? { ...sourceAction, args: editedArgs } : sourceAction,
  )

  const nestedFactsFor = (callId: string): ParamFact[] => paramFacts(nestedEntries[callId])
  const nestedFacts = (callId: string): ParamFact[] | null =>
    nestedEntries[callId] === undefined ? null : nestedFactsFor(callId)

  const fields = $derived.by((): ArgField[] => {
    if (shownAction === null) return []
    if (isCondition) {
      const value =
        (shownAction.call as string) !== ''
          ? ({ call: shownAction.call, args: shownAction.args } as Node)
          : (shownAction.args[0] ?? (target as { node: SpannedCondition }).node.value)
      return conditionFields(
        paramFacts(valueEntry),
        paramFacts(callableEntry),
        value,
        nestedFacts,
      )
    }
    return argFields(paramFacts(callableEntry), shownAction, nestedFacts)
  })
  const missing = $derived(shownAction ? unsetRequired(shownAction, fields) : [])
  const flatFields = (list: ArgField[]): ArgField[] =>
    list.flatMap((field) => (field.nested ? [field, ...flatFields(field.nested)] : [field]))
  const canSave = $derived(
    missing.length === 0 &&
      (!isInsert ||
        (flatFields(fields).every((field) => field.insertable !== false) &&
          (isCondition
            ? editedArgs !== null &&
              editedArgs[0] !== null &&
              editedArgs[0] !== undefined
            : chosenCall !== null && callableEntry !== null))),
  )

  $effect(() => {
    if (target === null) {
      shownFor = null
      return
    }
    if (shownFor === target) return
    checked = disabled
    shownFor = target
    focusedFor = null
    callableEntry = null
    valueEntry = null
    actionOptions = null
    chosenCall = null
    actionsLoading = false
    const stagedValue = staged?.value as Node | undefined
    const seed =
      target.list === 'actions'
        ? Array.isArray(staged?.args)
          ? (staged.args as Node[])
          : null
        : stagedValue === undefined
          ? null
          : typeof stagedValue === 'object' &&
              stagedValue !== null &&
              'call' in stagedValue &&
              Array.isArray((stagedValue as { args?: Node[] }).args)
            ? ((stagedValue as { args: Node[] }).args ?? null)
            : [stagedValue]
    editedArgs = seed
    enumOptions = {}
    variableSymbols = null
    nestedEntries = {}
    enumLoading.clear()
    nestedLoading.clear()
    symbolsLoading = false
    optionsLocale = locale
    if (isCondition) {
      const value = (target.node as SpannedCondition).value
      const ids = [
        'compare',
        typeof value === 'object' && value !== null && 'call' in value
          ? (value.call as string)
          : null,
      ].filter((id): id is string => id !== null)
      void names.entries('compare').then((entries) => {
        if (shownFor === target) {
          callableEntry =
            entries.find((entry) => VALUE_KINDS.includes(entry.kind)) ?? entries[0] ?? null
        }
      })
      const valueCall = ids[1]
      if (valueCall) {
        void names.entries(valueCall).then((entries) => {
          if (shownFor === target) {
            valueEntry =
              entries.find((entry) => VALUE_KINDS.includes(entry.kind)) ?? entries[0] ?? null
          }
        })
      }
    } else {
      const callId = (target.node as SpannedAction).call
      if (callId !== '') {
        void names.entries(callId).then((entries) => {
          if (shownFor === target) {
            callableEntry =
              entries.find((entry) => STATEMENT_KINDS.includes(entry.kind)) ?? entries[0] ?? null
          }
        })
      }
    }
  })

  let focusedFor: unknown = null
  $effect(() => {
    if (target === null || focusPath === null || fields.length === 0) return
    if (focusedFor === target) return
    const el = root?.querySelector<HTMLElement>(`[data-arg-control="${focusPath.join('.')}"]`)
    if (!el) return
    focusedFor = target
    el.scrollIntoView?.({ block: 'nearest' })
    ;(el.querySelector<HTMLElement>('input,button') ?? el).focus()
  })

  $effect(() => {
    if (fields.length === 0) return
    if (optionsLocale !== locale) {
      enumOptions = {}
      variableSymbols = null
      nestedEntries = {}
      enumLoading.clear()
      nestedLoading.clear()
      symbolsLoading = false
      optionsLocale = locale
    }
    const flat = flatFields(fields)
    const operationIds = flat.flatMap((field) => field.operations ?? [])
    if (operationIds.length > 0) void names.warm(operationIds)
    const domains = new Set(
      flat.map((field) => field.domain).filter((domain): domain is string => !!domain),
    )
    for (const domain of domains) {
      if (enumOptions[domain] || enumLoading.has(domain)) continue
      enumLoading.add(domain)
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
      flat.some((field) => field.kind === 'variable') &&
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
    for (const field of flat) {
      if (field.kind !== 'call' || !shownAction) continue
      const node =
        field.path.length > 0 ? argAt(shownAction.args, field.path) : undefined
      const callId =
        node !== undefined && node !== null && typeof node === 'object' && 'call' in node
          ? (node.call as string)
          : (field.call ?? null)
      if (callId === null || callId in nestedEntries || nestedLoading.has(callId)) continue
      nestedLoading.add(callId)
      const requestLocale = locale
      void names
        .entries(callId)
        .then((entries) => {
          if (requestLocale !== optionsLocale) return
          const entry =
            entries.find((item) => VALUE_KINDS.includes(item.kind)) ?? entries[0] ?? null
          nestedEntries = { ...nestedEntries, [callId]: entry }
        })
        .catch(() => {})
        .finally(() => nestedLoading.delete(callId))
    }
  })

  const title = $derived.by(() => {
    if (!target) return ''
    if (isInsert) return t.addStatement(isCondition ? t.conditionNoun : t.actionNoun)
    if (!isCondition) {
      const action = target.node as SpannedAction
      return names.peek(action.call, STATEMENT_KINDS)?.name ?? action.call
    }
    return t.conditionTitle
  })

  $effect(() => {
    if (!isInsert || isCondition || target === null) return
    if (actionOptions !== null || actionsLoading) return
    actionsLoading = true
    const requestLocale = locale
    void call({
      op: 'listEntries',
      language: 'workshop',
      kind: 'action',
      locale: requestLocale,
      pageSize: 500,
    })
      .then((response) => {
        if ('result' in response && requestLocale === optionsLocale) {
          const entries = (response.result as ListEntriesResult).entries ?? []
          actionOptions = entries.map((entry) => ({
            id: entry.identity,
            label: entry.displayName,
          }))
        }
      })
      .catch(() => {})
      .finally(() => {
        actionsLoading = false
      })
  })

  function chooseAction(id: string) {
    if (!isInsert || isCondition || !target) return
    callableEntry = null
    chosenCall = id
    editedArgs = []
    void names.entries(id).then((entries) => {
      if (shownFor !== target) return
      callableEntry =
        entries.find((entry) => STATEMENT_KINDS.includes(entry.kind)) ?? entries[0] ?? null
      editedArgs = defaultArgs(paramFacts(callableEntry))
    })
  }

  function optionsFor(field: ArgField): { id: string; label: string }[] {
    if (field.kind === 'boolean') {
      return [
        { id: 'true', label: t.trueLiteral },
        { id: 'false', label: t.falseLiteral },
      ]
    }
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

  function callLabel(node: Node | undefined, field: ArgField): string {
    const subject = node ?? field.node
    const callId =
      subject !== undefined &&
      subject !== null &&
      typeof subject === 'object' &&
      'call' in subject
        ? (subject.call as string)
        : field.call
    if (callId === undefined) return ''
    if (
      subject !== undefined &&
      subject !== null &&
      typeof subject === 'object' &&
      'call' in subject &&
      ['globalVariable', 'playerVariable', 'memberAccess'].includes(subject.call as string)
    ) {
      return statementText(subject, names, VALUE_KINDS)
    }
    return names.peek(callId, VALUE_KINDS)?.name ?? callId
  }

  function choose(field: ArgField, id: string) {
    if (!shownAction) return
    const node =
      field.kind === 'boolean'
        ? id === 'true'
        : field.kind === 'variable'
          ? variableNode(id)
          : field.kind === 'operation'
            ? operationNode(id)
            : enumNode(id)
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, node).args
  }

  function setLiteral(field: ArgField, text: string) {
    if (!shownAction || text === '') return
    const node =
      field.kind === 'number'
        ? Number(text)
        : field.kind === 'other'
          ? /^-?\d+(\.\d+)?$/.test(text)
            ? Number(text)
            : stringNode(text)
          : stringNode(text)
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, node).args
  }
</script>

<Dialog open={target !== null} {onclose} class="editor" {title} titleClass="editor-title">
  {#if target}
    <div class="editor-root" bind:this={root}>
      {#if !isInsert}
        <label class="editor-disable">
          <input type="checkbox" bind:checked={checked} data-testid="editor-disable" />
          {t.disabled}
        </label>
      {/if}
      {#if isInsert && !isCondition}
        <!-- § Form · First row: the Action picker over every catalog
             action; choosing one seeds the parameter rows from
             `lookup` defaults. -->
        <div class="editor-arg" data-arg-control="action">
          <span class="editor-arg-label">{t.actionLabel}</span>
          <Picker
            options={actionOptions ?? []}
            selected={chosenCall}
            placeholder={t.unset}
            ariaLabel={t.actionNoun}
            onselect={chooseAction}
          />
        </div>
      {/if}
      {#if fields.length > 0}
        <div class="editor-args">
          {@render rows(fields)}
        </div>
      {/if}
      <div class="editor-foot">
        <button type="button" class="cancel" onclick={onclose}>{t.cancel}</button>
        <button
          type="button"
          class="ok"
          disabled={!canSave}
          onclick={() => {
            if (isCondition) {
              const call =
                typeof sourceAction?.call === 'string' && sourceAction.call !== ''
                  ? sourceAction.call
                  : null
              const value =
                call !== null && shownAction
                  ? ({ call, args: shownAction.args } as Node)
                  : (shownAction?.args?.[0] as Node | undefined)
              onsave(checked, undefined, value)
            } else {
              onsave(checked, editedArgs ?? undefined, undefined, chosenCall ?? undefined)
            }
            onclose()
          }}>{t.ok}</button
        >
      </div>
    </div>
  {/if}
</Dialog>

{#snippet rows(list: ArgField[])}
  {#each list as field (field.label + field.path.join('.'))}
    {@const node =
      shownAction && field.path.length > 0
        ? argAt(shownAction.args, field.path)
        : undefined}
    {@const unset = field.required && !slotSet(shownAction ?? { call: '', args: [] }, field)}
    <div class="editor-arg" class:unset data-arg-control={field.path.join('.')}>
      <span class="editor-arg-label">{field.label}</span>
      {#if field.kind === 'number'}
        <input
          class="editor-input"
          type="number"
          step="any"
          value={typeof node === 'number' ? node : ''}
          placeholder={unset ? t.unset : ''}
          aria-label={field.label}
          oninput={(event) => setLiteral(field, event.currentTarget.value)}
        />
      {:else if field.kind === 'string'}
        <input
          class="editor-input"
          type="text"
          value={node !== undefined && node !== null && typeof node === 'object' && 'string' in node
            ? (node.string as string)
            : ''}
          placeholder={unset ? t.unset : ''}
          aria-label={field.label}
          oninput={(event) => setLiteral(field, event.currentTarget.value)}
        />
      {:else if field.kind === 'call'}
        <span class="editor-arg-value">{callLabel(node, field)}</span>
      {:else if field.kind === 'other' && isInsert}
        <!-- An untyped slot on the `+` form (`Any` and friends) takes a
             literal text input until #919's value picker lands: a
             numeric spelling writes the number literal, anything else
             a `{"string"}` node — both `rule-content-v1` nodes
             `editStatements` validates, never Workshop text. -->
        <input
          class="editor-input"
          type="text"
          value={typeof node === 'number'
            ? String(node)
            : node !== undefined && node !== null && typeof node === 'object' && 'string' in node
              ? (node.string as string)
              : ''}
          placeholder={unset ? t.unset : ''}
          aria-label={field.label}
          oninput={(event) => setLiteral(field, event.currentTarget.value)}
        />
      {:else if field.kind === 'other'}
        <span class="editor-arg-value" class:warn={unset}>
          {unset ? t.unset : node !== undefined ? statementText(node, names, VALUE_KINDS) : ''}
        </span>
      {:else}
        <Picker
          options={optionsFor(field)}
          selected={currentId(node)}
          placeholder={unset ? t.unset : field.label}
          ariaLabel={field.label}
          onselect={(id) => choose(field, id)}
        />
      {/if}
    </div>
    {#if field.nested}
      <div class="editor-nested">
        {@render rows(field.nested)}
      </div>
    {/if}
  {/each}
{/snippet}

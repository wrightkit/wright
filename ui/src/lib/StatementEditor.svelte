<script lang="ts">
  import { call } from './api'
  import {
    type ArgField,
    admitsCall,
    argAt,
    argFields,
    callNode,
    conditionFields,
    currentId,
    defaultArgs,
    enumNode,
    fillGaps,
    operationNode,
    paramFacts,
    slotSet,
    stringNode,
    subroutineNode,
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
    used,
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
    used?: ReadonlySet<string>
    focusPath?: number[] | null
    /** Whether the form stages a `+` insert instead of a `replace`. */
    isInsert?: boolean
    onsave: (disabled: boolean, args?: Node[], value?: Node, call?: string) => void
    onclose: () => void
  } = $props()

  let checked = $state(false)
  let shownFor: unknown = null
  let callableEntry = $state<LookupEntry | null>(null)
  let valueEntry = $state<LookupEntry | null>(null)
  let editedArgs = $state<Node[] | null>(null)
  let editedCall = $state<string | null>(null)
  let enumOptions = $state<Record<string, { id: string; label: string }[]>>({})
  let variableSymbols = $state<Symbol[] | null>(null)
  let actionEntries = $state<LookupEntry[] | null>(null)
  let valueEntries = $state<LookupEntry[] | null>(null)
  let nestedEntries = $state<Record<string, LookupEntry | null>>({})
  const enumLoading = new Set<string>()
  const nestedLoading = new Set<string>()
  let symbolsLoading = false
  let actionLoading = false
  let valueLoading = false
  let optionsLocale = ''
  let root: HTMLElement | undefined = $state()
  /** The `+` form's picked call (an insert action). */
  let chosenCall = $state<string | null>(null)
  /** The `+` form's declared subroutines, for the `Subroutine` slot. */
  let subroutineSymbols = $state<Symbol[] | null>(null)
  let subroutinesLoading = false

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
    sourceAction
      ? {
          ...sourceAction,
          call: editedCall ?? sourceAction.call,
          args: editedArgs ?? sourceAction.args,
        }
      : sourceAction,
  )

  const listedEntry = (callId: string): LookupEntry | undefined =>
    (actionEntries ?? []).find((entry) => entry.identity === callId) ??
    (valueEntries ?? []).find((entry) => entry.identity === callId)

  const entryFor = (callId: string): LookupEntry | null =>
    listedEntry(callId) ?? nestedEntries[callId] ?? null

  const nestedFacts = (callId: string): ParamFact[] | null => {
    const listed = listedEntry(callId)
    const entry = listed ?? (callId in nestedEntries ? nestedEntries[callId] : undefined)
    return entry === undefined ? null : paramFacts(entry)
  }

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
    return argFields(
      paramFacts(callableEntry),
      shownAction,
      nestedFacts,
      subroutineSymbols !== null && subroutineSymbols.length > 0,
    )
  })
  const missing = $derived(shownAction ? unsetRequired(shownAction, fields) : [])
  const flatFields = (list: ArgField[]): ArgField[] =>
    list.flatMap((field) => (field.nested ? [field, ...flatFields(field.nested)] : [field]))
  /**
   * OK stays disabled while a slot is unset (`missing`) or holds a
   * position only #919's pickers can fill (`insertable === false`) — a
   * staged insert must submit a node `editStatements` accepts, and the
   * `+` form's controls cannot write a `{"variable"}`, `{"subroutine"}`,
   * or modify-operation shape the slot requires.
   */
  const canSave = $derived(
    missing.length === 0 &&
      (!isInsert ||
        (flatFields(fields).every((field) => field.insertable !== false) &&
          (isCondition
            ? editedArgs !== null && editedArgs[0] !== null && editedArgs[0] !== undefined
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
    chosenCall = null
    subroutineSymbols = null
    subroutinesLoading = false
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
    const stagedCall =
      target.list === 'actions' && typeof staged?.call === 'string'
        ? staged.call
        : typeof stagedValue === 'object' &&
            stagedValue !== null &&
            'call' in stagedValue &&
            typeof stagedValue.call === 'string'
          ? stagedValue.call
          : null
    editedCall = stagedCall !== null && stagedCall !== sourceAction?.call ? stagedCall : null
    enumOptions = {}
    variableSymbols = null
    nestedEntries = {}
    actionEntries = null
    valueEntries = null
    enumLoading.clear()
    nestedLoading.clear()
    symbolsLoading = false
    actionLoading = false
    valueLoading = false
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
    if (target === null) return
    if (optionsLocale !== locale) {
      enumOptions = {}
      variableSymbols = null
      nestedEntries = {}
      actionEntries = null
      valueEntries = null
      enumLoading.clear()
      nestedLoading.clear()
      symbolsLoading = false
      actionLoading = false
      valueLoading = false
      optionsLocale = locale
    }
    if (!isCondition && actionEntries === null && !actionLoading) {
      actionLoading = true
      const requestLocale = locale
      // The Action row lists the `action` catalog plus the `structural`
      // calls `listEntries` serves — `setGlobalVariable`,
      // `setPlayerVariable`, `startRule` and friends are callable
      // statement calls even though the owner kinds them `structural`,
      // so the "all actions" picker includes them (issue #919).
      void Promise.all([listAll('action', requestLocale), listAll('structural', requestLocale)])
        .then(([actions, structural]) => {
          if (requestLocale === optionsLocale) actionEntries = [...actions, ...structural]
          else actionEntries = []
        })
        .finally(() => {
          actionLoading = false
        })
    }
    const allFields = (list: ArgField[]): ArgField[] =>
      list.flatMap((field) => (field.nested ? [field, ...allFields(field.nested)] : [field]))
    const flat = allFields(fields)
    if (
      valueEntries === null &&
      !valueLoading &&
      flat.some((field) => field.kind === 'call' || field.kind === 'comparison')
    ) {
      valueLoading = true
      const requestLocale = locale
      void listAll('value', requestLocale)
        .then((entries) => {
          if (requestLocale === optionsLocale) valueEntries = entries
          else valueEntries = []
        })
        .finally(() => {
          valueLoading = false
        })
    }
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
      if (
        callId === null ||
        callId in nestedEntries ||
        nestedLoading.has(callId) ||
        listedEntry(callId) !== undefined
      )
        continue
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
    const callId = shownAction?.call
    if (!isCondition && typeof callId === 'string' && callId !== '') {
      return names.peek(callId, STATEMENT_KINDS)?.name ?? callId
    }
    return t.conditionTitle
  })

  async function listAll(
    kind: 'action' | 'value' | 'structural',
    locale: string,
  ): Promise<LookupEntry[]> {
    const out: LookupEntry[] = []
    let cursor: string | null = null
    do {
      try {
        const response = await call({
          op: 'listEntries',
          language: 'workshop',
          kind,
          locale,
          pageSize: 500,
          cursor,
        })
        if (!('result' in response)) break
        const result = response.result as ListEntriesResult
        out.push(...(result.entries ?? []))
        cursor = result.nextCursor ?? null
      } catch {
        break
      }
    } while (cursor !== null)
    return out
  }

  function usageOrder(options: { id: string; label: string }[]): {
    id: string
    label: string
  }[] {
    if (used === undefined || used.size === 0) return options
    const first = options.filter((option) => used.has(option.id))
    return first.length === 0 ? options : [...first, ...options.filter((o) => !used.has(o.id))]
  }

  function optionsFor(field: ArgField, node: Node | undefined): { id: string; label: string }[] {
    if (field.kind === 'boolean') {
      return [
        { id: 'true', label: t.trueLiteral },
        { id: 'false', label: t.falseLiteral },
      ]
    }
    if (field.kind === 'variable') {
      return usageOrder(
        (variableSymbols ?? [])
          .filter((symbol) => symbol.kind === field.scope)
          .map((symbol) => ({ id: symbol.name, label: symbol.name })),
      )
    }
    if (field.kind === 'operation' || field.kind === 'comparison') {
      return usageOrder(
        (field.operations ?? []).map((id) => ({
          id,
          label: names.peek(id, ['operator'])?.name ?? id,
        })),
      )
    }
    if (field.kind === 'call') {
      // `rule-content-v1` has no `{call:"compare"}` — the surface
      // `compare` value exists only as the operator-call form the
      // condition's own rows already decompose, so the picker drops it
      // rather than offer a node `editStatements` must refuse.
      const options = (valueEntries ?? [])
        .filter(
          (entry) =>
            entry.identity !== 'compare' && admitsCall(field.type, entry.callable?.returns),
        )
        .map((entry) => ({ id: entry.identity, label: entry.displayName }))
      const current = currentId(node ?? field.node)
      if (
        current !== null &&
        !options.some((option) => option.id === current)
      ) {
        options.unshift({ id: current, label: callLabel(node, field) })
      }
      return usageOrder(options)
    }
    return usageOrder(enumOptions[field.domain ?? ''] ?? [])
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

  /**
   * The Action row's options: every `listEntries` action the service
   * lists — `__…__`-mangled internal sentinel ids excluded, since a
   * block opener's surface form needs the sibling `end` a single
   * statement edit cannot stage — plus the statement's own call when
   * the catalog does not list it (a `structural` call such as
   * `setGlobalVariable` only `lookup` resolves, docs/ui/editor.md §
   * Form · First row). Options the project already uses come first.
   */
  const actionOptions = $derived.by(() => {
    const options = (actionEntries ?? [])
      .filter(
        (entry) =>
          !(entry.identity.startsWith('__') && entry.identity.endsWith('__')),
      )
      .map((entry) => ({
        id: entry.identity,
        label: entry.displayName,
      }))
    const current = isInsert ? chosenCall : shownAction?.call
    if (typeof current === 'string' && current !== '' && !options.some((o) => o.id === current)) {
      options.unshift({
        id: current,
        label: names.peek(current, STATEMENT_KINDS)?.name ?? current,
      })
    }
    return usageOrder(options)
  })

  /** The `+` form's declared subroutines load when a `Subroutine` slot appears. */
  $effect(() => {
    if (!isInsert || target === null) return
    if (!flatFields(fields).some((field) => field.type === 'Subroutine')) return
    if (subroutineSymbols !== null || subroutinesLoading) return
    subroutinesLoading = true
    void call({ op: 'symbols', kind: 'subroutine', all: true })
      .then((response) => {
        if ('result' in response) {
          const result = response.result
          subroutineSymbols = (Array.isArray(result)
            ? result
            : ((result as { symbols?: Symbol[] }).symbols ?? [])) as Symbol[]
        }
      })
      .catch(() => {})
      .finally(() => {
        subroutinesLoading = false
      })
  })

  /**
   * Pick the Action row's call: an existing statement re-picks its own
   * call to restore the staged original, or picks a different one to
   * rebuild the argument rows from its `lookup` defaults; the `+`
   * form's pick seeds the empty statement the same way (docs/ui/
   * editor.md § Form · First row, "a default from `lookup` is filled
   * in when a call is chosen").
   */
  function chooseAction(id: string) {
    if (isCondition || !target) return
    if (isInsert) {
      callableEntry = null
      chosenCall = id
      editedArgs = []
      void names.entries(id).then((entries) => {
        if (shownFor !== target) return
        callableEntry =
          entries.find((entry) => STATEMENT_KINDS.includes(entry.kind)) ?? entries[0] ?? null
        editedArgs = defaultArgs(paramFacts(callableEntry))
      })
      return
    }
    if (id === sourceAction?.call) {
      editedCall = null
      editedArgs = null
      const requestLocale = locale
      void names.entries(id).then((entries) => {
        if (requestLocale === optionsLocale && shownFor === target) {
          callableEntry =
            entries.find((item) => STATEMENT_KINDS.includes(item.kind)) ?? entries[0] ?? null
        }
      })
      return
    }
    const entry = listedEntry(id) ?? null
    editedCall = id
    callableEntry = entry ?? callableEntry
    editedArgs = defaultArgs(paramFacts(entry))
    void names.warm([id])
  }

  function choose(field: ArgField, id: string) {
    if (!shownAction) return
    if (field.path.length === 0) {
      if (field.kind === 'comparison') {
        editedCall = id
        const requestLocale = locale
        void names.entries(id).then((entries) => {
          if (requestLocale === optionsLocale && shownFor === target) {
            valueEntry =
              entries.find((item) => VALUE_KINDS.includes(item.kind)) ?? entries[0] ?? null
          }
        })
        return
      }
      if (field.kind === 'call') {
        const entry = entryFor(id)
        editedCall = id
        editedArgs = defaultArgs(paramFacts(entry))
        valueEntry = entry
        void names.warm([id])
        return
      }
      return
    }
    const node =
      field.kind === 'boolean'
        ? id === 'true'
        : field.kind === 'variable'
          ? variableNode(id)
          : field.kind === 'operation'
            ? operationNode(id)
            : field.kind === 'call'
              ? callNode(id, paramFacts(entryFor(id)))
              : enumNode(id)
    if (field.kind === 'call' || field.kind === 'comparison') void names.warm([id])
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, node).args
  }

  function setLiteral(field: ArgField, text: string) {
    if (!shownAction || text === '') return
    const node =
      field.kind === 'number'
        ? Number(text)
        : field.kind === 'other' && /^-?\d+(\.\d+)?$/.test(text)
          ? Number(text)
          : stringNode(text)
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, node).args
  }

  /** A `Subroutine` slot's declared-name pick, a `{"subroutine"}` node. */
  function chooseSubroutine(field: ArgField, name: string) {
    if (!shownAction) return
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, subroutineNode(name)).args
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
      {#if !isCondition && shownAction}
        <!-- § Form · First row: the Action picker over every catalog
             action; choosing one rebuilds the parameter rows from
             `lookup` defaults. A `+` insert starts from an empty
             statement, so its selected call is the staged pick. -->
        <div class="editor-args">
          <div class="editor-arg" data-arg-control="action">
            <span class="editor-arg-label">{t.actionPickerLabel}</span>
            <Picker
              options={actionOptions}
              selected={isInsert ? chosenCall : shownAction.call === '' ? null : (shownAction.call as string)}
              placeholder={isInsert ? t.unset : t.actionPickerLabel}
              ariaLabel={t.actionPickerLabel}
              onselect={chooseAction}
            />
          </div>
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
                shownAction && typeof shownAction.call === 'string' && shownAction.call !== ''
                  ? shownAction.call
                  : null
              const value =
                call !== null && shownAction
                  ? ({ call, args: shownAction.args } as Node)
                  : (shownAction?.args?.[0] as Node | undefined)
              onsave(checked, undefined, value, undefined)
            } else {
              onsave(
                checked,
                editedArgs ?? undefined,
                undefined,
                (isInsert ? chosenCall : editedCall) ?? undefined,
              )
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
      {:else if field.kind === 'call' || field.kind === 'comparison'}
        <Picker
          options={optionsFor(field, node)}
          selected={currentId(node ?? field.node)}
          placeholder={unset ? t.unset : field.label}
          ariaLabel={field.label}
          onselect={(id) => choose(field, id)}
        />
      {:else if field.kind === 'other' && isInsert && field.type === 'Subroutine'}
        <!-- A `Subroutine` slot takes a `{"subroutine"}` node bound to a
             declared name; the `+` form lists the project's subroutines
             so the pick is always valid. With none declared the slot
             stays `insertable: false` and OK blocks (#920 review). -->
        <Picker
          options={(subroutineSymbols ?? []).map((symbol) => ({ id: symbol.name, label: symbol.name }))}
          selected={node !== undefined && node !== null && typeof node === 'object' && 'subroutine' in node ? (node.subroutine as string) : null}
          placeholder={unset ? t.unset : field.label}
          ariaLabel={field.label}
          onselect={(name) => chooseSubroutine(field, name)}
        />
      {:else if field.kind === 'other'}
        <span class="editor-arg-value" class:warn={unset}>
          {unset ? t.unset : node !== undefined ? statementText(node, names, VALUE_KINDS) : ''}
        </span>
      {:else}
        <Picker
          options={optionsFor(field, node)}
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

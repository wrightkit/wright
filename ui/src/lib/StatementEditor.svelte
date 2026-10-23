<script lang="ts">
  /**
   * The statement editor (ADR-0032 editing layer, #918): opened by clicking
   * a statement row — or an argument inside it, which focuses that slot's
   * control and scrolls it into view (docs/ui/editor.md § Form · Open).
   * The title is the call's session-locale name; each `lookup` parameter
   * gets one `--lbl` label cell and the control its slot type calls for:
   * a numeric input, a text input, the two-literal boolean picker, a
   * picker over the slot's options — actions, admitted values, enum
   * members, scope variables, modify operations, comparison operators
   * (#919) — with a call argument's own parameters as nested rows.
   * A required slot with no value reads "unset" in `--warn` and
   * disables OK. OK stages one `replace` edit through the pending-changes
   * model; Cancel stages nothing. The dialog composes no Workshop text.
   */
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
    /**
     * The identities the loaded program uses — call ids, enum member
     * identities, and declared names — so a picker lists the options the
     * project already uses first (docs/ui/editor.md § Pickers).
     */
    used?: ReadonlySet<string>
    /** The clicked argument's `args` path; its control gets focus. */
    focusPath?: number[] | null
    onsave: (disabled: boolean, call?: string, args?: Node[], value?: Node) => void
    onclose: () => void
  } = $props()

  let checked = $state(false)
  let shownFor: unknown = null
  /** The callable's `lookup` entry — an action's, or `compare` for a condition. */
  let callableEntry = $state<LookupEntry | null>(null)
  /** A condition's own `value.call` entry — its parameters decompose the value's args. */
  let valueEntry = $state<LookupEntry | null>(null)
  /** The form's staged args, replacing the statement's on save. */
  let editedArgs = $state<Node[] | null>(null)
  /** The staged call id — an action or condition value the picker replaced. */
  let editedCall = $state<string | null>(null)
  /** `listEntries` results per enum domain the form touches. */
  let enumOptions = $state<Record<string, { id: string; label: string }[]>>({})
  /** The project's declared variables, for `variable` fields. */
  let variableSymbols = $state<Symbol[] | null>(null)
  /** The `listEntries` `action` and `value` catalogs the pickers list. */
  let actionEntries = $state<LookupEntry[] | null>(null)
  let valueEntries = $state<LookupEntry[] | null>(null)
  /**
   * `lookup` entries per nested call id — a `call` field's own parameter
   * facts for its indented rows.
   */
  let nestedEntries = $state<Record<string, LookupEntry | null>>({})
  /** In-flight option fetches, so a `fields` recompute can't duplicate them. */
  const enumLoading = new Set<string>()
  const nestedLoading = new Set<string>()
  let symbolsLoading = false
  let actionLoading = false
  let valueLoading = false
  /** The locale the option caches below were loaded under. */
  let optionsLocale = ''
  let root: HTMLElement | undefined = $state()

  const isCondition = $derived(target?.list === 'conditions')

  /**
   * The args-tree the form edits: an action's own `args`, or a condition's
   * `value` dressed as a call — `value.call`/`value.args` — so one field
   * model serves both. A plain (non-call) condition value keeps no args.
   */
  const sourceAction = $derived.by((): Action | null => {
    if (target === null) return null
    if (!isCondition) return target.node as SpannedAction
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

  /** The `listEntries` catalogs a call id resolves to — an action or
      value picker's own options. */
  const listedEntry = (callId: string): LookupEntry | undefined =>
    (actionEntries ?? []).find((entry) => entry.identity === callId) ??
    (valueEntries ?? []).find((entry) => entry.identity === callId)

  /** The entry a call id resolves to for `paramFacts` — the `listEntries`
      catalogs first, then the `names.entries` cache a nested row fetched. */
  const entryFor = (callId: string): LookupEntry | null =>
    listedEntry(callId) ?? nestedEntries[callId] ?? null

  /** A nested call's parameters — its `listEntries` entry or its fetched
      `lookup` entry; `null` until either lands. */
  const nestedFacts = (callId: string): ParamFact[] | null => {
    const listed = listedEntry(callId)
    const entry = listed ?? (callId in nestedEntries ? nestedEntries[callId] : undefined)
    return entry === undefined ? null : paramFacts(entry)
  }

  const fields = $derived.by((): ArgField[] => {
    if (shownAction === null) return []
    if (isCondition) {
      // The condition's live value node — staged edits replace inside it.
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
    // A staged `change` seeds the form, so reopening shows it.
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
    // A staged `change` also seeds a renamed call, so reopening shows
    // the picker's choice.
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
      void names.entries(callId).then((entries) => {
        if (shownFor === target) {
          callableEntry =
            entries.find((entry) => STATEMENT_KINDS.includes(entry.kind)) ?? entries[0] ?? null
        }
      })
    }
  })

  /**
   * Focus the clicked argument's control once the form renders — its row
   * carries `data-arg-control` keyed by the `args` path and scrolls into
   * view (§ Form · Open). Runs once per open so a re-render mid-edit
   * never steals focus back.
   */
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

  // The form's option sources load once the callable's facts arrive:
  // operation spellings warm the names cache, enum domains list their
  // members, a variable field pulls the project's declared symbols, the
  // `call` rows pull the `listEntries` catalogs their pickers need, and
  // a nested call outside the catalogs fetches its own `lookup` entry for
  // its indented rows. A locale switch invalidates the caches so the
  // next pass refetches.
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
    // The Action row lists every action; `call` fields list the values
    // their slot type admits. Both come from `listEntries` in the
    // session locale (docs/ui/editor.md § Controls by slot type).
    if (!isCondition && actionEntries === null && !actionLoading) {
      actionLoading = true
      const requestLocale = locale
      void listAll('action', requestLocale)
        .then((entries) => {
          if (requestLocale === optionsLocale) actionEntries = entries
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
    // A `call` field's nested rows need the chosen call's own `lookup`
    // parameters; the field recomputes when the entry lands. A call the
    // `listEntries` catalogs already cover skips the fetch.
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
    const callId = shownAction?.call
    if (!isCondition && typeof callId === 'string' && callId !== '') {
      return names.peek(callId, STATEMENT_KINDS)?.name ?? callId
    }
    return t.conditionTitle
  })

  /** Every `listEntries` entry of a kind, page by page (ADR-0031). */
  async function listAll(
    kind: 'action' | 'value',
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

  /**
   * The pickers' usage ordering (docs/ui/editor.md § Pickers): options
   * the loaded program already uses come first, owner order kept inside
   * each group.
   */
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
        // A reserved or operator call the listing does not carry keeps
        // its own statement text as an option so the row reads right.
        options.unshift({ id: current, label: callLabel(node, field) })
      }
      return usageOrder(options)
    }
    return usageOrder(enumOptions[field.domain ?? ''] ?? [])
  }

  /**
   * The plain text a `call` row shows — the session-locale call name for a
   * catalog call, or the variable-access text a reserved call renders as.
   */
  function callLabel(node: Node | undefined, field: ArgField): string {
    // A pseudo-field with no arg position names its node on the field
    // itself (the condition's single `call` row carries the whole value).
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

  /** The action picker's options: every `listEntries` action, the
      statement's own call synthesized when the listing misses it (a
      `structural` call is not listable), used-first. */
  const actionOptions = $derived.by(() => {
    const options = (actionEntries ?? []).map((entry) => ({
      id: entry.identity,
      label: entry.displayName,
    }))
    const current = shownAction?.call
    if (typeof current === 'string' && current !== '' && !options.some((o) => o.id === current)) {
      options.unshift({
        id: current,
        label: names.peek(current, STATEMENT_KINDS)?.name ?? current,
      })
    }
    return usageOrder(options)
  })

  function chooseAction(id: string) {
    if (id === sourceAction?.call) {
      // Repicking the statement's own call restores its args and entry.
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
      // The condition's path-less rows: the operator row swaps the
      // value's call id, keeping the operands; the single `call` row
      // replaces the whole value with the picked call's defaults.
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
    const node = field.kind === 'number' ? Number(text) : stringNode(text)
    const filled = fillGaps(shownAction, fields, field.path)
    editedArgs = withArg(filled, field.path, node).args
  }
</script>

<Dialog open={target !== null} {onclose} class="editor" {title} titleClass="editor-title">
  {#if target}
    <div class="editor-root" bind:this={root}>
      <label class="editor-disable">
        <input type="checkbox" bind:checked={checked} data-testid="editor-disable" />
        {t.disabled}
      </label>
      {#if !isCondition && shownAction}
        <div class="editor-args">
          <div class="editor-arg" data-arg-control="call">
            <span class="editor-arg-label">{t.actionPickerLabel}</span>
            <Picker
              options={actionOptions}
              selected={shownAction.call === '' ? null : (shownAction.call as string)}
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
          disabled={missing.length > 0}
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
              onsave(checked, undefined, undefined, value)
            } else {
              onsave(checked, editedCall ?? undefined, editedArgs ?? undefined)
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

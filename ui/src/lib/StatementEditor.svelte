<script lang="ts">
  /**
   * The statement editor (ADR-0032 editing layer, #918): opened by clicking
   * a statement row — or an argument inside it, which focuses that slot's
   * control and scrolls it into view (docs/ui/editor.md § Form · Open).
   * The title is the call's session-locale name; each `lookup` parameter
   * gets one `--lbl` label cell and the control its slot type calls for:
   * a numeric input, a text input, the two-literal boolean picker, or a
   * call argument's plain-text name with the call's own parameters as
   * nested rows (#919 adds the action/enum/value/variable/operator
   * pickers). A required slot with no value reads "unset" in `--warn` and
   * disables OK. OK stages one `replace` edit through the pending-changes
   * model; Cancel stages nothing. The dialog composes no Workshop text.
   */
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
    /** The statement's effective disabled state, pending edits included. */
    disabled: boolean
    /** The pending `change` staged at this statement, when one exists. */
    staged?: RuleContentStatement
    /** The session locale's chrome strings. */
    t: ChromeStrings
    /** The session locale, for `listEntries`. */
    locale: string
    /** The clicked argument's `args` path; its control gets focus. */
    focusPath?: number[] | null
    /**
     * The `+` form (docs/ui/editor.md § Form · Insert): the target is an
     * empty statement — no disable toggle, OK stages an `insert`.
     */
    isInsert?: boolean
    onsave: (disabled: boolean, args?: Node[], value?: Node, call?: string) => void
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
  /** `listEntries` results per enum domain the form touches. */
  let enumOptions = $state<Record<string, { id: string; label: string }[]>>({})
  /** The project's declared variables, for `variable` fields. */
  let variableSymbols = $state<Symbol[] | null>(null)
  /**
   * `lookup` entries per nested call id — a `call` field's own parameter
   * facts for its indented rows.
   */
  let nestedEntries = $state<Record<string, LookupEntry | null>>({})
  /** In-flight option fetches, so a `fields` recompute can't duplicate them. */
  const enumLoading = new Set<string>()
  const nestedLoading = new Set<string>()
  let symbolsLoading = false
  /**
   * The `+` form's Action row options — every catalog action
   * `listEntries` returns (docs/ui/editor.md § Form · First row) — and
   * the call the user picked, which replaces the empty node's `call`.
   */
  let actionOptions = $state<{ id: string; label: string }[] | null>(null)
  let chosenCall = $state<string | null>(null)
  let actionsLoading = false
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

  /** A nested call's parameters, once its `lookup` entry landed. */
  const nestedFactsFor = (callId: string): ParamFact[] => paramFacts(nestedEntries[callId])
  const nestedFacts = (callId: string): ParamFact[] | null =>
    nestedEntries[callId] === undefined ? null : nestedFactsFor(callId)

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
  /**
   * OK stays disabled while a required slot is unset — and, on the `+`
   * form, while the statement has no call yet: an action needs the
   * Action row's pick, a condition a value (`null` is the empty form).
   */
  const canSave = $derived(
    missing.length === 0 &&
      (!isInsert ||
        (isCondition
          ? editedArgs !== null &&
            editedArgs[0] !== null &&
            editedArgs[0] !== undefined
          : chosenCall !== null &&
            // The call's `lookup` entry holds the parameter facts the
            // required-slot check needs; before it lands OK would
            // stage a bare `{call, args: []}`. */
            callableEntry !== null)),
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
      // The `+` form's empty action has no call to look up until the
      // Action row picks one (`chooseAction` fetches the entry then).
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
  // members, a variable field pulls the project's declared symbols, and a
  // nested `call` field fetches its own callable's `lookup` entry for its
  // indented rows. A locale switch invalidates the caches so the next
  // pass refetches.
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
    const allFields = (list: ArgField[]): ArgField[] =>
      list.flatMap((field) => (field.nested ? [field, ...allFields(field.nested)] : [field]))
    const flat = allFields(fields)
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
    // parameters; the field recomputes when the entry lands.
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
    // The `+` form has no statement to name: the heading reads like the
    // button that opened it.
    if (isInsert) return t.addStatement(isCondition ? t.conditionNoun : t.actionNoun)
    if (!isCondition) {
      const action = target.node as SpannedAction
      return names.peek(action.call, STATEMENT_KINDS)?.name ?? action.call
    }
    return t.conditionTitle
  })

  // The `+` form's Action row options list once, lazily — the picker
  // opens on the same `listEntries` source the other option lists use.
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

  /**
   * The `+` form's Action row: choosing the call seeds its `lookup`
   * defaults and refetches the entry so the parameter rows rebuild
   * (docs/ui/editor.md § Form: defaults fill in when a call is chosen).
   */
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

<script lang="ts">
  /**
   * The simulation panel (#836): the in-game custom-game room as a modal
   * dialog — two team columns of hero slots, a simulated-seconds field,
   * the project's lobby settings read-only, and a Start button. Start
   * builds an `athena` scenario from the occupied slots and the field and
   * calls `simulate` against the project identity; the result shows the
   * trace's outcome, variables, and logs, and a refusal is a message that
   * leaves the previous result in place. The two incomplete outcomes
   * carry the not-verified wording — the panel never presents them as
   * verified.
   */
  import type { ChromeStrings } from './chrome'
  import type {
    AthenaTrace,
    LookupEntry,
    ProjectResult,
    SettingsNode,
    ToolErrorInfo,
    ToolResponse,
  } from './types/generated/wright-agent-v1'
  import Picker from './Picker.svelte'
  import { call } from './api'
  import { buildScenario, logRows, matchTimeLimit, outcomeText, variableRows } from './simulate'

  let {
    open,
    project,
    heroes,
    t,
    onclose,
  }: {
    open: boolean
    project: ProjectResult | null
    heroes: LookupEntry[] | null
    t: ChromeStrings
    onclose: () => void
  } = $props()

  // Six slots per team, as in the in-game room; the user's slot is the
  // first team-1 slot. `null` hero means an empty slot — the scenario
  // carries only occupied slots.
  let slots = $state(
    Array.from({ length: 12 }, (_, index) => ({
      hero: null as string | null,
      team: (index < 6 ? 'team1' : 'team2') as 'team1' | 'team2',
    })),
  )
  let seconds = $state<string>('')
  let secondsTouched = $state(false)
  let running = $state(false)
  let trace = $state<AthenaTrace | null>(null)
  let refusal = $state<string | null>(null)
  let dialogEl: HTMLDialogElement | undefined = $state()

  const settings = $derived<SettingsNode[] | null>(project?.settings ?? null)
  // The room's run length (#836): the field wins once touched; otherwise
  // the lobby's match time limit, else 60.
  const effectiveSeconds = $derived.by(() => {
    if (secondsTouched) return seconds
    return String(matchTimeLimit(settings) ?? 60)
  })
  const outcome = $derived(trace ? outcomeText(trace) : null)

  const heroOptions = $derived(
    (heroes ?? []).map((entry) => ({
      // The scenario's hero is the catalog member token — `athena` accepts
      // the canonical member id and en-US spellings — while the picker's
      // label is the localized display name.
      id: entry.identity.split('.').pop() ?? entry.identity,
      label: entry.displayName ?? entry.identity,
    })),
  )

  $effect(() => {
    if (!dialogEl) return
    if (open && !dialogEl.open) dialogEl.showModal()
    if (!open && dialogEl.open) dialogEl.close()
  })

  async function start() {
    const value = Number(effectiveSeconds)
    if (!Number.isFinite(value) || value <= 0 || running || project?.inputIdentity == null) return
    running = true
    refusal = null
    try {
      const response = await call({
        op: 'simulate',
        inputIdentity: project.inputIdentity,
        scenario: buildScenario(slots, value),
      })
      if ('error' in response) {
        const error = (response as { error?: ToolErrorInfo }).error
        refusal = error?.message ?? t.simRefused
      } else {
        trace = ((response as ToolResponse).result as { trace?: AthenaTrace }).trace ?? null
      }
    } catch {
      refusal = t.simRefused
    } finally {
      running = false
    }
  }
</script>

<dialog
  bind:this={dialogEl}
  class="dialog view sim"
  id="dialog-simulate"
  onclose={onclose}
  onclick={(event) => event.target === dialogEl && onclose()}
>
  <h2 class="dlg-title">{t.simulation}</h2>
  <div class="dlg-body sim-body">
    <div class="room">
      {#each ['team1', 'team2'] as team (team)}
        <div class="team-col">
          <h3>{team === 'team1' ? t.simTeam1 : t.simTeam2}</h3>
          {#each slots as slot, index (index)}
            {#if slot.team === team}
              <div class="slot" class:you={index === 0}>
                <span class="slot-name">{index === 0 ? t.simYou : `${t.simSlot} ${(index % 6) + 1}`}</span>
                <Picker
                  options={heroOptions}
                  selected={slot.hero}
                  placeholder={t.simNoHero}
                  ariaLabel={`${t.simSlot} ${(index % 6) + 1}`}
                  onselect={(id) => (slot.hero = slot.hero === id ? null : id)}
                />
              </div>
            {/if}
          {/each}
        </div>
      {/each}
    </div>

    {#if settings && settings.length > 0}
      <section class="sim-settings" aria-label={t.simLobbySettings}>
        <h3>{t.simLobbySettings}</h3>
        {#each settings as node (node.name)}
          <div class="settings-group">
            <div class="settings-row group"><span>{node.name}</span></div>
            {#each node.children ?? [] as leaf (leaf.name)}
              <div class="settings-row">
                <span class="settings-name">{leaf.name}</span>
                <span class="settings-value">{leaf.value ?? ''}</span>
              </div>
            {/each}
          </div>
        {/each}
      </section>
    {/if}

    <div class="sim-run">
      <label class="seconds-field">
        <span>{t.simSeconds}</span>
        <input
          type="number"
          min="1"
          value={effectiveSeconds}
          oninput={(event) => {
            secondsTouched = true
            seconds = event.currentTarget.value
          }}
        />
      </label>
      <button type="button" class="primary" disabled={running} onclick={() => void start()}>
        {running ? t.simRunning : t.simStart}
      </button>
    </div>

    {#if refusal}
      <p class="refusal">{refusal}</p>
    {/if}

    {#if trace && outcome}
      <section class="sim-result">
        <div class="outcome" class:incomplete={outcome.status !== 'complete'}>
          <strong>{outcome.status}</strong>
          {#if outcome.detail}<span>{outcome.detail} — {t.simNotVerified}</span>{/if}
        </div>
        <h3>{t.simVariables}</h3>
        <div class="kv">
          {#each variableRows(trace) as row (row.name)}
            <div class="kv-row">
              <span class="settings-name">{row.name}</span>
              <span class="settings-value">{row.value}</span>
            </div>
          {/each}
        </div>
        <h3>{t.simLogs}</h3>
        <div class="kv">
          {#each logRows(trace) as row, index (index)}
            <div class="kv-row">
              <span class="settings-name">{row.tick}</span>
              <span class="settings-value">{row.text}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
  <div class="dlg-foot">
    <button type="button" class="primary" onclick={onclose}>{t.close}</button>
  </div>
</dialog>

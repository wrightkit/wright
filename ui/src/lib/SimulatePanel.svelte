<script lang="ts">
  import type { ChromeStrings } from './chrome'
  import Dialog from './components/Dialog.svelte'
  import type {
    AthenaTrace,
    LookupEntry,
    ProjectResult,
    SettingsNode,
    ToolErrorInfo,
    ToolResponse,
  } from './types/generated/wright-agent-v1'
  import Picker from './components/Picker.svelte'
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

  const settings = $derived<SettingsNode[] | null>(project?.settings ?? null)
  const effectiveSeconds = $derived.by(() => {
    if (secondsTouched) return seconds
    return String(matchTimeLimit(settings) ?? 60)
  })
  const outcome = $derived(trace ? outcomeText(trace) : null)

  const heroOptions = $derived(
    (heroes ?? []).map((entry) => ({
      id: entry.identity.split('.').pop() ?? entry.identity,
      label: entry.displayName ?? entry.identity,
    })),
  )

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

<Dialog
  view
  closeOnOutside
  {open}
  {onclose}
  id="dialog-simulate"
  class="sim"
  title={t.simulation}
>
  <div class="sim-body">
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
  {#snippet footer()}
    <button type="button" class="primary" onclick={onclose}>{t.close}</button>
  {/snippet}
</Dialog>

<style>
  /* The dialog's `sim` class lands on Bits UI markup, so it is global;
     the panel contents are this file's markup. */
  :global(.dialog.sim) {
    width: min(46rem, calc(100vw - 4rem));
    max-width: 46rem;
  }

  .sim-body .room {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
  }

  .sim-body .team-col h3 {
    margin: 0 0 0.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .sim-body .slot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.3rem;
  }

  .sim-body .slot-name {
    flex: none;
    width: 4.25rem;
    font-size: 0.8rem;
    color: var(--muted);
  }

  .sim-body .slot.you .slot-name {
    color: var(--gold);
    font-weight: 600;
  }

  .sim-settings {
    margin-top: 0.9rem;
  }

  .sim-body h3 {
    margin: 0.6rem 0 0.3rem;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .sim-settings .settings-group {
    background: var(--card);
    border-radius: 3px;
    padding: 0.45rem 0.6rem;
    margin-bottom: 0.35rem;
  }

  .settings-row {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.85rem;
  }

  .settings-row.group {
    color: var(--gold);
    font-weight: 600;
    margin-bottom: 0.15rem;
  }

  .settings-row .settings-name {
    color: var(--muted);
  }

  .settings-row .settings-value {
    font-variant-numeric: tabular-nums;
  }

  .sim-run {
    display: flex;
    align-items: flex-end;
    gap: 0.75rem;
    margin-top: 0.9rem;
  }

  .sim-run .seconds-field {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.8rem;
    color: var(--muted);
  }

  .sim-run .seconds-field input {
    width: 7rem;
    padding: 0.3rem 0.6rem;
    border: 0;
    border-radius: 3px;
    background: var(--white);
    color: var(--on-white);
    font-size: 0.85rem;
  }

  .sim-body .refusal {
    margin: 0.75rem 0 0;
  }

  .sim-result {
    margin-top: 0.9rem;
  }

  .sim-result .outcome {
    background: var(--card);
    border-radius: 3px;
    padding: 0.55rem 0.75rem;
    font-size: 0.9rem;
  }

  .sim-result .outcome strong {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-size: 0.8rem;
  }

  .sim-result .outcome.incomplete {
    border-left: 3px solid var(--gold);
  }

  .sim-result .outcome.incomplete span {
    color: var(--muted);
    margin-left: 0.5rem;
    font-size: 0.8rem;
  }

  .sim-result .kv {
    background: var(--card);
    border-radius: 3px;
    padding: 0.45rem 0.6rem;
    margin-bottom: 0.35rem;
  }

  .sim-result .kv-row {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.85rem;
  }
</style>

// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The statement editor's pickers (docs/ui/editor.md § Pickers, #919): the
 * Action row lists every `listEntries` action, a value slot lists the
 * values its parameter type admits, a variable slot lists the scope's
 * `symbols`, a condition's operator row lists the comparison operators,
 * a picked call rebuilds its rows from `lookup` defaults, and options the
 * project already uses come first.
 */
import { describe, expect, it, vi } from 'vitest'
import {
  api,
  action,
  click,
  contentFor,
  editor,
  editorLabels,
  handlerFor,
  mountApp,
  ok,
  openRule,
  pick,
  pickerOptions,
  rows,
  settle,
  summaryFor,
} from './lib/editor-harness'
import type { ToolRequest } from './lib/types/generated/wright-agent-v1'

/** Wire the shared harness `api` object into the mocked `./lib/api`. */
vi.mock('./lib/api', () => ({
  sessionToken: 'test-token',
  call: (request: ToolRequest) => {
    if (!api.handler) return Promise.reject(new Error('test installed no api handler'))
    return api.handler(request)
  },
}))

describe('the statement editor pickers (#919)', () => {
  const setGlobal = action('setGlobalVariable', [{ variable: 'score' }, 0])

  it('lists every action in the Action picker and rebuilds the args on change', async () => {
    api.handler = handlerFor(
      [
        summaryFor(
          1,
          'setup',
          contentFor([action('wait', [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }])]),
        ),
      ],
      [{ kind: 'globalVariable', name: 'score' }],
    )
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // The Action row heads the form and opens the `listEntries` action
    // catalog.
    const actionRow = editor().querySelector<HTMLElement>('[data-arg-control="call"]')!
    const options = await pickerOptions(actionRow)
    const labels = options.map((option) => option.textContent?.trim())
    expect(labels).toContain('Set Global Variable')
    expect(labels).toContain('Wait')
    expect(labels).toContain('Set Crouch Enabled')

    // Switching to `setGlobalVariable` rebuilds the rows from its
    // `lookup` facts: neither required slot carries a default, so the
    // Variable slot reads unset and OK stays disabled until one is
    // chosen.
    pick(options.find((option) => option.textContent?.trim() === 'Set Global Variable')!)
    await settle()
    expect(editorLabels()).toEqual(['Action', 'Variable', 'Value'])
    expect(ok().disabled, 'the required Variable slot is unset').toBe(true)
    expect(editor().querySelectorAll('.unset').length).toBeGreaterThan(0)
    expect(editor().querySelector('.editor-title')!.textContent).toBe('Set Global Variable')

    // The Variable slot is a picker over the scope's declared globals;
    // choosing `score` fills it.
    const variableRow = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    const variableOptions = await pickerOptions(variableRow)
    expect(variableOptions.map((option) => option.textContent?.trim())).toEqual(['score'])
    pick(variableOptions[0])
    await settle()

    // Switching back to `wait` fills its declared defaults: Wait
    // Behavior gets `Wait.IGNORE_CONDITION`.
    const actionRow2 = editor().querySelector<HTMLElement>('[data-arg-control="call"]')!
    const options2 = await pickerOptions(actionRow2)
    pick(options2.find((option) => option.textContent?.trim() === 'Wait')!)
    await settle()
    expect(editorLabels()).toEqual(['Action', 'Duration', 'Wait Behavior'])
    const waitRow = editor().querySelector<HTMLElement>('[data-arg-control="1"]')!
    expect(waitRow.querySelector('.picker-value')!.textContent).toBe('Ignore Condition')
    expect(editor().querySelector('.editor-title')!.textContent).toBe('Wait')
  })

  it('lists the slot-admitted values in a value slot picker', async () => {
    api.handler = handlerFor(
      [summaryFor(1, 'setup', contentFor([action('setGlobalVariable', [{ variable: 'score' }])]))],
      [{ kind: 'globalVariable', name: 'score' }],
    )
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // The Value slot (type Any) lists every value the `listEntries`
    // catalog admits — `compare` is excluded because rule-content-v1 has
    // no `{call:"compare"}`.
    const valueRow = editor().querySelector<HTMLElement>('[data-arg-control="1"]')!
    const options = await pickerOptions(valueRow)
    const labels = options.map((option) => option.textContent?.trim())
    expect(labels).toContain('Event Player')
    expect(labels).toContain('Max Health')
    expect(labels).not.toContain('Compare')
    expect(labels).not.toContain('Set Global Variable')
  })

  it('restricts the value picker by the slot type and rebuilds the nested rows on change', async () => {
    api.handler = handlerFor(
      [
        summaryFor(
          1,
          'setup',
          contentFor([
            action('setCrouchEnabled', [
              { call: 'eventPlayer', args: [] },
              { call: 'true', args: [] },
            ]),
          ]),
        ),
      ],
      [{ kind: 'globalVariable', name: 'score' }],
    )
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // The Player|Array slot admits `Player`- and `Array`-returning
    // values, not `Number`-returning `getMaxHealth`.
    const playerRow = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    let options = await pickerOptions(playerRow)
    let labels = options.map((option) => option.textContent?.trim())
    expect(labels).toContain('Event Player')
    expect(labels).toContain('All Players')
    expect(labels).not.toContain('Max Health')
    // Close the list before the next row's picker opens its own.
    pick(options.find((option) => option.textContent?.trim() === 'Event Player')!)

    // The Boolean slot admits `true` (the current value) but not
    // `getMaxHealth`; picking another call rebuilds its own rows.
    const enabledRow = editor().querySelector<HTMLElement>('[data-arg-control="1"]')!
    options = await pickerOptions(enabledRow)
    labels = options.map((option) => option.textContent?.trim())
    expect(labels).toContain('True')
    expect(labels).not.toContain('Max Health')

    // Repick the Player slot as `allPlayers` — an `Array` admit — and
    // its own Team parameter appears nested with its declared default.
    options = await pickerOptions(playerRow)
    pick(options.find((option) => option.textContent?.trim() === 'All Players')!)
    await settle()
    const nested = editor().querySelector<HTMLElement>(
      '.editor-nested [data-arg-control="0.0"]',
    )
    expect(nested, 'the picked call rebuilds its own parameter rows').not.toBeNull()
    expect(nested!.querySelector('.editor-arg-label')!.textContent).toBe('Team')
    expect(nested!.querySelector('.picker-value')!.textContent).toBe('All Teams')
  })

  it('lists the scope variables in a variable slot picker', async () => {
    api.handler = handlerFor(
      [
        summaryFor(
          1,
          'setup',
          contentFor([
            action('setGlobalVariable', [{ variable: 'score' }, 0]),
            action('setPlayerVariable', [
              { call: 'eventPlayer', args: [] },
              { variable: 'aim' },
              0,
            ]),
          ]),
        ),
      ],
      [
        { kind: 'globalVariable', name: 'score' },
        { kind: 'playerVariable', name: 'aim' },
      ],
    )
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    const variableRow = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    const options = await pickerOptions(variableRow)
    expect(options.map((option) => option.textContent?.trim())).toEqual(['score'])
  })

  it('lists the comparison operators in a condition operator slot', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor([], [{ value: { call: '>=', args: [1, 2] } }]),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    const operatorRow = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    const options = await pickerOptions(operatorRow)
    const labels = options.map((option) => option.textContent?.trim())
    for (const operator of ['==', '!=', '<', '<=', '>', '>=']) {
      expect(labels).toContain(operator)
    }
  })

  it('lists used options first', async () => {
    api.handler = handlerFor(
      [
        summaryFor(
          1,
          'setup',
          contentFor(
            [],
            [{ value: { call: '>=', args: [{ call: 'eventPlayer', args: [] }, 3] } }],
          ),
        ),
        summaryFor(
          2,
          'other',
          contentFor([action('setCrouchEnabled', [{ call: 'getMaxHealth', args: [] }, true])]),
        ),
      ],
      [{ kind: 'globalVariable', name: 'score' }],
    )
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // `eventPlayer` and `getMaxHealth` are used in the project; `true`
    // and `null` are not — the used entries lead the Value slot's list.
    const leftRow = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    const options = await pickerOptions(leftRow)
    const labels = options.map((option) => option.textContent?.trim())
    const eventPlayer = labels.indexOf('Event Player')
    const maxHealth = labels.indexOf('Max Health')
    const trueIndex = labels.indexOf('True')
    expect(eventPlayer, 'eventPlayer is listed').toBeGreaterThanOrEqual(0)
    expect(maxHealth, 'getMaxHealth is listed').toBeGreaterThanOrEqual(0)
    expect(eventPlayer).toBeLessThan(trueIndex)
    expect(maxHealth).toBeLessThan(trueIndex)
  })
})

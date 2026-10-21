import { describe, expect, it } from 'vitest'
import {
  buildRequest,
  effectiveDisabled,
  isStale,
  keyFor,
  setDisabled,
  stageDeletes,
  type PendingChange,
  type StatementNode,
} from './edit'

const span = { start: { line: 9, col: 9 }, end: { line: 9, col: 30 } }

const node = (extra: Record<string, unknown> = {}): StatementNode =>
  ({
    call: 'setGlobalVariable',
    args: [{ variable: 'score' }, 1],
    span,
    ...extra,
  }) as StatementNode

const pending = () => new Map<string, PendingChange>()

describe('setDisabled', () => {
  it('stages a replace whose statement carries disabled:true, spans stripped', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    const change = map.get(keyFor(2, 'actions', 0))
    expect(change?.tag).toBe('change')
    expect(change?.statement).toEqual({
      call: 'setGlobalVariable',
      args: [{ variable: 'score' }, 1],
      disabled: true,
    })
  })

  it('stages enabling a disabled statement by removing the modifier', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node({ disabled: true }), false)
    const change = map.get(keyFor(2, 'actions', 0))
    expect(change?.statement).toEqual({
      call: 'setGlobalVariable',
      args: [{ variable: 'score' }, 1],
    })
    expect(change?.statement).not.toHaveProperty('disabled')
  })

  it('unstages instead of producing a no-op replace back at the original state', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    setDisabled(map, 2, 'setup', 'actions', 0, node(), false)
    expect(map.size).toBe(0)
  })

  it('is a no-op at the state the staged edits already produce', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    expect(map.size).toBe(1)
  })

  it('leaves a marked-for-deletion row alone', () => {
    const map = pending()
    stageDeletes(map, 2, 'setup', 'actions', [0])
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    expect(map.get(keyFor(2, 'actions', 0))?.tag).toBe('delete')
  })
})

describe('effectiveDisabled', () => {
  it('reads the staged value ahead of the node', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    expect(effectiveDisabled(map, 2, 'actions', 0, node())).toBe(true)
    expect(effectiveDisabled(map, 2, 'actions', 0, node({ disabled: true }))).toBe(true)
  })

  it('reads the node when nothing is staged', () => {
    const map = pending()
    expect(effectiveDisabled(map, 2, 'actions', 0, node())).toBe(false)
    expect(effectiveDisabled(map, 2, 'actions', 0, node({ disabled: true }))).toBe(true)
  })
})

describe('stageDeletes', () => {
  it('supersedes a staged change at the same address', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    stageDeletes(map, 2, 'setup', 'actions', [0])
    expect(map.get(keyFor(2, 'actions', 0))?.tag).toBe('delete')
    expect(map.get(keyFor(2, 'actions', 0))?.statement).toBeUndefined()
  })
})

describe('buildRequest', () => {
  it('carries the freshness pair and one edit per staged change', () => {
    const map = pending()
    setDisabled(map, 2, 'setup', 'actions', 0, node(), true)
    stageDeletes(map, 1, 'branch', 'actions', [1, 2])
    const request = buildRequest(map, 'sha256:abc', 4)
    expect(request).toEqual({
      op: 'editStatements',
      inputIdentity: 'sha256:abc',
      snapshot: 4,
      edits: [
        {
          rule: 2,
          list: 'actions',
          index: 0,
          change: 'replace',
          statement: { call: 'setGlobalVariable', args: [{ variable: 'score' }, 1], disabled: true },
        },
        { rule: 1, list: 'actions', index: 1, change: 'delete' },
        { rule: 1, list: 'actions', index: 2, change: 'delete' },
      ],
    })
  })
})

describe('isStale', () => {
  it('recognizes the freshness refusal codes', () => {
    expect(isStale([{ code: 'edit-stale-source' }])).toBe(true)
    expect(isStale([{ code: 'stale-id' }])).toBe(true)
    expect(isStale([{ code: 'edit-unknown-source' }])).toBe(false)
    expect(isStale([])).toBe(false)
    expect(isStale(undefined)).toBe(false)
  })
})

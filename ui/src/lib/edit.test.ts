import { describe, expect, it } from 'vitest'
import {
  buildRequest,
  effectiveDisabled,
  insertAddress,
  insertKey,
  insertMarkKey,
  isStale,
  keyFor,
  setDisabled,
  stageDeletes,
  stageInsert,
  stageStatement,
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

describe('insertAddress', () => {
  it('appends at the column end with no selection', () => {
    expect(insertAddress(3, [])).toBe(3)
  })

  it('targets after the one selected row', () => {
    expect(insertAddress(3, [1])).toBe(2)
    expect(insertAddress(3, [2])).toBe(3)
  })

  it('appends at the column end when more than one row is selected', () => {
    expect(insertAddress(3, [0, 1])).toBe(3)
  })
})

describe('stageInsert', () => {
  it('stages the statement plus its placeholder mark', () => {
    const map = pending()
    stageInsert(map, 2, 'setup', 'actions', 1, [{ call: 'wait', args: [0.25] }])
    const change = map.get(insertKey(2, 'actions', 1))
    expect(change?.tag).toBe('insert')
    expect(change?.statements).toEqual([{ call: 'wait', args: [0.25] }])
    expect(map.get(insertMarkKey(2, 'actions', 1))?.position).toBe('before')
  })

  it('does not collide with a change or delete on the row at the index', () => {
    const map = pending()
    stageInsert(map, 1, 'r', 'actions', 1, [{ call: 'wait', args: [1] }])
    stageDeletes(map, 1, 'r', 'actions', [1])
    expect(buildRequest(map, 'digest', 0).edits).toEqual([
      {
        rule: 1,
        list: 'actions',
        index: 1,
        change: 'insert',
        statements: [{ call: 'wait', args: [1] }],
      },
      { rule: 1, list: 'actions', index: 1, change: 'delete' },
    ])
    expect(map.get(keyFor(1, 'actions', 1))?.tag).toBe('delete')
    expect(map.get(insertKey(1, 'actions', 1))?.tag).toBe('insert')

    const second = pending()
    stageDeletes(second, 1, 'r', 'actions', [1])
    stageInsert(second, 1, 'r', 'actions', 1, [{ call: 'wait', args: [1] }])
    expect(second.get(keyFor(1, 'actions', 1))?.tag).toBe('delete')
    expect(buildRequest(second, 'digest', 0).edits).toHaveLength(2)

    const third = pending()
    setDisabled(third, 1, 'r', 'actions', 1, node(), true)
    stageInsert(third, 1, 'r', 'actions', 1, [{ call: 'wait', args: [1] }])
    expect(third.get(keyFor(1, 'actions', 1))?.tag).toBe('change')
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

  it('emits an insert at its address and skips the placeholder mark', () => {
    const map = pending()
    stageInsert(map, 2, 'setup', 'actions', 1, [
      { call: 'wait', args: [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }] },
    ])
    const request = buildRequest(map, 'sha256:abc', 4)
    expect(request.edits).toEqual([
      {
        rule: 2,
        list: 'actions',
        index: 1,
        change: 'insert',
        statements: [{ call: 'wait', args: [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }] }],
      },
    ])
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

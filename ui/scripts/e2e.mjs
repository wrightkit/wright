#!/usr/bin/env node
/**
 * End-to-end harness for `wright ui` (#804): starts `wright ui --no-open`
 * on the committed Workshop fixture and drives the page in a real headless
 * Chromium-family browser over the DevTools protocol — no browser driver
 * dependency; Node >= 22 already has fetch and WebSocket.
 *
 *   WRIGHT_BIN          wright binary (default: ../target/debug/wright,
 *                       falling back to ../target/release/wright)
 *   WRIGHT_UI_BROWSER   browser executable (default: the first of
 *                       brave/chromium/chrome/edge found on PATH or in the
 *                       platform's usual application locations)
 *
 * The CDP driving is deliberately small: Target.attach + Runtime.evaluate.
 * Assertions run inside the page against its DOM, so clicks and rendered
 * text are exactly what a user produces and sees.
 */
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..', '..')
const fixture = join(root, 'tests', 'fixtures', 'workshop', 'synthetic', 'ui-view.ws')

const WRIGHT_CANDIDATES = [
  process.env.WRIGHT_BIN,
  join(root, 'target', 'debug', 'wright'),
  join(root, 'target', 'release', 'wright'),
].filter(Boolean)

const BROWSER_COMMANDS = [
  // google-chrome before chromium: Ubuntu's `chromium` is often the snap
  // shim, which exits without a DevTools endpoint instead of launching.
  'google-chrome',
  'google-chrome-stable',
  'brave',
  'brave-browser',
  'chromium',
  'chromium-browser',
  'microsoft-edge',
  'headless_shell',
]
const BROWSER_APP_PATHS = [
  '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
]

function which(command) {
  const path = process.env.PATH ?? ''
  for (const dir of path.split(':')) {
    const candidate = join(dir, command)
    if (existsSync(candidate)) return candidate
  }
  return null
}

function findWright() {
  for (const candidate of WRIGHT_CANDIDATES) {
    if (existsSync(candidate)) return candidate
  }
  fail(`no wright binary found; set WRIGHT_BIN (tried ${WRIGHT_CANDIDATES.join(', ')})`)
}

function findBrowser() {
  if (process.env.WRIGHT_UI_BROWSER) {
    const command = which(process.env.WRIGHT_UI_BROWSER)
    if (command) return command
    if (existsSync(process.env.WRIGHT_UI_BROWSER)) return process.env.WRIGHT_UI_BROWSER
    fail(`WRIGHT_UI_BROWSER does not exist: ${process.env.WRIGHT_UI_BROWSER}`)
  }
  for (const command of BROWSER_COMMANDS) {
    const found = which(command)
    if (found) return found
  }
  for (const candidate of BROWSER_APP_PATHS) {
    if (existsSync(candidate)) return candidate
  }
  fail('no Chromium-family browser found; set WRIGHT_UI_BROWSER')
}

function fail(message) {
  console.error(`e2e: ${message}`)
  process.exitCode = 1
  throw new Error(message)
}

/** Yield the next complete line from a stdio stream, in order. */
function nextLine(stream) {
  let pending = ''
  const queue = []
  const waiters = []
  stream.setEncoding('utf8')
  stream.on('data', (chunk) => {
    pending += chunk
    let index
    while ((index = pending.indexOf('\n')) >= 0) {
      queue.push(pending.slice(0, index))
      pending = pending.slice(index + 1)
    }
    while (queue.length > 0 && waiters.length > 0) waiters.shift()(queue.shift())
  })
  return () =>
    new Promise((resolve) => {
      if (queue.length > 0) resolve(queue.shift())
      else waiters.push(resolve)
    })
}

/** Scan lines until one satisfies `predicate` or the timeout fires. */
async function waitLine(read, predicate, what, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const remaining = deadline - Date.now()
    if (remaining <= 0) throw new Error(`timed out waiting for ${what}`)
    const line = await Promise.race([
      read(),
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error(`timed out waiting for ${what}`)), remaining),
      ),
    ])
    if (predicate(line)) return line
  }
}

/** A minimal CDP client over the browser-level WebSocket. */
class Cdp {
  #ws
  #next = 1
  #pending = new Map()

  static async connect(wsUrl) {
    const cdp = new Cdp()
    cdp.#ws = new WebSocket(wsUrl)
    await new Promise((resolve, reject) => {
      cdp.#ws.addEventListener('open', resolve, { once: true })
      cdp.#ws.addEventListener('error', reject, { once: true })
    })
    cdp.#ws.addEventListener('message', (event) => {
      const message = JSON.parse(event.data)
      if (message.id !== undefined && cdp.#pending.has(message.id)) {
        const { resolve, reject } = cdp.#pending.get(message.id)
        cdp.#pending.delete(message.id)
        if (message.error) reject(new Error(message.error.message))
        else resolve(message.result)
      }
    })
    return cdp
  }

  send(method, params = {}, sessionId) {
    const id = this.#next++
    const message = { id, method, params }
    if (sessionId) message.sessionId = sessionId
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject })
      this.#ws.send(JSON.stringify(message))
    })
  }

  /** Attach to a new page and return its session id. */
  async openPage(url) {
    const { targetId } = await this.send('Target.createTarget', { url })
    const { sessionId } = await this.send('Target.attachToTarget', {
      targetId,
      flatten: true,
    })
    return sessionId
  }

  /** Evaluate an expression in a page session, returning the JSON value. */
  async evaluate(sessionId, expression) {
    const { result, exceptionDetails } = await this.send(
      'Runtime.evaluate',
      { expression, returnByValue: true, awaitPromise: true },
      sessionId,
    )
    if (exceptionDetails) {
      throw new Error(`page evaluation failed: ${JSON.stringify(exceptionDetails)}`)
    }
    return result.value
  }

  /** Poll until the expression is truthy or the deadline passes. On
      timeout the failure report carries the rule view's DOM, since a
      missing element alone cannot distinguish loading, refusal, and a
      wrong rule. */
  async waitFor(sessionId, expression, timeoutMs = 15_000) {
    const deadline = Date.now() + timeoutMs
    for (;;) {
      const value = await this.evaluate(sessionId, expression)
      if (value) return value
      if (Date.now() > deadline) {
        const dom = await this.evaluate(
          sessionId,
          `document.querySelector('.rule-view')?.outerHTML ?? document.body.outerHTML`,
        ).catch(() => '<dom dump failed>')
        const net = await this.evaluate(
          sessionId,
          `JSON.stringify(performance.getEntriesByType('resource').map(e => [e.name.split('/').pop(), Math.round(e.startTime), Math.round(e.duration), e.responseStatus]))`,
        ).catch(() => '<net dump failed>')
        throw new Error(
          `timed out waiting for: ${expression}\nrule-view: ${String(dom).slice(0, 2000)}\nnet: ${net}`,
        )
      }
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
  }

  close() {
    this.#ws.close()
  }
}

const checks = []
let failures = 0
function check(name, ok, detail = '') {
  checks.push({ name, ok })
  if (ok) console.log(`  ok   ${name}`)
  else {
    failures += 1
    console.log(`  FAIL ${name}${detail ? ` — ${detail}` : ''}`)
  }
}

async function main() {
  const wright = findWright()
  const browser = findBrowser()
  console.log(`e2e: wright=${wright}`)
  console.log(`e2e: browser=${browser}`)

  const ui = spawn(wright, ['ui', '--no-open', fixture], {
    stdio: ['ignore', 'pipe', 'inherit'],
  })
  // The browser profile lives under target/ so the worktree owns the
  // temporary state.
  const profile = join(root, 'target', `ui-e2e-profile-${process.pid}`)
  mkdirSync(profile, { recursive: true })
  let chrome
  let cdp
  try {
    const sessionUrl = await waitLine(
      nextLine(ui.stdout),
      (line) => line.startsWith('http://'),
      'the wright ui session URL',
    )
    const [base, token] = sessionUrl.trim().split('#')
    if (!base.startsWith('http://127.0.0.1:') || !token) {
      fail(`wright ui printed no loopback session URL: ${sessionUrl}`)
    }
    console.log(`e2e: session ${base}`)

    chrome = spawn(
      browser,
      [
        '--headless=new',
        '--remote-debugging-port=0',
        '--remote-allow-origins=*',
        `--user-data-dir=${profile}`,
        '--no-first-run',
        '--disable-gpu',
        '--disable-extensions',
        'about:blank',
      ],
      { stdio: ['ignore', 'ignore', 'pipe'] },
    )
    if (process.env.E2E_TRACE) {
      chrome.stderr.on('data', (chunk) =>
        console.log(`[chrome] ${String(chunk).slice(0, 300)}`),
      )
      chrome.on('exit', (code, signal) => console.log(`[chrome exit ${code} ${signal}]`))
      chrome.on('error', (error) => console.log(`[chrome error ${error.message}]`))
    }
    const wsLine = await waitLine(
      nextLine(chrome.stderr),
      (line) => line.startsWith('DevTools listening on ws://'),
      'the browser DevTools endpoint',
      30_000,
    )
    const wsUrl = wsLine.trim().replace(/^DevTools listening on\s*/, '')

    cdp = await Cdp.connect(wsUrl)
    const page = await cdp.openPage(`${base}#${token}`)
    await cdp.waitFor(
      page,
      `document.readyState === 'complete' && document.querySelectorAll('[data-e2e=rule-item]').length > 0`,
    )

    // Rule list order and names come straight from `rules`.
    const names = await cdp.evaluate(
      page,
      `[...document.querySelectorAll('[data-e2e=rule-item] [data-e2e=rule-name]')].map(e => e.textContent)`,
    )
    check(
      'rules render in service order',
      JSON.stringify(names) ===
        JSON.stringify(['off', 'setup', 'players', 'opaque', 'after', 'Subroutine shared']),
      JSON.stringify(names),
    )
    check(
      'the disabled rule is marked in the list',
      await cdp.evaluate(
        page,
        `document.querySelector('[data-e2e=rule-item]').classList.contains('disabled') &&
         document.querySelector('[data-e2e=rule-item] [data-e2e=disabled]')?.textContent === 'disabled'`,
      ),
    )

    // Clicking a rule is asynchronous inside the page — the previous
    // rule's statements still render until the new state flushes — so each
    // open waits on the rule's own title before asserting its body.
    const openRule = async (id, name) => {
      await cdp.evaluate(page, `document.querySelector('[data-rule-id="${id}"]').click()`)
      await cdp.waitFor(
        page,
        `document.querySelector('.rule-title')?.textContent.trim() === ${JSON.stringify(name)}`,
      )
    }

    // Open "setup": event, conditions, indented statements, names.
    await openRule(1, 'setup')
    await cdp.waitFor(page, `document.querySelectorAll('[data-e2e=statement]').length > 0`)
    check(
      'the event header shows the lookup display name',
      (await cdp.evaluate(page, `document.querySelector('[data-e2e=event]').textContent`)).includes(
        'Ongoing - Global',
      ),
    )
    check(
      'conditions render with lookup names and the disabled marker',
      await cdp.evaluate(
        page,
        `(() => {
          const items = [...document.querySelectorAll('[data-e2e=condition]')]
          return items.length === 2 &&
            items[0].textContent.includes('score') &&
            items[0].textContent.includes('<') &&
            items[0].textContent.includes('10') &&
            items[1].classList.contains('disabled') &&
            items[1].querySelector('[data-e2e=disabled]')?.textContent === 'disabled'
        })()`,
      ),
    )
    const statements = await cdp.evaluate(
      page,
      `[...document.querySelectorAll('[data-e2e=statement]')].map(e => e.textContent)`,
    )
    check(
      'actions render with lookup display names',
      statements.some((t) => t.includes('If')) &&
        statements.some((t) => t.includes('Set Global Variable')) &&
        statements.some((t) => t.includes('Call Subroutine')) &&
        statements.some((t) => t.includes('While')) &&
        statements.some((t) => t.includes('Modify Global Variable')) &&
        statements.some((t) => t.includes('Add')) &&
        statements.some((t) => t.includes('Wait')) &&
        statements.some((t) => t.includes('Ignore Condition')),
      JSON.stringify(statements),
    )
    check(
      'no End statements render as lines',
      !statements.some((t) => t.trim() === 'End'),
      JSON.stringify(statements),
    )
    check(
      'block bodies indent deeper than their openers',
      await cdp.evaluate(
        page,
        `(() => {
          const items = [...document.querySelectorAll('[data-e2e=statement]')]
          const depthOf = el => parseInt(getComputedStyle(el).paddingLeft)
          const ifLine = items.find(e => e.textContent.includes('If('))
          const setLine = items.find(e => e.textContent.includes('Set Global Variable(score, 1)'))
          const elseLine = items.find(e => e.textContent.trimStart().startsWith('Else'))
          const modifyLine = items.find(e => e.textContent.includes('Modify Global Variable'))
          return ifLine && setLine && elseLine && modifyLine &&
            depthOf(setLine) > depthOf(ifLine) &&
            depthOf(elseLine) === depthOf(ifLine) &&
            depthOf(modifyLine) > depthOf(ifLine)
        })()`,
      ),
    )

    // The disabled statement inside "players".
    await openRule(2, 'players')
    await cdp.waitFor(page, `document.querySelectorAll('[data-e2e=statement]').length > 0`)
    check(
      'a disabled statement is marked',
      await cdp.evaluate(
        page,
        `[...document.querySelectorAll('[data-e2e=statement]')].some(e =>
           e.classList.contains('disabled') && e.textContent.includes('Wait'))`,
      ),
    )
    check(
      'the each-player event shows its filters',
      await cdp.evaluate(
        page,
        `(() => {
          const event = document.querySelector('[data-e2e=event]').textContent
          return event.includes('Ongoing - Each Player') &&
            event.includes('Team 1') && event.includes('Slot 0')
        })()`,
      ),
    )

    // A rule the export cannot express shows the refusal; a later rule
    // still renders.
    await openRule(3, 'opaque')
    await cdp.waitFor(page, `document.querySelector('[data-e2e=refusal]')`)
    check(
      'an inexpressible rule renders the service refusal',
      (await cdp.evaluate(page, `document.querySelector('[data-e2e=refusal]').textContent`)).includes(
        'rule-content-unavailable',
      ),
    )
    await openRule(4, 'after')
    await cdp.waitFor(
      page,
      `[...document.querySelectorAll('[data-e2e=statement]')].some(e => e.textContent.includes('Wait'))`,
    )
    check('rules after the refused one still render', true)

    // Debug output stays behind ?debug.
    check(
      'no debug output without ?debug',
      (await cdp.evaluate(page, `document.querySelectorAll('[data-debug]').length`)) === 0,
    )
    const debugPage = await cdp.openPage(`${base}?debug#${token}`)
    await cdp.waitFor(
      debugPage,
      `document.querySelectorAll('[data-e2e=statement]').length > 0`,
    )
    check(
      '?debug adds line numbers, call ids, and node JSON',
      await cdp.evaluate(
        debugPage,
        `document.querySelectorAll('[data-debug]').length > 0 &&
         [...document.querySelectorAll('[data-debug]')].some(e => e.textContent.includes('line '))`,
      ),
    )

    // The dialogs read `symbols` and `findings`.
    await cdp.evaluate(page, `document.querySelector('[data-e2e=open-variables]').click()`)
    await cdp.waitFor(page, `document.querySelectorAll('[data-e2e=symbol]').length > 0`)
    check(
      'the Variables dialog lists declared variables and subroutines',
      await cdp.evaluate(
        page,
        `(() => {
          const text = document.querySelector('[data-e2e=dialog]').textContent
          return text.includes('score') && text.includes('hp') && text.includes('shared')
        })()`,
      ),
    )
    await cdp.evaluate(page, `document.querySelector('[data-e2e=dialog-close]').click()`)
    await cdp.evaluate(page, `document.querySelector('[data-e2e=open-diagnostics]').click()`)
    await cdp.waitFor(page, `document.querySelectorAll('[data-e2e=finding]').length > 0`)
    check(
      'the Diagnostics dialog lists findings',
      await cdp.evaluate(
        page,
        `document.querySelector('[data-e2e=dialog]').textContent.includes('min-wait-loop')`,
      ),
    )

    // The name filter narrows the list.
    await cdp.evaluate(
      page,
      `(() => {
        const input = document.querySelector('[data-e2e=rule-filter]')
        input.value = 'set'
        input.dispatchEvent(new Event('input', { bubbles: true }))
      })()`,
    )
    const filteredNames = await cdp.evaluate(
      page,
      `[...document.querySelectorAll('[data-e2e=rule-item] [data-e2e=rule-name]')].map(e => e.textContent)`,
    )
    check(
      'the name filter narrows the rule list',
      JSON.stringify(filteredNames) === JSON.stringify(['setup']),
      JSON.stringify(filteredNames),
    )
  } finally {
    cdp?.close()
    chrome?.kill('SIGKILL')
    ui.kill()
    rmSync(profile, { recursive: true, force: true })
  }

  if (failures > 0) {
    console.error(`e2e: ${failures} of ${checks.length} checks failed`)
    process.exitCode = 1
  } else {
    console.log(`e2e: all ${checks.length} checks passed`)
  }
}

main().catch((error) => {
  console.error(`e2e: ${error.message ?? error}`)
  process.exitCode = 1
})

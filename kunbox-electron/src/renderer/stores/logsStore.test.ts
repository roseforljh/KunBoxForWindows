import assert from 'node:assert/strict'
import { afterEach, beforeEach, test } from 'node:test'
import type { LogEntry } from '../../shared/types'
import { subscribeRuntimeLogs, useLogsStore } from './logsStore.ts'

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
let callbacks: Set<(entry: LogEntry) => void>
let timers: Map<number, () => void>
let events: EventTarget
let resolveSettings: (settings: { enableRuntimeLogs: boolean }) => void
let cleanups: Array<() => void>

beforeEach(() => {
  useLogsStore.getState().clearLogs()
  useLogsStore.getState().setLogsEnabled(true)
  callbacks = new Set()
  timers = new Map()
  events = new EventTarget()
  cleanups = []
  let nextTimerId = 0
  const settings = new Promise<{ enableRuntimeLogs: boolean }>((resolve) => {
    resolveSettings = resolve
  })
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: Object.assign(events, {
      api: {
        singbox: {
          onLog: (callback: (entry: LogEntry) => void) => {
            callbacks.add(callback)
            return () => callbacks.delete(callback)
          },
        },
        settings: { get: () => settings },
      },
      setInterval: (callback: () => void) => {
        const id = ++nextTimerId
        timers.set(id, callback)
        return id
      },
      clearInterval: (id: number) => timers.delete(id),
    }),
  })
})

afterEach(() => {
  for (const cleanup of cleanups) cleanup()
  useLogsStore.getState().clearLogs()
  if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow)
  else Reflect.deleteProperty(globalThis, 'window')
})

function start() {
  const stop = subscribeRuntimeLogs()
  cleanups.push(stop)
  return stop
}

function entry(message: string): LogEntry {
  return { timestamp: 1, level: 'info', tag: 'sing-box', message }
}

function emit(message: string) {
  for (const callback of callbacks) callback(entry(message))
}

function flush() {
  for (const callback of timers.values()) callback()
}

function toggle(enabled: boolean) {
  events.dispatchEvent(new CustomEvent('runtime-logs-change', { detail: { enabled } }))
}

test('collects logs before the page opens and while the page is unmounted', () => {
  start()
  emit('before opening logs')
  flush()
  assert.equal(useLogsStore.getState().logs[0].message, 'before opening logs')

  const closePage = useLogsStore.subscribe(() => {})
  emit('page open')
  closePage()
  emit('page closed before batch flush')
  flush()
  assert.equal(callbacks.size, 1)
  assert.deepEqual(useLogsStore.getState().logs.map((log) => log.message), [
    'before opening logs', 'page open', 'page closed before batch flush',
  ])

  emit('another page')
  flush()
  assert.equal(useLogsStore.getState().logs.at(-1)?.message, 'another page')
})

test('clear removes both displayed logs and logs waiting for the next batch', () => {
  start()
  emit('displayed')
  flush()
  emit('pending')
  useLogsStore.getState().clearLogs()
  flush()
  assert.deepEqual(useLogsStore.getState().logs, [])
  emit('after clear')
  flush()
  assert.deepEqual(useLogsStore.getState().logs.map((log) => log.message), ['after clear'])
})

test('disabling recording retains history and resuming accepts new logs', () => {
  start()
  emit('before disabling')
  toggle(false)
  emit('disabled')
  flush()
  assert.equal(useLogsStore.getState().logsEnabled, false)
  assert.deepEqual(useLogsStore.getState().logs.map((log) => log.message), ['before disabling'])
  toggle(true)
  emit('resumed')
  flush()
  assert.deepEqual(useLogsStore.getState().logs.map((log) => log.message), ['before disabling', 'resumed'])
})

test('a late settings response cannot overwrite a newer setting change', async () => {
  start()
  toggle(false)
  resolveSettings({ enableRuntimeLogs: true })
  await Promise.resolve()
  emit('must stay disabled')
  flush()
  assert.equal(useLogsStore.getState().logsEnabled, false)
  assert.deepEqual(useLogsStore.getState().logs, [])
})

test('the saved disabled setting is respected even when logs page has never opened', async () => {
  start()
  resolveSettings({ enableRuntimeLogs: false })
  await Promise.resolve()
  emit('disabled at startup')
  flush()
  assert.equal(useLogsStore.getState().logsEnabled, false)
  assert.deepEqual(useLogsStore.getState().logs, [])
})

test('cleanup flushes pending logs and prevents late callbacks from adding entries', async () => {
  const stop = start()
  const lateCallback = [...callbacks][0]
  emit('pending at cleanup')
  stop()
  lateCallback(entry('late event'))
  resolveSettings({ enableRuntimeLogs: false })
  await Promise.resolve()
  assert.equal(callbacks.size, 0)
  assert.equal(timers.size, 0)
  assert.equal(useLogsStore.getState().logsEnabled, true)
  assert.deepEqual(useLogsStore.getState().logs.map((log) => log.message), ['pending at cleanup'])
})

test('bursts retain only the newest 500 entries across batches', () => {
  start()
  for (let index = 0; index < 1500; index++) emit(String(index))
  flush()
  assert.equal(useLogsStore.getState().logs.length, 500)
  assert.equal(useLogsStore.getState().logs[0].message, '1000')
  emit('1500')
  flush()
  assert.equal(useLogsStore.getState().logs.length, 500)
  assert.equal(useLogsStore.getState().logs[0].message, '1001')
  assert.equal(useLogsStore.getState().logs.at(-1)?.message, '1500')
})

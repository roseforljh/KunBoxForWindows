import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { notifyRunningInstance, requestStartupElevation, startActivationServer, waitForAdminRestart } from './windowsAdmin'

test('Windows startup requests elevation only when enabled and not already elevated or relaunched', { skip: process.platform !== 'win32' }, async () => {
  let requests = 0
  const launch = async () => { requests++ }
  const notAdmin = async () => false

  assert.equal(await requestStartupElevation(false, launch, notAdmin, []), false)
  assert.equal(await requestStartupElevation(true, launch, async () => true, []), false)
  assert.equal(await requestStartupElevation(true, launch, notAdmin, ['--kunbox-wait-for-exit=123']), false)
  assert.equal(requests, 0)
  assert.equal(await requestStartupElevation(true, launch, notAdmin, []), true)
  assert.equal(requests, 1)

  const cancelled = new Error('UAC cancelled')
  await assert.rejects(requestStartupElevation(true, async () => { throw cancelled }, notAdmin, []), cancelled)
})

test('elevated restart waits for the previous process and leaves it alone on timeout', { skip: process.platform !== 'win32' }, async () => {
  const previous = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 30000)'], { windowsHide: true })
  const exited = once(previous, 'exit')
  const args = [`--kunbox-wait-for-exit=${previous.pid}`]

  try {
    await assert.rejects(waitForAdminRestart(args, 1))
    assert.equal(previous.exitCode, null)

    const waiting = waitForAdminRestart(args)
    previous.kill()
    await waiting
    await exited
  } finally {
    if (previous.exitCode === null) previous.kill()
    await exited
  }
})

test('activation server wakes running instance and cleans up port file', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'kunbox-activate-'))
  const portFile = join(dir, 'instance.port')
  let activated = 0

  try {
    assert.equal(await notifyRunningInstance(portFile, 200), false)

    const stop = startActivationServer(portFile, () => {
      activated++
    })

    try {
      for (let i = 0; i < 20 && !existsSync(portFile); i++) {
        await new Promise((resolve) => setTimeout(resolve, 25))
      }
      assert.equal(existsSync(portFile), true)
      assert.equal(await notifyRunningInstance(portFile, 500), true)
      for (let i = 0; i < 20 && activated === 0; i++) {
        await new Promise((resolve) => setTimeout(resolve, 25))
      }
      assert.equal(activated, 1)
    } finally {
      stop()
    }

    assert.equal(existsSync(portFile), false)
    assert.equal(await notifyRunningInstance(portFile, 200), false)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

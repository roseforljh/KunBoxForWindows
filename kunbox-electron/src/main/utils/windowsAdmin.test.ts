import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { test } from 'node:test'
import { waitForAdminRestart } from './windowsAdmin'

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

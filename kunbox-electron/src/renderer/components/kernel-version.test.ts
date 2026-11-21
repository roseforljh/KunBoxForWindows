import assert from 'node:assert/strict'
import { isNewerRelease } from './kernel-version'
import { mockApi } from '../../shared/mock-api'

assert.equal(isNewerRelease('1.0.0-0123456789ab', '0.0.2'), true)
assert.equal(isNewerRelease('1.0.0-invalid', '0.0.2'), false)
assert.equal(isNewerRelease('0.0.9', '0.0.10'), true)
assert.equal(isNewerRelease('0.1.0', '0.0.10'), false)
assert.equal(isNewerRelease('0.0.2', '0.0.2'), false)
assert.equal(isNewerRelease('invalid', '0.0.2'), false)
assert.equal(isNewerRelease('0.0.1', '0.0.2-beta'), false)
const phases: string[] = []
const off = mockApi.kernel.onDownloadProgress(event => { if (event.phase) phases.push(event.phase) })
assert.equal((await mockApi.kernel.download('windows-v0.0.2')).success, true)
assert.deepEqual(phases, ['downloading', 'verifying', 'installing'])
assert.equal((await mockApi.kernel.getLocalVersion()).version, '0.0.2')
assert.equal(await mockApi.kernel.canRollback(), true)
assert.equal((await mockApi.kernel.rollback()).success, true)
assert.equal((await mockApi.kernel.getLocalVersion()).version, '0.0.1')
off()

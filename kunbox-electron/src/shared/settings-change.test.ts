import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mockApi } from './mock-api'

test('settings subscribers receive the saved port snapshot and can unsubscribe', async () => {
  const { get, set, onChange } = mockApi.settings
  const initial = await get()
  const updates: Awaited<ReturnType<typeof get>>[] = []
  const unsubscribe = onChange((settings) => updates.push(settings))
  await set({ localPort: 17890 })
  assert.deepEqual(updates, [{ ...initial, localPort: 17890 }])
  unsubscribe()
  await set({ socksPort: 17891 })
  assert.equal(updates.length, 1)
  assert.equal((await get()).socksPort, 17891)
})

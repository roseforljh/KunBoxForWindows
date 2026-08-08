import assert from 'node:assert/strict'
import test from 'node:test'

test('节点延迟缓存不会随时间失效', async () => {
  const cacheKey = 'profile-1::node-a'
  const persistedStore = JSON.stringify({
    state: {
      latencyCache: {
        [cacheKey]: {
          latencyMs: 123,
          latencyStatus: 'success',
          isTimeout: false,
          timestamp: Date.now() - 7 * 24 * 60 * 60 * 1000,
        },
      },
      activeNodeTag: 'node-a',
      nodeSelections: {},
    },
    version: 0,
  })

  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => key === 'kunbox-nodes-store' ? persistedStore : null,
      setItem: () => {},
      removeItem: () => {},
    },
  })
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      api: {
        profile: { getActive: async () => 'profile-1' },
        node: {
          list: async () => [{ tag: 'node-a', type: 'socks' }],
          getActive: async () => 'node-a',
          setActive: async () => {},
        },
      },
    },
  })

  const { useNodesStore } = await import('./nodesStore.ts')
  await useNodesStore.persist.rehydrate()
  await useNodesStore.getState().loadNodes()

  assert.equal(useNodesStore.getState().nodes[0]?.latencyMs, 123)
  assert.equal(useNodesStore.getState().latencyCache[cacheKey]?.latencyMs, 123)
})

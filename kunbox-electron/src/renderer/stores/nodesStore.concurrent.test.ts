import assert from 'node:assert/strict'
import test from 'node:test'

Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    getItem: () => null,
    setItem: () => {},
    removeItem: () => {},
  },
})

test('测速代际判断丢弃旧批次结果', async () => {
  const { shouldCommitLatencyResult } = await import('./nodesStore.ts')
  assert.equal(shouldCommitLatencyResult(10, 10), true)
  assert.equal(shouldCommitLatencyResult(10, 11), false)
})

test('并发切换节点与 loadNodes 乱序返回时，UI 始终保持最后一次选中结果', async () => {
  let activeTagOnBackend = 'Node2'
  let setActiveCalls: string[] = []

  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      api: {
        profile: { getActive: async () => 'profile-1' },
        singbox: {
          switchNode: async (tag: string) => {
            const delay = tag === 'Node2' ? 35 : 5
            await new Promise((r) => setTimeout(r, delay))
            activeTagOnBackend = tag
            return { success: true }
          },
        },
        node: {
          list: async () => [
            { tag: 'Node1', type: 'socks' },
            { tag: 'Node2', type: 'socks' },
          ],
          getActive: async () => {
            await new Promise((r) => setTimeout(r, 35))
            return 'Node2'
          },
          setActive: async (tag: string) => {
            setActiveCalls.push(tag)
            activeTagOnBackend = tag
          },
        },
      },
    },
  })

  const { useNodesStore } = await import('./nodesStore.ts')

  // 1. 先点 Node2（慢返回）再点 Node1（快返回），最终必须停留在 Node1
  const [resOld, resNew] = await Promise.all([
    useNodesStore.getState().selectNode('Node2'),
    useNodesStore.getState().selectNode('Node1'),
  ])
  assert.equal(resOld, false)
  assert.equal(resNew, true)
  assert.equal(useNodesStore.getState().activeNodeTag, 'Node1')

  // 2. loadNodes 正在异步拉取旧 backendActiveNodeTag='Node2' 期间用户选中 Node1，
  //    loadNodes 完成后不能将 activeNodeTag 覆盖回 Node2
  setActiveCalls = []
  const loading = useNodesStore.getState().loadNodes()
  const switched = await useNodesStore.getState().selectNode('Node1')
  await loading

  assert.equal(switched, true)
  assert.equal(useNodesStore.getState().activeNodeTag, 'Node1')
  assert.deepEqual(setActiveCalls, [])
  assert.equal(activeTagOnBackend, 'Node1')
})

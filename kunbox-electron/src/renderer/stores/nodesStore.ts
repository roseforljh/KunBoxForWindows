import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { SingBoxOutbound, NodeWithProfile, NodeLatencyResult, NodeLatencyStatus, HealthStatus } from '@shared/types'

interface NodeItem extends SingBoxOutbound {
  latencyMs?: number | null
  latencyStatus?: NodeLatencyStatus
  healthStatus?: HealthStatus
  isTimeout?: boolean
  isTesting?: boolean
}

interface AllNodeItem extends NodeWithProfile {
  latencyMs?: number | null
  latencyStatus?: NodeLatencyStatus
  healthStatus?: HealthStatus
  isTimeout?: boolean
}

export type FilterMode = 'none' | 'include' | 'exclude'

export interface NodeFilter {
  filterMode: FilterMode
  includeKeywords: string[]
  excludeKeywords: string[]
}

// Persisted latency data
interface LatencyCache {
  [tag: string]: {
    latencyMs: number | null
    latencyStatus?: NodeLatencyStatus
    isTimeout: boolean
  }
}

const MIN_VALID_CACHED_LATENCY_MS = 10

function normalizeLatencyResult(result: NodeLatencyResult) {
  const latencyStatus = result.status
  return {
    latencyMs: result.latencyMs ?? null,
    latencyStatus,
    isTimeout: latencyStatus === 'timeout',
  }
}

function localFailureLatencyResult(): ReturnType<typeof normalizeLatencyResult> {
  return normalizeLatencyResult({
    status: 'local_test_failed',
    latencyMs: null,
  })
}

function isUsableCachedLatency(
  entry: LatencyCache[string] | null | undefined,
): entry is LatencyCache[string] {
  if (!entry) return false
  if (
    entry.latencyStatus === 'success' &&
    (!entry.latencyMs || entry.latencyMs < MIN_VALID_CACHED_LATENCY_MS)
  ) {
    return false
  }
  return true
}

// Abort controller for batch testing
let abortController: AbortController | null = null
let testAllRunId = 0

interface NodesState {
  nodes: NodeItem[]
  allNodes: AllNodeItem[]
  activeNodeTag: string | null
  searchText: string
  sortMode: 'default' | 'latency' | 'name' | 'region'
  nodeFilter: NodeFilter
  isTesting: boolean
  testProgress: number
  testTotal: number
  latencyCache: LatencyCache
  nodeSelections: Record<string, string>
  nodeHealth: Record<string, HealthStatus>

  setNodes: (nodes: SingBoxOutbound[]) => Promise<void>
  setActiveNode: (tag: string | null) => void
  setNodeHealth: (tag: string, status: HealthStatus) => void
  setSearchText: (text: string) => void
  setSortMode: (mode: NodesState['sortMode']) => void
  setNodeFilter: (filter: NodeFilter) => void
  clearNodeFilter: () => void
  selectNode: (tag: string) => Promise<boolean>
  testAllLatency: () => Promise<void>
  cancelTestAllLatency: () => void
  testNodeLatency: (tag: string) => Promise<void>
  loadNodes: () => Promise<void>
  loadAllNodes: () => Promise<void>
  saveNodeSelection: (profileId: string, nodeTag: string) => void
  restoreNodeSelection: (profileId: string) => string | null
}

export const useNodesStore = create<NodesState>()(
  persist(
    (set, get) => ({
      nodes: [],
      allNodes: [],
      activeNodeTag: null,
      searchText: '',
      sortMode: 'default',
      nodeFilter: {
        filterMode: 'none',
        includeKeywords: [],
        excludeKeywords: []
      },
      isTesting: false,
      testProgress: 0,
      testTotal: 0,
      latencyCache: {},
      nodeSelections: {},
      nodeHealth: {},

      setNodes: async (nodes: SingBoxOutbound[]) => {
        const { latencyCache, nodeHealth } = get()
        const activeProfileId = await window.api.profile.getActive()
        
        const nodesWithLatency = nodes.map((n: SingBoxOutbound) => {
          const cacheKey = activeProfileId && n.tag ? `${activeProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          const healthStatus = n.tag ? nodeHealth[n.tag] : undefined
          if (isUsableCachedLatency(cached)) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              healthStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n, healthStatus }
        })
        set({ nodes: nodesWithLatency })
      },
      setActiveNode: (tag: string | null) => set({ activeNodeTag: tag }),
      setNodeHealth: (tag: string, status: HealthStatus) => {
        set((state: NodesState) => ({
          nodeHealth: {
            ...state.nodeHealth,
            [tag]: status
          },
          nodes: state.nodes.map((node) => node.tag === tag
            ? { ...node, healthStatus: status }
            : node
          ),
          allNodes: state.allNodes.map((node) => node.tag === tag
            ? { ...node, healthStatus: status }
            : node
          )
        }))
      },
      setSearchText: (text: string) => set({ searchText: text }),
      setSortMode: (mode: NodesState['sortMode']) => set({ sortMode: mode }),
      setNodeFilter: (filter: NodeFilter) => set({ nodeFilter: filter }),
      clearNodeFilter: () => set({ 
        nodeFilter: { filterMode: 'none', includeKeywords: [], excludeKeywords: [] } 
      }),

      selectNode: async (tag: string) => {
        const result = await window.api.singbox.switchNode(tag)
        if (!result.success) {
          return false
        }

        await window.api.node.setActive(tag)
        set({ activeNodeTag: tag })
        return true
      },

      saveNodeSelection: (profileId: string, nodeTag: string) => {
        set((state: NodesState) => ({
          nodeSelections: {
            ...state.nodeSelections,
            [profileId]: nodeTag
          }
        }))
      },

      restoreNodeSelection: (profileId: string) => {
        return get().nodeSelections[profileId] || null
      },

      testAllLatency: async () => {
        const { nodes } = get()
        if (nodes.length === 0) return

        const activeProfileId = await window.api.profile.getActive()
        if (!activeProfileId) return

        abortController = new AbortController()
        const signal = abortController.signal
        const runId = ++testAllRunId
        const queue = nodes
          .map((node: NodeItem, index: number) => ({ node, index }))
          .filter((item): item is { node: NodeItem; index: number } => Boolean(item.node.tag))

        if (queue.length === 0) return

        const batchLatencyCache: LatencyCache = {}
        const queueIndices = new Set(queue.map((item) => item.index))

        set((state: NodesState) => ({
          isTesting: true,
          testProgress: 0,
          testTotal: queue.length,
          nodes: state.nodes.map((node, index) => queueIndices.has(index)
            ? { ...node, isTesting: true }
            : node
          )
        }))

        try {
          await window.api.node.beginLatencyTests(runId)

          const CONCURRENCY = 8
          let nextQueueIndex = 0

          const testNextNode = async () => {
            while (nextQueueIndex < queue.length) {
              if (signal.aborted || runId !== testAllRunId) return

              const item = queue[nextQueueIndex]
              nextQueueIndex += 1

              const tag = item.node.tag
              if (!tag) continue

              let normalized: ReturnType<typeof normalizeLatencyResult>
              try {
                const result = await window.api.node.testLatency(tag, runId)
                if (signal.aborted || runId !== testAllRunId) return
                normalized = normalizeLatencyResult(result)
              } catch {
                if (signal.aborted || runId !== testAllRunId) return
                normalized = localFailureLatencyResult()
              }

              batchLatencyCache[`${activeProfileId}::${tag}`] = normalized
              set((state: NodesState) => {
                if (signal.aborted || runId !== testAllRunId) return state
                return {
                  nodes: state.nodes.map((node, index) => index === item.index
                    ? { ...node, ...normalized, isTesting: false }
                    : node
                  ),
                  latencyCache: { ...state.latencyCache, ...batchLatencyCache },
                  testProgress: Math.min(state.testProgress + 1, queue.length)
                }
              })
            }
          }

          const workers = Array.from(
            { length: Math.min(CONCURRENCY, queue.length) },
            () => testNextNode()
          )
          await Promise.all(workers)
        } catch {
          if (signal.aborted || runId !== testAllRunId) return

          for (const node of queue) {
            if (!node.node.tag) continue
            const fallback = localFailureLatencyResult()
            batchLatencyCache[`${activeProfileId}::${node.node.tag}`] = {
              latencyMs: fallback.latencyMs,
              latencyStatus: fallback.latencyStatus,
              isTimeout: fallback.isTimeout
            }
          }
          set((state: NodesState) => {
            if (signal.aborted || runId !== testAllRunId) return state
            return {
              nodes: state.nodes.map(n => n.tag
                ? { ...n, isTesting: false, ...localFailureLatencyResult() }
                : { ...n, isTesting: false }
              ),
              latencyCache: { ...state.latencyCache, ...batchLatencyCache },
              testProgress: queue.length
            }
          })
        } finally {
          if (runId === testAllRunId) {
            abortController = null
            set((state: NodesState) => ({
              isTesting: false,
              testProgress: 0,
              nodes: state.nodes.map(n => ({ ...n, isTesting: false })),
              latencyCache: { ...state.latencyCache, ...batchLatencyCache }
            }))
          }
        }
      },

      cancelTestAllLatency: () => {
        const cancelledRunId = testAllRunId
        testAllRunId += 1
        if (abortController) {
          abortController.abort()
          abortController = null
        }
        void window.api.node.cancelLatencyTests(cancelledRunId).catch(() => {})
        set((state: NodesState) => ({
          isTesting: false,
          testProgress: 0,
          nodes: state.nodes.map(n => ({ ...n, isTesting: false }))
        }))
      },

      testNodeLatency: async (tag) => {
        const activeProfileId = await window.api.profile.getActive()
        if (!activeProfileId) return

        set((state) => ({
          nodes: state.nodes.map(n => n.tag === tag ? { ...n, isTesting: true } : n)
        }))

        const cacheKey = `${activeProfileId}::${tag}`

        try {
          const result = await window.api.node.testLatency(tag)
          const normalized = normalizeLatencyResult(result)
          
          set((state: NodesState) => ({
            nodes: state.nodes.map(n => n.tag === tag
              ? { ...n, ...normalized, isTesting: false }
              : n
            ),
            latencyCache: {
              ...state.latencyCache,
              [cacheKey]: normalized
            }
          }))
        } catch {
          const fallback = localFailureLatencyResult()
          set((state: NodesState) => ({
            nodes: state.nodes.map(n => n.tag === tag 
              ? { ...n, isTesting: false, ...fallback } 
              : n
            ),
            latencyCache: {
              ...state.latencyCache,
              [cacheKey]: fallback
            }
          }))
        }
      },

      loadNodes: async () => {
        const nodes = await window.api.node.list()
        const activeProfileId = await window.api.profile.getActive()
        const backendActiveNodeTag = await window.api.node.getActive()
        const { activeNodeTag, latencyCache, nodeHealth } = get()

        const nodesWithLatency = nodes.map((n: SingBoxOutbound) => {
          const cacheKey = activeProfileId && n.tag ? `${activeProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          const healthStatus = n.tag ? nodeHealth[n.tag] : undefined
          if (isUsableCachedLatency(cached)) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              healthStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n, healthStatus }
        })

        const nodeTags = new Set(nodes.map((node) => node.tag).filter(Boolean))
        const validBackendTag = backendActiveNodeTag && nodeTags.has(backendActiveNodeTag)
          ? backendActiveNodeTag
          : null
        const validLocalTag = activeNodeTag && nodeTags.has(activeNodeTag)
          ? activeNodeTag
          : null
        const nextActiveTag = validBackendTag || validLocalTag || nodes[0]?.tag || null

        if (nextActiveTag && nextActiveTag !== backendActiveNodeTag) {
          await window.api.node.setActive(nextActiveTag)
        }

        set({
          nodes: nodesWithLatency,
          activeNodeTag: nextActiveTag
        })
      },

      loadAllNodes: async () => {
        const allNodes = await window.api.node.listAll()
        const { latencyCache, nodeHealth } = get()
        
        const nodesWithLatency = allNodes.map((n: NodeWithProfile) => {
          const cacheKey = n.sourceProfileId && n.tag ? `${n.sourceProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          const healthStatus = n.tag ? nodeHealth[n.tag] : undefined
          if (isUsableCachedLatency(cached)) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              healthStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n, healthStatus }
        })
        
        set({ allNodes: nodesWithLatency })
      }
    }),
    {
      name: 'kunbox-nodes-store',
      partialize: (state) => ({
        latencyCache: state.latencyCache,
        activeNodeTag: state.activeNodeTag,
        nodeSelections: state.nodeSelections
      })
    }
  )
)

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { SingBoxOutbound, NodeWithProfile, NodeLatencyResult, NodeLatencyStatus } from '@shared/types'

interface NodeItem extends SingBoxOutbound {
  latencyMs?: number | null
  latencyStatus?: NodeLatencyStatus
  isTimeout?: boolean
  isTesting?: boolean
}

interface AllNodeItem extends NodeWithProfile {
  latencyMs?: number | null
  latencyStatus?: NodeLatencyStatus
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
    timestamp: number
  }
}

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

  setNodes: (nodes: SingBoxOutbound[]) => Promise<void>
  setActiveNode: (tag: string | null) => void
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

      setNodes: async (nodes: SingBoxOutbound[]) => {
        const { latencyCache } = get()
        const now = Date.now()
        const activeProfileId = await window.api.profile.getActive()
        
        const nodesWithLatency = nodes.map((n: SingBoxOutbound) => {
          const cacheKey = activeProfileId && n.tag ? `${activeProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          if (cached && now - cached.timestamp < 3600000) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n }
        })
        set({ nodes: nodesWithLatency })
      },
      setActiveNode: (tag: string | null) => set({ activeNodeTag: tag }),
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

        const batchLatencyCache: LatencyCache = {}

        set((state: NodesState) => ({
          isTesting: true,
          testProgress: 0,
          testTotal: queue.length,
          nodes: state.nodes.map(n => ({ ...n, isTesting: true }))
        }))

        try {
          await window.api.node.beginLatencyTests(runId)

          const CONCURRENCY = 8
          for (let i = 0; i < queue.length; i += CONCURRENCY) {
            const batch = queue.slice(i, i + CONCURRENCY)
            await Promise.all(batch.map(async (item) => {
              if (signal.aborted || runId !== testAllRunId) return

              const tag = item.node.tag
              if (!tag) return

              let latencyMs: number | null = null
              let latencyStatus: NodeLatencyStatus = 'local_test_failed'
              let isTimeout = false

              try {
                const result = await window.api.node.testLatency(tag, runId)
                if (signal.aborted || runId !== testAllRunId) return
                const normalized = normalizeLatencyResult(result)
                latencyMs = normalized.latencyMs
                latencyStatus = normalized.latencyStatus
                isTimeout = normalized.isTimeout
              } catch {
                if (signal.aborted || runId !== testAllRunId) return
                const fallback = localFailureLatencyResult()
                latencyMs = fallback.latencyMs
                latencyStatus = fallback.latencyStatus
                isTimeout = fallback.isTimeout
              }

              const timestamp = Date.now()
              const cacheKey = `${activeProfileId}::${tag}`
              batchLatencyCache[cacheKey] = { latencyMs, latencyStatus, isTimeout, timestamp }
              set((state: NodesState) => {
                if (signal.aborted || runId !== testAllRunId) return state
                return {
                  nodes: state.nodes.map((node, index) => index === item.index
                    ? { ...node, latencyMs, latencyStatus, isTimeout, isTesting: false }
                    : node
                  ),
                  testProgress: Math.min(state.testProgress + 1, queue.length)
                }
              })
            }))
          }
        } catch {
          if (signal.aborted || runId !== testAllRunId) return

          const timestamp = Date.now()
          for (const node of queue) {
            if (!node.node.tag) continue
            const fallback = localFailureLatencyResult()
            batchLatencyCache[`${activeProfileId}::${node.node.tag}`] = {
              latencyMs: fallback.latencyMs,
              latencyStatus: fallback.latencyStatus,
              isTimeout: fallback.isTimeout,
              timestamp
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
              [cacheKey]: { ...normalized, timestamp: Date.now() }
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
              [cacheKey]: { ...fallback, timestamp: Date.now() }
            }
          }))
        }
      },

      loadNodes: async () => {
        const nodes = await window.api.node.list()
        const activeProfileId = await window.api.profile.getActive()
        const backendActiveNodeTag = await window.api.node.getActive()
        const { activeNodeTag, latencyCache } = get()
        const now = Date.now()

        const newCache = { ...latencyCache }
        let cacheChanged = false
        for (const tag in newCache) {
          if (now - newCache[tag].timestamp >= 3600000) {
            delete newCache[tag]
            cacheChanged = true
          }
        }

        const nodesWithLatency = nodes.map((n: SingBoxOutbound) => {
          const cacheKey = activeProfileId && n.tag ? `${activeProfileId}::${n.tag}` : null
          const cached = cacheKey ? newCache[cacheKey] : null
          if (cached) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n }
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
          activeNodeTag: nextActiveTag,
          ...(cacheChanged ? { latencyCache: newCache } : {})
        })
      },

      loadAllNodes: async () => {
        const allNodes = await window.api.node.listAll()
        const { latencyCache } = get()
        const now = Date.now()
        
        const nodesWithLatency = allNodes.map((n: NodeWithProfile) => {
          const cacheKey = n.sourceProfileId && n.tag ? `${n.sourceProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          if (cached && now - cached.timestamp < 3600000) {
            return {
              ...n,
              latencyMs: cached.latencyMs,
              latencyStatus: cached.latencyStatus,
              isTimeout: cached.isTimeout,
            }
          }
          return { ...n }
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
      }),
       merge: (persisted: any, current: NodesState) => {
        const now = Date.now()
        if (persisted.latencyCache) {
          for (const key in persisted.latencyCache) {
            if (now - persisted.latencyCache[key].timestamp >= 3600000) {
              delete persisted.latencyCache[key]
            }
          }
        }
        return { ...current, ...persisted }
      }
    }
  )
)

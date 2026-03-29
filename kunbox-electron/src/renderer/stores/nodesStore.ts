import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { SingBoxOutbound, NodeWithProfile } from '@shared/types'

interface NodeItem extends SingBoxOutbound {
  latencyMs?: number | null
  isTimeout?: boolean
  isTesting?: boolean
}

interface AllNodeItem extends NodeWithProfile {
  latencyMs?: number | null
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
    isTimeout: boolean
    timestamp: number
  }
}

// Abort controller for batch testing
let abortController: AbortController | null = null

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

      setNodes: async (nodes) => {
        const { latencyCache } = get()
        const now = Date.now()
        const activeProfileId = await window.api.profile.getActive()
        
        const nodesWithLatency = nodes.map((n: SingBoxOutbound) => {
          const cacheKey = activeProfileId && n.tag ? `${activeProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          if (cached && now - cached.timestamp < 3600000) {
            return { ...n, latencyMs: cached.latencyMs, isTimeout: cached.isTimeout }
          }
          return { ...n }
        })
        set({ nodes: nodesWithLatency })
      },
      setActiveNode: (tag) => set({ activeNodeTag: tag }),
      setSearchText: (text) => set({ searchText: text }),
      setSortMode: (mode) => set({ sortMode: mode }),
      setNodeFilter: (filter) => set({ nodeFilter: filter }),
      clearNodeFilter: () => set({ 
        nodeFilter: { filterMode: 'none', includeKeywords: [], excludeKeywords: [] } 
      }),

      selectNode: async (tag) => {
        const result = await window.api.singbox.switchNode(tag)
        if (!result.success) {
          return false
        }

        await window.api.node.setActive(tag)
        set({ activeNodeTag: tag })
        return true
      },

      saveNodeSelection: (profileId, nodeTag) => {
        set((state) => ({
          nodeSelections: {
            ...state.nodeSelections,
            [profileId]: nodeTag
          }
        }))
      },

      restoreNodeSelection: (profileId) => {
        return get().nodeSelections[profileId] || null
      },

      testAllLatency: async () => {
        const { nodes } = get()
        if (nodes.length === 0) return

        const activeProfileId = await window.api.profile.getActive()
        if (!activeProfileId) return

        abortController = new AbortController()
        const signal = abortController.signal

        set({ isTesting: true, testProgress: 0, testTotal: nodes.length })

        for (let i = 0; i < nodes.length; i++) {
          if (signal.aborted) {
            break
          }

          const node = nodes[i]
          if (!node.tag) continue
          
          set({ testProgress: i + 1 })
          
          try {
            const latency = await window.api.node.testLatency(node.tag)
            const isTimeout = latency <= 0
            const latencyMs = latency > 0 ? latency : null
            
            const cacheKey = `${activeProfileId}::${node.tag}`
            
            set((state) => ({
              nodes: state.nodes.map(n => n.tag === node.tag
                ? { ...n, latencyMs, isTimeout }
                : n
              ),
              latencyCache: {
                ...state.latencyCache,
                [cacheKey]: { latencyMs, isTimeout, timestamp: Date.now() }
              }
            }))
          } catch {
            const cacheKey = `${activeProfileId}::${node.tag}`
            set((state) => ({
              nodes: state.nodes.map(n => n.tag === node.tag
                ? { ...n, latencyMs: null, isTimeout: true }
                : n
              ),
              latencyCache: {
                ...state.latencyCache,
                [cacheKey]: { latencyMs: null, isTimeout: true, timestamp: Date.now() }
              }
            }))
          }
        }
        
        abortController = null
        set({ isTesting: false, testProgress: 0 })
      },

      cancelTestAllLatency: () => {
        if (abortController) {
          abortController.abort()
          abortController = null
        }
        set({ isTesting: false, testProgress: 0 })
      },

      testNodeLatency: async (tag) => {
        const activeProfileId = await window.api.profile.getActive()
        if (!activeProfileId) return

        set((state) => ({
          nodes: state.nodes.map(n => n.tag === tag ? { ...n, isTesting: true } : n)
        }))

        const cacheKey = `${activeProfileId}::${tag}`

        try {
          const latency = await window.api.node.testLatency(tag)
          const isTimeout = latency <= 0
          const latencyMs = latency > 0 ? latency : null
          
          set((state) => ({
            nodes: state.nodes.map(n => n.tag === tag
              ? { ...n, latencyMs, isTimeout, isTesting: false }
              : n
            ),
            latencyCache: {
              ...state.latencyCache,
              [cacheKey]: { latencyMs, isTimeout, timestamp: Date.now() }
            }
          }))
        } catch {
          set((state) => ({
            nodes: state.nodes.map(n => n.tag === tag 
              ? { ...n, isTesting: false, isTimeout: true, latencyMs: null } 
              : n
            ),
            latencyCache: {
              ...state.latencyCache,
              [cacheKey]: { latencyMs: null, isTimeout: true, timestamp: Date.now() }
            }
          }))
        }
      },

      loadNodes: async () => {
        const nodes = await window.api.node.list()
        const activeProfileId = await window.api.profile.getActive()
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
            return { ...n, latencyMs: cached.latencyMs, isTimeout: cached.isTimeout }
          }
          return { ...n }
        })
        
        if (nodes.length > 0 && !activeNodeTag) {
          const firstTag = nodes[0].tag
          if (firstTag) {
            await window.api.node.setActive(firstTag)
            set({ nodes: nodesWithLatency, activeNodeTag: firstTag, ...(cacheChanged ? { latencyCache: newCache } : {}) })
            return
          }
        }
        
        set({ nodes: nodesWithLatency, ...(cacheChanged ? { latencyCache: newCache } : {}) })
      },

      loadAllNodes: async () => {
        const allNodes = await window.api.node.listAll()
        const { latencyCache } = get()
        const now = Date.now()
        
        const nodesWithLatency = allNodes.map((n: NodeWithProfile) => {
          const cacheKey = n.sourceProfileId && n.tag ? `${n.sourceProfileId}::${n.tag}` : null
          const cached = cacheKey ? latencyCache[cacheKey] : null
          if (cached && now - cached.timestamp < 3600000) {
            return { ...n, latencyMs: cached.latencyMs, isTimeout: cached.isTimeout }
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
      })
    }
  )
)

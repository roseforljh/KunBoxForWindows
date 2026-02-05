import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { SingBoxOutbound } from '@shared/types'

interface NodeItem extends SingBoxOutbound {
  latencyMs?: number | null
  isTimeout?: boolean
  isTesting?: boolean
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
  activeNodeTag: string | null
  searchText: string
  sortMode: 'default' | 'latency' | 'name' | 'region'
  nodeFilter: NodeFilter
  isTesting: boolean
  testProgress: number
  testTotal: number
  latencyCache: LatencyCache

  setNodes: (nodes: SingBoxOutbound[]) => void
  setActiveNode: (tag: string | null) => void
  setSearchText: (text: string) => void
  setSortMode: (mode: NodesState['sortMode']) => void
  setNodeFilter: (filter: NodeFilter) => void
  clearNodeFilter: () => void
  selectNode: (tag: string) => Promise<void>
  testAllLatency: () => Promise<void>
  cancelTestAllLatency: () => void
  testNodeLatency: (tag: string) => Promise<void>
  loadNodes: () => Promise<void>
}

export const useNodesStore = create<NodesState>()(
  persist(
    (set, get) => ({
      nodes: [],
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

      setNodes: (nodes) => {
        const { latencyCache } = get()
        // Restore latency from cache
        const nodesWithLatency = nodes.map(n => {
          const cached = n.tag ? latencyCache[n.tag] : null
          if (cached) {
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
        // Try hot switch first via Clash API
        const result = await window.api.singbox.switchNode(tag)
        if (result.success) {
          // Hot switch succeeded
          await window.api.node.setActive(tag)
          set({ activeNodeTag: tag })
        } else {
          // Hot switch failed, just save the selection (will apply on restart)
          await window.api.node.setActive(tag)
          set({ activeNodeTag: tag })
        }
      },

      testAllLatency: async () => {
        const { nodes } = get()
        if (nodes.length === 0) return

        // Create new abort controller
        abortController = new AbortController()
        const signal = abortController.signal

        set({ isTesting: true, testProgress: 0, testTotal: nodes.length })

        // Test nodes one by one
        for (let i = 0; i < nodes.length; i++) {
          // Check if cancelled
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
            
            // Update node and cache
            set((state) => ({
              nodes: state.nodes.map(n => n.tag === node.tag
                ? { ...n, latencyMs, isTimeout }
                : n
              ),
              latencyCache: {
                ...state.latencyCache,
                [node.tag!]: { latencyMs, isTimeout, timestamp: Date.now() }
              }
            }))
          } catch {
            set((state) => ({
              nodes: state.nodes.map(n => n.tag === node.tag
                ? { ...n, latencyMs: null, isTimeout: true }
                : n
              ),
              latencyCache: {
                ...state.latencyCache,
                [node.tag!]: { latencyMs: null, isTimeout: true, timestamp: Date.now() }
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
        set((state) => ({
          nodes: state.nodes.map(n => n.tag === tag ? { ...n, isTesting: true } : n)
        }))

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
              [tag]: { latencyMs, isTimeout, timestamp: Date.now() }
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
              [tag]: { latencyMs: null, isTimeout: true, timestamp: Date.now() }
            }
          }))
        }
      },

      loadNodes: async () => {
        const nodes = await window.api.node.list()
        const { activeNodeTag, latencyCache } = get()
        
        // Restore latency from cache
        const nodesWithLatency = nodes.map(n => {
          const cached = n.tag ? latencyCache[n.tag] : null
          if (cached) {
            return { ...n, latencyMs: cached.latencyMs, isTimeout: cached.isTimeout }
          }
          return { ...n }
        })
        
        // Auto-select first node if none is active
        if (nodes.length > 0 && !activeNodeTag) {
          const firstTag = nodes[0].tag
          if (firstTag) {
            await window.api.node.setActive(firstTag)
            set({ nodes: nodesWithLatency, activeNodeTag: firstTag })
            return
          }
        }
        
        set({ nodes: nodesWithLatency })
      }
    }),
    {
      name: 'kunbox-nodes-store',
      partialize: (state) => ({ 
        latencyCache: state.latencyCache,
        activeNodeTag: state.activeNodeTag
      })
    }
  )
)

import { create } from 'zustand'
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

interface NodesState {
  nodes: NodeItem[]
  activeNodeTag: string | null
  searchText: string
  sortMode: 'default' | 'latency' | 'name' | 'region'
  nodeFilter: NodeFilter
  isTesting: boolean
  testProgress: number
  testTotal: number

  setNodes: (nodes: SingBoxOutbound[]) => void
  setActiveNode: (tag: string | null) => void
  setSearchText: (text: string) => void
  setSortMode: (mode: NodesState['sortMode']) => void
  setNodeFilter: (filter: NodeFilter) => void
  clearNodeFilter: () => void
  selectNode: (tag: string) => Promise<void>
  testAllLatency: () => Promise<void>
  testNodeLatency: (tag: string) => Promise<void>
  loadNodes: () => Promise<void>
}

export const useNodesStore = create<NodesState>((set, get) => ({
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

  setNodes: (nodes) => set({ nodes: nodes.map(n => ({ ...n })) }),
  setActiveNode: (tag) => set({ activeNodeTag: tag }),
  setSearchText: (text) => set({ searchText: text }),
  setSortMode: (mode) => set({ sortMode: mode }),
  setNodeFilter: (filter) => set({ nodeFilter: filter }),
  clearNodeFilter: () => set({ 
    nodeFilter: { filterMode: 'none', includeKeywords: [], excludeKeywords: [] } 
  }),

  selectNode: async (tag) => {
    await window.api.node.setActive(tag)
    set({ activeNodeTag: tag })
  },

  testAllLatency: async () => {
    const { nodes } = get()
    if (nodes.length === 0) return

    set({ isTesting: true, testProgress: 0, testTotal: nodes.length })

    try {
      const results = await window.api.node.testAll()
      set((state) => ({
        nodes: state.nodes.map(n => ({
          ...n,
          latencyMs: n.tag ? results[n.tag] || null : null,
          isTimeout: n.tag ? !results[n.tag] : false
        }))
      }))
    } finally {
      set({ isTesting: false })
    }
  },

  testNodeLatency: async (tag) => {
    set((state) => ({
      nodes: state.nodes.map(n => n.tag === tag ? { ...n, isTesting: true } : n)
    }))

    try {
      const latency = await window.api.node.testLatency(tag)
      set((state) => ({
        nodes: state.nodes.map(n => n.tag === tag
          ? { ...n, latencyMs: latency > 0 ? latency : null, isTimeout: latency <= 0, isTesting: false }
          : n
        )
      }))
    } catch {
      set((state) => ({
        nodes: state.nodes.map(n => n.tag === tag ? { ...n, isTesting: false, isTimeout: true } : n)
      }))
    }
  },

  loadNodes: async () => {
    const nodes = await window.api.node.list()
    set({ nodes: nodes.map(n => ({ ...n })) })
  }
}))

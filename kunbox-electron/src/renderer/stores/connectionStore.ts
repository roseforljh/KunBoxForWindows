import { create } from 'zustand'
import type { ProxyState, TrafficStats } from '@shared/types'
import { useNodesStore } from './nodesStore'

export interface TrafficPoint {
  download: number
  upload: number
}

const MAX_TRAFFIC_HISTORY_POINTS = 40

interface ConnectionState {
  state: ProxyState
  traffic: TrafficStats | null
  trafficHistory: TrafficPoint[]
  currentNodeName: string | null
  currentProfileName: string | null
  lastError: string | null
  needsRestart: boolean

  setState: (state: ProxyState) => void
  setTraffic: (traffic: TrafficStats) => void
  setCurrentNode: (name: string | null) => void
  setCurrentProfile: (name: string | null) => void
  setNeedsRestart: (needs: boolean) => void
  connect: () => Promise<{ success: boolean; error?: string; warning?: string }>
  disconnect: () => Promise<{ success: boolean; error?: string; warning?: string }>
}

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  state: 'idle',
  traffic: null,
  trafficHistory: [],
  currentNodeName: null,
  currentProfileName: null,
  lastError: null,
  needsRestart: false,

  setState: (state) => set((prev) => (
    state === 'idle' || state === 'error'
      ? { state, traffic: null, trafficHistory: [] }
      : { state, traffic: prev.traffic, trafficHistory: prev.trafficHistory }
  )),
  setTraffic: (traffic) => set((prev) => ({
    traffic,
    trafficHistory: [
      ...prev.trafficHistory,
      { download: traffic.downloadSpeed, upload: traffic.uploadSpeed }
    ].slice(-MAX_TRAFFIC_HISTORY_POINTS)
  })),
  setCurrentNode: (name) => set({ currentNodeName: name }),
  setCurrentProfile: (name) => set({ currentProfileName: name }),
  setNeedsRestart: (needs) => set({ needsRestart: needs }),

  connect: async () => {
    const { state } = get()
    if (state === 'connected' || state === 'connecting') {
      return { success: false, error: '已经在连接中' }
    }

    useNodesStore.getState().cancelTestAllLatency()
    set({ state: 'connecting', lastError: null })
    try {
      const result = await window.api.singbox.start()
      if (!result.success) {
        set({ state: 'error', lastError: result.error })
        return { success: false, error: result.error }
      }
      set({ state: 'connected', needsRestart: false })
      return { success: true, warning: result.warning }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      set({ state: 'error', lastError: error })
      return { success: false, error }
    }
  },

  disconnect: async () => {
    const { state } = get()
    if (state === 'idle' || state === 'disconnecting') {
      return { success: false, error: '未在运行中' }
    }

    useNodesStore.getState().cancelTestAllLatency()
    set({ state: 'disconnecting' })
    try {
      const result = await window.api.singbox.stop()
      if (!result.success) {
        set({ state: 'error', lastError: result.error })
        return { success: false, error: result.error }
      }
      set({ state: 'idle', traffic: null, trafficHistory: [], lastError: null, needsRestart: false })
      return { success: true }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      set({ state: 'error', lastError: error })
      return { success: false, error }
    }
  }
}))

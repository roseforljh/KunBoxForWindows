import { create } from 'zustand'
import type { ProxyState, TrafficStats } from '@shared/types'

interface ConnectionState {
  state: ProxyState
  traffic: TrafficStats | null
  currentNodeName: string | null
  currentProfileName: string | null
  lastError: string | null
  needsRestart: boolean

  setState: (state: ProxyState) => void
  setTraffic: (traffic: TrafficStats) => void
  setCurrentNode: (name: string | null) => void
  setCurrentProfile: (name: string | null) => void
  setNeedsRestart: (needs: boolean) => void
  connect: () => Promise<{ success: boolean; error?: string }>
  disconnect: () => Promise<{ success: boolean; error?: string }>
}

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  state: 'idle',
  traffic: null,
  currentNodeName: null,
  currentProfileName: null,
  lastError: null,
  needsRestart: false,

  setState: (state) => set({ state }),
  setTraffic: (traffic) => set({ traffic }),
  setCurrentNode: (name) => set({ currentNodeName: name }),
  setCurrentProfile: (name) => set({ currentProfileName: name }),
  setNeedsRestart: (needs) => set({ needsRestart: needs }),

  connect: async () => {
    const { state } = get()
    if (state === 'connected' || state === 'connecting') {
      return { success: false, error: '已经在连接中' }
    }

    set({ state: 'connecting', lastError: null })
    try {
      const result = await window.api.singbox.start()
      if (!result.success) {
        set({ state: 'error', lastError: result.error })
        return { success: false, error: result.error }
      }
      set({ state: 'connected', needsRestart: false })
      return { success: true }
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

    set({ state: 'disconnecting' })
    try {
      const result = await window.api.singbox.stop()
      if (!result.success) {
        set({ state: 'error', lastError: result.error })
        return { success: false, error: result.error }
      }
      set({ state: 'idle', traffic: null, lastError: null, needsRestart: false })
      return { success: true }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      set({ state: 'error', lastError: error })
      return { success: false, error }
    }
  }
}))

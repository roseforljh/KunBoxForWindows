import { create } from 'zustand'
import type { ProxyState, TrafficStats } from '@shared/types'

interface ConnectionState {
  state: ProxyState
  traffic: TrafficStats | null
  currentNodeName: string | null
  currentProfileName: string | null

  setState: (state: ProxyState) => void
  setTraffic: (traffic: TrafficStats) => void
  setCurrentNode: (name: string | null) => void
  setCurrentProfile: (name: string | null) => void
  connect: () => Promise<void>
  disconnect: () => Promise<void>
}

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  state: 'idle',
  traffic: null,
  currentNodeName: null,
  currentProfileName: null,

  setState: (state) => set({ state }),
  setTraffic: (traffic) => set({ traffic }),
  setCurrentNode: (name) => set({ currentNodeName: name }),
  setCurrentProfile: (name) => set({ currentProfileName: name }),

  connect: async () => {
    const { state } = get()
    if (state === 'connected' || state === 'connecting') return

    set({ state: 'connecting' })
    try {
      await window.api.singbox.start()
    } catch {
      set({ state: 'error' })
    }
  },

  disconnect: async () => {
    const { state } = get()
    if (state === 'idle' || state === 'disconnecting') return

    set({ state: 'disconnecting' })
    try {
      await window.api.singbox.stop()
      set({ state: 'idle', traffic: null })
    } catch {
      set({ state: 'error' })
    }
  }
}))

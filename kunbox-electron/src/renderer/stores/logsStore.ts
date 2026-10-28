import { create } from 'zustand'
import type { LogEntry } from '@shared/types'

const MAX_LOGS = 500
const MAX_PENDING_LOGS = 1000

interface LogsState {
  logs: LogEntry[]
  logsEnabled: boolean
  appendLog: (entry: LogEntry) => void
  flushLogs: () => void
  clearLogs: () => void
  setLogsEnabled: (enabled: boolean) => void
}

export const useLogsStore = create<LogsState>((set, get) => {
  const pendingLogs: LogEntry[] = []

  return {
    logs: [],
    logsEnabled: true,
    appendLog: (entry) => {
      if (!get().logsEnabled) return
      pendingLogs.push(entry)
      if (pendingLogs.length > MAX_PENDING_LOGS) {
        pendingLogs.splice(0, pendingLogs.length - MAX_PENDING_LOGS)
      }
    },
    flushLogs: () => {
      if (pendingLogs.length === 0) return
      const chunk = pendingLogs.splice(0)
      set((state) => ({ logs: [...state.logs, ...chunk].slice(-MAX_LOGS) }))
    },
    clearLogs: () => {
      pendingLogs.length = 0
      set({ logs: [] })
    },
    setLogsEnabled: (logsEnabled) => {
      get().flushLogs()
      set({ logsEnabled })
    },
  }
})

// Owned by the app so navigation never interrupts log collection.
export function subscribeRuntimeLogs(): () => void {
  const { appendLog, flushLogs, setLogsEnabled } = useLogsStore.getState()
  let active = true
  let settingChanged = false

  const onRuntimeLogsChange = (event: Event) => {
    const enabled = (event as CustomEvent<{ enabled: boolean }>).detail?.enabled
    if (typeof enabled !== 'boolean') return
    settingChanged = true
    setLogsEnabled(enabled)
  }

  window.addEventListener('runtime-logs-change', onRuntimeLogsChange)
  const unsubscribe = window.api.singbox.onLog((entry) => {
    if (active) appendLog(entry)
  })
  const flushTimer = window.setInterval(flushLogs, 150)

  void window.api.settings.get().then((settings) => {
    if (active && !settingChanged) setLogsEnabled(settings.enableRuntimeLogs)
  }).catch((err) => {
    console.error('Failed to load runtime log setting:', err)
  })

  return () => {
    active = false
    unsubscribe()
    window.clearInterval(flushTimer)
    window.removeEventListener('runtime-logs-change', onRuntimeLogsChange)
    flushLogs()
  }
}

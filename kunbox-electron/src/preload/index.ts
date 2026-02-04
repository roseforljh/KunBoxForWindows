import { contextBridge, ipcRenderer } from 'electron'
import { IPC_CHANNELS } from '../shared/constants'
import type { AppSettings, Profile, SingBoxOutbound, ProxyState, TrafficStats, LogEntry } from '../shared/types'

const api = {
  singbox: {
    start: () => ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_START),
    stop: () => ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_STOP),
    restart: () => ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_RESTART),
    onStateChange: (callback: (state: ProxyState) => void) => {
      const handler = (_: unknown, state: ProxyState) => callback(state)
      ipcRenderer.on(IPC_CHANNELS.SINGBOX_STATE, handler)
      return () => ipcRenderer.removeListener(IPC_CHANNELS.SINGBOX_STATE, handler)
    },
    onTraffic: (callback: (stats: TrafficStats) => void) => {
      const handler = (_: unknown, stats: TrafficStats) => callback(stats)
      ipcRenderer.on(IPC_CHANNELS.SINGBOX_TRAFFIC, handler)
      return () => ipcRenderer.removeListener(IPC_CHANNELS.SINGBOX_TRAFFIC, handler)
    },
    onLog: (callback: (entry: LogEntry) => void) => {
      const handler = (_: unknown, entry: LogEntry) => callback(entry)
      ipcRenderer.on(IPC_CHANNELS.SINGBOX_LOG, handler)
      return () => ipcRenderer.removeListener(IPC_CHANNELS.SINGBOX_LOG, handler)
    }
  },

  profile: {
    list: (): Promise<Profile[]> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_LIST),
    add: (url: string, name?: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_ADD, url, name, settings),
    importContent: (name: string, content: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_IMPORT_CONTENT, name, content, settings),
    update: (id: string): Promise<Profile> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_UPDATE, id),
    delete: (id: string): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_DELETE, id),
    setActive: (id: string): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_SET_ACTIVE, id),
    refresh: (id: string): Promise<Profile> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_REFRESH, id),
    edit: (id: string, data: { name: string; url: string; autoUpdateInterval?: number; dnsPreResolve?: boolean; dnsServer?: string | null }): Promise<Profile> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_EDIT, id, data),
    setEnabled: (id: string, enabled: boolean): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_SET_ENABLED, id, enabled)
  },

  node: {
    list: (): Promise<SingBoxOutbound[]> => ipcRenderer.invoke(IPC_CHANNELS.NODE_LIST),
    setActive: (tag: string): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.NODE_SET_ACTIVE, tag),
    add: (link: string, target?: { type: 'existing'; profileId: string } | { type: 'new'; profileName: string }): Promise<SingBoxOutbound> => ipcRenderer.invoke(IPC_CHANNELS.NODE_ADD, link, target),
    testLatency: (tag: string): Promise<number> => ipcRenderer.invoke(IPC_CHANNELS.NODE_TEST_LATENCY, tag),
    testAll: (): Promise<Record<string, number>> => ipcRenderer.invoke(IPC_CHANNELS.NODE_TEST_ALL),
    delete: (tag: string): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.NODE_DELETE, tag),
    export: (tag: string): Promise<string> => ipcRenderer.invoke(IPC_CHANNELS.NODE_EXPORT, tag)
  },

  settings: {
    get: (): Promise<AppSettings> => ipcRenderer.invoke(IPC_CHANNELS.SETTINGS_GET),
    set: (settings: Partial<AppSettings>): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.SETTINGS_SET, settings)
  },

  kernel: {
    getLocalVersion: (isAlpha?: boolean) => ipcRenderer.invoke('kernel:get-local-version', isAlpha),
    getInstalledVersions: () => ipcRenderer.invoke('kernel:get-installed-versions'),
    getRemoteReleases: (includePrerelease?: boolean) => ipcRenderer.invoke('kernel:get-remote-releases', includePrerelease),
    download: (release: any, isAlpha?: boolean) => ipcRenderer.invoke('kernel:download', release, isAlpha),
    rollback: (isAlpha?: boolean) => ipcRenderer.invoke('kernel:rollback', isAlpha),
    canRollback: (isAlpha?: boolean) => ipcRenderer.invoke('kernel:can-rollback', isAlpha),
    clearCache: () => ipcRenderer.invoke('kernel:clear-cache'),
    openReleasesPage: () => ipcRenderer.invoke('kernel:open-releases-page'),
    openDirectory: () => ipcRenderer.invoke('kernel:open-directory'),
    onDownloadProgress: (callback: (progress: { downloaded: number; total: number; percent: number }) => void) => {
      const handler = (_: unknown, progress: any) => callback(progress)
      ipcRenderer.on('kernel:download-progress', handler)
      return () => ipcRenderer.removeListener('kernel:download-progress', handler)
    },
    onDownloadComplete: (callback: () => void) => {
      const handler = () => callback()
      ipcRenderer.on('kernel:download-complete', handler)
      return () => ipcRenderer.removeListener('kernel:download-complete', handler)
    },
    onDownloadError: (callback: (error: string) => void) => {
      const handler = (_: unknown, error: string) => callback(error)
      ipcRenderer.on('kernel:download-error', handler)
      return () => ipcRenderer.removeListener('kernel:download-error', handler)
    }
  },

  window: {
    minimize: () => ipcRenderer.send(IPC_CHANNELS.WINDOW_MINIMIZE),
    maximize: () => ipcRenderer.send(IPC_CHANNELS.WINDOW_MAXIMIZE),
    close: () => ipcRenderer.send(IPC_CHANNELS.WINDOW_CLOSE)
  }
}

contextBridge.exposeInMainWorld('api', api)

export type API = typeof api

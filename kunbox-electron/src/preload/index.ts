import { contextBridge, ipcRenderer } from 'electron'
import { IPC_CHANNELS } from '../shared/constants'
import type {
  AppSettings,
  Profile,
  SingBoxOutbound,
  ProxyState,
  TrafficStats,
  LogEntry,
  DomainRule,
  CustomRules,
  NodeWithProfile,
  NodeLatencyResult,
  HealthEvent,
  CustomProfileNodeSelection,
  RuleSet
} from '../shared/types'

function onEvent<T = any>(channel: string, callback: (data: T) => void): () => void {
  const handler = (_: unknown, data: T) => callback(data)
  ipcRenderer.on(channel, handler)
  return () => {
    ipcRenderer.removeListener(channel, handler)
  }
}

const api = {
  singbox: {
    start: (): Promise<{ success: boolean; error?: string; warning?: string }> =>
      ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_START),
    stop: (): Promise<{ success: boolean; error?: string; warning?: string }> =>
      ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_STOP),
    restart: (): Promise<{ success: boolean; error?: string; warning?: string }> =>
      ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_RESTART),
    switchNode: (nodeTag: string): Promise<{ success: boolean; error?: string }> =>
      ipcRenderer.invoke(IPC_CHANNELS.SINGBOX_SWITCH_NODE, nodeTag),
    enableSystemProxy: (port?: number): Promise<{ success: boolean; error?: string }> =>
      ipcRenderer.invoke('singbox:enable-system-proxy', port),
    disableSystemProxy: (): Promise<{ success: boolean; error?: string }> =>
      ipcRenderer.invoke('singbox:disable-system-proxy'),
    onStateChange: (callback: (state: ProxyState) => void) =>
      onEvent<ProxyState>('singbox:state', callback),
    onTraffic: (callback: (stats: TrafficStats) => void) =>
      onEvent<TrafficStats>('singbox:traffic', callback),
    onLog: (callback: (entry: LogEntry) => void) =>
      onEvent<LogEntry>('singbox:log', callback),
    testSelectorLatency: (selectorTag: string, testUrl?: string): Promise<{
      success: boolean
      selector: string
      total: number
      tested: number
      timeout: number
      bestNode?: string
      bestDelay?: number
    }> => ipcRenderer.invoke('singbox:test-selector-latency', selectorTag, testUrl),
    onSelectorSwitch: (callback: (data: { selector: string; node: string; delay: number; stage: 'first' | 'final' }) => void) =>
      onEvent('singbox:selector-switch', callback),
    onHealthEvent: (callback: (data: HealthEvent) => void) =>
      onEvent<HealthEvent>('singbox:health', callback)
  },

  tray: {
    updateStatus: (connected: boolean) =>
      ipcRenderer.send('tray:status-update', { connected }),
    onVpnStart: (callback: () => void) =>
      onEvent('tray-vpn-start', callback),
    onVpnStop: (callback: () => void) =>
      onEvent('tray-vpn-stop', callback),
    onVpnRestart: (callback: () => void) =>
      onEvent('tray-vpn-restart', callback),
    onProxyEnable: (callback: () => void) =>
      onEvent('tray-proxy-enable', callback),
    onProxyDisable: (callback: () => void) =>
      onEvent('tray-proxy-disable', callback),
    onTunEnable: (callback: () => void) =>
      onEvent('tray-tun-enable', callback),
    onTunDisable: (callback: () => void) =>
      onEvent('tray-tun-disable', callback),
    onQuit: (callback: () => void) =>
      onEvent('tray-quit', callback)
  },

  profile: {
    list: (): Promise<Profile[]> => ipcRenderer.invoke(IPC_CHANNELS.PROFILE_LIST),
    add: (url: string, name?: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> =>
      ipcRenderer.invoke('profile:add', url, name, settings),
    importContent: (name: string, content: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }): Promise<Profile> =>
      ipcRenderer.invoke('profile:import-content', name, content, settings),
    createCustom: (name: string, selections: CustomProfileNodeSelection[], newNodeLinks: string[] = []): Promise<Profile> =>
      ipcRenderer.invoke('profile:create-custom', name, selections, newNodeLinks),
    update: (id: string): Promise<Profile> => ipcRenderer.invoke('profile:update', id),
    delete: (id: string): Promise<void> => ipcRenderer.invoke('profile:delete', id),
    getActive: (): Promise<string | null> => ipcRenderer.invoke('profile:get-active'),
    setActive: (id: string): Promise<void> => ipcRenderer.invoke('profile:set-active', id),
    refresh: (id: string): Promise<Profile> => ipcRenderer.invoke('profile:update', id),
    edit: (id: string, data: { name: string; url: string; autoUpdateInterval?: number; dnsPreResolve?: boolean; dnsServer?: string | null }): Promise<Profile> =>
      ipcRenderer.invoke('profile:edit', id, data),
    setEnabled: (id: string, enabled: boolean): Promise<void> => ipcRenderer.invoke('profile:set-enabled', id, enabled)
  },

  node: {
    list: (): Promise<SingBoxOutbound[]> => ipcRenderer.invoke(IPC_CHANNELS.NODE_LIST),
    getActive: (): Promise<string | null> => ipcRenderer.invoke('node:get-active'),
    setActive: (tag: string): Promise<void> => ipcRenderer.invoke('node:set-active', tag),
    add: (link: string, target?: { type: 'existing'; profileId: string } | { type: 'new'; profileName: string }): Promise<SingBoxOutbound> =>
      ipcRenderer.invoke('node:add', link, target),
    beginLatencyTests: (_runId: number): Promise<void> => ipcRenderer.invoke('node:begin-latency-tests'),
    testLatency: (tag: string, runId?: number): Promise<NodeLatencyResult> =>
      ipcRenderer.invoke('node:test-latency', tag, runId),
    testAll: (): Promise<Record<string, number>> => ipcRenderer.invoke('node:test-all'),
    cancelLatencyTests: (_runId?: number): Promise<void> => ipcRenderer.invoke('node:cancel-latency-tests'),
    delete: (tag: string): Promise<void> => ipcRenderer.invoke('node:delete', tag),
    update: (profileId: string, originalTag: string, node: SingBoxOutbound): Promise<SingBoxOutbound> =>
      ipcRenderer.invoke('node:update', profileId, originalTag, node),
    export: (tag: string, _node?: SingBoxOutbound): Promise<string> =>
      ipcRenderer.invoke('node:export', tag),
    listAll: (includeDisabled?: boolean): Promise<NodeWithProfile[]> =>
      ipcRenderer.invoke('node:list-all', includeDisabled)
  },

  settings: {
    get: (): Promise<AppSettings> => ipcRenderer.invoke(IPC_CHANNELS.SETTINGS_GET),
    set: (settings: Partial<AppSettings>): Promise<void> => ipcRenderer.invoke(IPC_CHANNELS.SETTINGS_SET, settings),
    onChange: (callback: (settings: AppSettings) => void) =>
      onEvent(IPC_CHANNELS.SETTINGS_CHANGED, callback),
    randomAvailablePorts: (): Promise<[number, number]> => ipcRenderer.invoke('settings:random-available-ports')
  },

  kernel: {
    getLocalVersion: async (isAlpha?: boolean) =>
      ipcRenderer.invoke('kernel:get-local-version', isAlpha),
    getInstalledVersions: () =>
      ipcRenderer.invoke('kernel:get-installed-versions'),
    getCapabilities: () =>
      ipcRenderer.invoke('kernel:get-capabilities'),
    getRemoteReleases: async (includePrerelease?: boolean) =>
      ipcRenderer.invoke('kernel:get-remote-releases', includePrerelease),
    download: (tagName: string) =>
      ipcRenderer.invoke('kernel:download', tagName),
    rollback: (isAlpha?: boolean) =>
      ipcRenderer.invoke('kernel:rollback', isAlpha),
    canRollback: (isAlpha?: boolean) =>
      ipcRenderer.invoke('kernel:can-rollback', isAlpha),
    clearCache: () =>
      ipcRenderer.invoke('kernel:clear-cache'),
    openReleasesPage: () =>
      ipcRenderer.invoke('kernel:open-releases-page'),
    openDirectory: () =>
      ipcRenderer.invoke('kernel:open-directory'),
    onDownloadProgress: (callback: (progress: { phase?: string; downloaded: number; total: number; percent: number }) => void) =>
      onEvent('kernel:download-progress', callback),
    onDownloadComplete: (callback: () => void) =>
      onEvent('kernel:download-complete', callback),
    onDownloadError: (callback: (error: string) => void) =>
      onEvent('kernel:download-error', callback)
  },

  ruleset: {
    list: (): Promise<RuleSet[]> => ipcRenderer.invoke(IPC_CHANNELS.RULESET_LIST),
    save: (ruleSets: RuleSet[]) => ipcRenderer.invoke(IPC_CHANNELS.RULESET_SAVE, ruleSets),
    download: (ruleSet: RuleSet, force = false) => ipcRenderer.invoke(IPC_CHANNELS.RULESET_DOWNLOAD, ruleSet, force),
    isCached: (tag: string): Promise<boolean> => ipcRenderer.invoke(IPC_CHANNELS.RULESET_IS_CACHED, tag),
    fetchHub: () => ipcRenderer.invoke('ruleset:fetch-hub')
  },

  customRules: {
    get: (): Promise<CustomRules> => ipcRenderer.invoke('custom-rules:get'),
    save: (rules: CustomRules): Promise<void> => ipcRenderer.invoke('custom-rules:save', rules),
    getDomainRules: (): Promise<DomainRule[]> => ipcRenderer.invoke('domain-rules:get'),
    saveDomainRules: (rules: DomainRule[]): Promise<void> => ipcRenderer.invoke('domain-rules:save', rules)
  },

  updater: {
    getCurrentVersion: (): Promise<string> => ipcRenderer.invoke('updater:get-current-version'),
    check: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> =>
      ipcRenderer.invoke('updater:check'),
    downloadAndInstall: (): Promise<{ currentVersion: string; hasUpdate: boolean; version?: string; date?: string; body?: string }> =>
      ipcRenderer.invoke('updater:download-and-install'),
    onDownloadProgress: (callback: (progress: { downloaded: number; contentLength: number }) => void) =>
      onEvent('updater:download-progress', callback),
    onDownloadFinished: (callback: () => void) =>
      onEvent('updater:download-finished', callback)
  },

  window: {
    minimize: () => ipcRenderer.invoke('window:minimize'),
    maximize: () => ipcRenderer.invoke('window:maximize'),
    close: () => ipcRenderer.invoke('window:close'),
    show: () => ipcRenderer.invoke('window:show'),
    restartAsAdmin: () => ipcRenderer.invoke('window:restart-as-admin'),
    isAdmin: (): Promise<boolean> => ipcRenderer.invoke('window:is-admin')
  }
}

contextBridge.exposeInMainWorld('api', api)

export type API = typeof api

// Mock API for browser-only environments (dev/QA testing without Tauri/Electron)
import type { AppSettings, Profile, SingBoxOutbound, TrafficStats, LogEntry, DomainRule, CustomRules, NodeWithProfile } from './types'

const noop = () => {}
const noopUnlisten = () => noop

const defaultSettings: AppSettings = {
  localPort: 7890,
  socksPort: 7891,
  allowLan: false,
  systemProxy: false,
  tunEnabled: false,
  tunStack: 'mixed',
  localDns: '223.5.5.5',
  remoteDns: 'https://dns.google/dns-query',
  fakeDns: false,
  bypassLan: true,
  routingMode: 'rule',
  defaultRule: 'proxy',
  latencyTestUrl: 'https://www.gstatic.com/generate_204',
  latencyTestTimeout: 3000,
  autoConnect: false,
  minimizeToTray: true,
  startWithWindows: false,
  startMinimized: false,
  silentStart: false,
  exitOnClose: false,
  theme: 'dark',
  requireAdmin: false,
  enableRuntimeLogs: true,
}

const mockProfiles: Profile[] = [
  {
    id: 'mock-profile-1',
    name: '示例订阅',
    url: 'https://example.com/subscribe',
    lastUpdate: Date.now() - 3600000,
    nodeCount: 5,
    enabled: true,
    autoUpdateInterval: 60,
    dnsPreResolve: false,
    dnsServer: null,
  },
]

const mockNodes: SingBoxOutbound[] = [
  { tag: '🇭🇰 香港 01', type: 'shadowsocks', server: 'hk1.example.com', server_port: 443 },
  { tag: '🇯🇵 日本 01', type: 'vmess', server: 'jp1.example.com', server_port: 443 },
  { tag: '🇺🇸 美国 01', type: 'trojan', server: 'us1.example.com', server_port: 443 },
  { tag: '🇸🇬 新加坡 01', type: 'vless', server: 'sg1.example.com', server_port: 443 },
  { tag: '🇹🇼 台湾 01', type: 'shadowsocks', server: 'tw1.example.com', server_port: 443 },
]

const mockNodesWithProfile: NodeWithProfile[] = mockNodes.map((n) => ({
  ...n,
  sourceProfileId: 'mock-profile-1',
  sourceProfileName: '示例订阅',
}))

export const mockApi = {
  singbox: {
    start: () => Promise.resolve({ success: true }),
    stop: () => Promise.resolve({ success: true }),
    restart: () => Promise.resolve({ success: true }),
    switchNode: (_nodeTag: string) => Promise.resolve({ success: true }),
    enableSystemProxy: (_port?: number) => Promise.resolve({ success: true }),
    disableSystemProxy: () => Promise.resolve({ success: true }),
    onStateChange: (_callback: (state: any) => void) => noopUnlisten(),
    onTraffic: (_callback: (stats: TrafficStats) => void) => noopUnlisten(),
    onLog: (_callback: (entry: LogEntry) => void) => noopUnlisten(),
    testSelectorLatency: (_selectorTag: string, _testUrl?: string) =>
      Promise.resolve({ success: true, selector: _selectorTag, total: 5, tested: 5, timeout: 0 }),
    onSelectorSwitch: (_callback: (data: any) => void) => noopUnlisten(),
  },

  tray: {
    updateStatus: (_connected: boolean) => {},
    onVpnStart: (_callback: () => void) => noopUnlisten(),
    onVpnStop: (_callback: () => void) => noopUnlisten(),
    onVpnRestart: (_callback: () => void) => noopUnlisten(),
    onProxyEnable: (_callback: () => void) => noopUnlisten(),
    onProxyDisable: (_callback: () => void) => noopUnlisten(),
    onTunEnable: (_callback: () => void) => noopUnlisten(),
    onTunDisable: (_callback: () => void) => noopUnlisten(),
    onQuit: (_callback: () => void) => noopUnlisten(),
  },

  profile: {
    list: (): Promise<Profile[]> => Promise.resolve([...mockProfiles]),
    add: (_url: string, _name?: string, _settings?: any): Promise<Profile> =>
      Promise.resolve({
        id: `mock-profile-${Date.now()}`,
        name: _name || '新订阅',
        url: _url,
        lastUpdate: Date.now(),
        nodeCount: 0,
        enabled: true,
        autoUpdateInterval: 0,
        dnsPreResolve: false,
        dnsServer: null,
      }),
    importContent: (_name: string, _content: string, _settings?: any): Promise<Profile> =>
      Promise.resolve({
        id: `mock-profile-${Date.now()}`,
        name: _name,
        url: '',
        lastUpdate: Date.now(),
        nodeCount: 0,
        enabled: true,
        autoUpdateInterval: 0,
        dnsPreResolve: false,
        dnsServer: null,
      }),
    update: (_id: string): Promise<Profile> =>
      Promise.resolve(mockProfiles[0]),
    delete: (_id: string): Promise<void> => Promise.resolve(),
    getActive: (): Promise<string | null> => Promise.resolve('mock-profile-1'),
    setActive: (_id: string): Promise<void> => Promise.resolve(),
    refresh: (_id: string): Promise<Profile> => Promise.resolve(mockProfiles[0]),
    edit: (_id: string, _data: any): Promise<Profile> => Promise.resolve(mockProfiles[0]),
    setEnabled: (_id: string, _enabled: boolean): Promise<void> => Promise.resolve(),
  },

  node: {
    list: (): Promise<SingBoxOutbound[]> => Promise.resolve([...mockNodes]),
    setActive: (_tag: string): Promise<void> => Promise.resolve(),
    add: (_link: string, _target?: any): Promise<SingBoxOutbound> =>
      Promise.resolve({ tag: '新节点', type: 'shadowsocks', server: 'new.example.com', server_port: 443 }),
    testLatency: (_tag: string): Promise<number> => Promise.resolve(Math.floor(Math.random() * 200) + 50),
    testAll: (): Promise<Record<string, number>> => {
      const result: Record<string, number> = {}
      mockNodes.forEach((n) => {
        result[n.tag!] = Math.floor(Math.random() * 300) + 30
      })
      return Promise.resolve(result)
    },
    delete: (_tag: string): Promise<void> => Promise.resolve(),
    export: (_tag: string): Promise<string> => Promise.resolve('ss://mock-exported-link'),
    listAll: (): Promise<NodeWithProfile[]> => Promise.resolve([...mockNodesWithProfile]),
  },

  settings: {
    get: (): Promise<AppSettings> => Promise.resolve({ ...defaultSettings }),
    set: (_settings: Partial<AppSettings>): Promise<void> => Promise.resolve(),
  },

  kernel: {
    getLocalVersion: async (_isAlpha?: boolean) => ({
      version: '1.11.0',
      versionDetail: 'sing-box 1.11.0 (mock)',
      isAlpha: false,
    }),
    getInstalledVersions: () =>
      Promise.resolve([
        { version: '1.11.0', versionDetail: 'sing-box 1.11.0', isBackup: false, path: '/mock/path' },
      ]),
    getCapabilities: () =>
      Promise.resolve({
        version: '1.11.0',
        supportsNaive: false,
        supportsIcmpProxy: false,
        supportsBypassAction: true,
      }),
    getRemoteReleases: async (_includePrerelease?: boolean) => [
      {
        version: '1.12.0',
        tagName: 'v1.12.0',
        publishedAt: new Date().toISOString(),
        isPrerelease: false,
        downloadUrl: 'https://example.com/release',
        assetName: 'sing-box-1.12.0-windows-amd64.zip',
      },
    ],
    download: (_release: any, _isAlpha?: boolean) => Promise.resolve({ success: true }),
    rollback: (_isAlpha?: boolean) => Promise.resolve({ success: true }),
    canRollback: (_isAlpha?: boolean) => Promise.resolve(false),
    clearCache: () => Promise.resolve({ success: true, freedBytes: 0 }),
    openReleasesPage: () => Promise.resolve(),
    openDirectory: () => Promise.resolve(),
    onDownloadProgress: (_callback: (progress: any) => void) => noopUnlisten(),
    onDownloadComplete: (_callback: () => void) => noopUnlisten(),
    onDownloadError: (_callback: (error: string) => void) => noopUnlisten(),
  },

  ruleset: {
    list: () => Promise.resolve([]),
    save: (_ruleSets: any[]) => Promise.resolve(),
    download: (_ruleSet: any) => Promise.resolve({ success: true }),
    isCached: (_tag: string) => Promise.resolve(false),
    fetchHub: () => Promise.resolve({ tree: [] }),
  },

  customRules: {
    get: (): Promise<CustomRules> => Promise.resolve({ domainRules: [] }),
    save: (_rules: CustomRules): Promise<void> => Promise.resolve(),
    getDomainRules: (): Promise<DomainRule[]> => Promise.resolve([]),
    saveDomainRules: (_rules: DomainRule[]): Promise<void> => Promise.resolve(),
  },

  updater: {
    getCurrentVersion: (): Promise<string> => Promise.resolve('0.1.0-dev'),
    check: () =>
      Promise.resolve({ currentVersion: '0.1.0-dev', hasUpdate: false }),
    downloadAndInstall: () =>
      Promise.resolve({ currentVersion: '0.1.0-dev', hasUpdate: false }),
    onDownloadProgress: (_callback: (progress: any) => void) => noopUnlisten(),
    onDownloadFinished: (_callback: () => void) => noopUnlisten(),
  },

  window: {
    minimize: () => Promise.resolve(),
    maximize: () => Promise.resolve(),
    close: () => Promise.resolve(),
    listRunningProcesses: (): Promise<string[]> => Promise.resolve([]),
    restartAsAdmin: () => Promise.resolve(),
    isAdmin: (): Promise<boolean> => Promise.resolve(false),
    quit: () => Promise.resolve(),
  },
}

export function initMockApi() {
  ;(window as any).api = mockApi
}

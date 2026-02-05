export type ProxyState = 'idle' | 'connecting' | 'connected' | 'disconnecting' | 'error'

export type LogLevel = 'debug' | 'info' | 'warn' | 'error'

export interface LogEntry {
  timestamp: number
  level: LogLevel
  tag: string
  message: string
}

export interface TrafficStats {
  uploadSpeed: number
  downloadSpeed: number
  uploadTotal: number
  downloadTotal: number
  duration: number
}

export interface Profile {
  id: string
  name: string
  url: string
  lastUpdate?: number
  nodeCount: number
  enabled: boolean
  autoUpdateInterval: number // 0 means disabled, minutes
  dnsPreResolve: boolean
  dnsServer: string | null
}

export interface SingBoxOutbound {
  tag?: string
  type?: string
  server?: string
  server_port?: number
  method?: string
  password?: string
  uuid?: string
  flow?: string
  security?: string
  alter_id?: number
  packet_encoding?: string
  tls?: {
    enabled?: boolean
    server_name?: string
    insecure?: boolean
    alpn?: string[]
    utls?: {
      enabled?: boolean
      fingerprint?: string
    }
    reality?: {
      enabled?: boolean
      public_key?: string
      short_id?: string
    }
  }
  transport?: {
    type?: string
    path?: string
    headers?: Record<string, string>
    host?: string[]
    service_name?: string
    max_early_data?: number
    early_data_header_name?: string
  }
  multiplex?: object
}

export interface AppSettings {
  localPort: number
  socksPort: number
  allowLan: boolean
  systemProxy: boolean
  tunEnabled: boolean
  tunStack: 'system' | 'gvisor' | 'mixed'
  localDns: string
  remoteDns: string
  fakeDns: boolean
  blockAds: boolean
  bypassLan: boolean
  routingMode: 'rule' | 'global-proxy' | 'global-direct'
  defaultRule: 'direct' | 'proxy' | 'block'
  latencyTestUrl: string
  latencyTestTimeout: number
  autoConnect: boolean
  minimizeToTray: boolean
  startWithWindows: boolean
  startMinimized: boolean
  exitOnClose: boolean
  theme: 'dark' | 'light' | 'system'
}

export interface RuleSet {
  id: string
  tag: string
  name: string
  type: 'remote' | 'local'
  format: 'source' | 'binary'
  url?: string
  path?: string
  enabled: boolean
  outboundMode: 'direct' | 'proxy' | 'block'
  isBuiltIn?: boolean
}

export const DEFAULT_SETTINGS: AppSettings = {
  localPort: 7890,
  socksPort: 7891,
  allowLan: false,
  systemProxy: true,
  tunEnabled: false,
  tunStack: 'mixed',
  localDns: '223.5.5.5',
  remoteDns: 'https://dns.google/dns-query',
  fakeDns: false,
  blockAds: false,
  bypassLan: true,
  routingMode: 'rule',
  defaultRule: 'proxy',
  latencyTestUrl: 'https://www.gstatic.com/generate_204',
  latencyTestTimeout: 5000,
  autoConnect: false,
  minimizeToTray: true,
  startWithWindows: false,
  startMinimized: false,
  exitOnClose: false,
  theme: 'dark'
}

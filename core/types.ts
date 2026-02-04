// Service state machine
export type ServiceState = 'idle' | 'starting' | 'running' | 'stopping' | 'error'

// Proxy modes
export type ProxyMode = 'rule' | 'global' | 'direct'

// Service status snapshot
export interface ServiceStatus {
  state: ServiceState
  pid: number | null
  startTime: number | null
  lastError: string | null
  configPath: string | null
  mode: ProxyMode
}

// Real-time traffic data
export interface TrafficSnapshot {
  uploadSpeed: number
  downloadSpeed: number
  uploadTotal: number
  downloadTotal: number
  connections: number
}

// Individual connection info
export interface ConnectionInfo {
  id: string
  network: string
  type: string
  source: string
  destination: string
  host: string
  rule: string
  rulePayload: string
  chains: string[]
  upload: number
  download: number
  start: number
}

// Windows system proxy settings
export interface SystemProxyConfig {
  enabled: boolean
  server: string
  port: number
  bypass: string[]
}

// Core executable config
export interface CoreConfig {
  execPath: string
  configDir: string
  workDir: string
  apiHost: string
  apiPort: number
  apiSecret: string
}

// Start options
export interface StartOptions {
  configPath?: string
  configContent?: string
  cleanCache?: boolean
}

// Stop options
export interface StopOptions {
  force?: boolean
  timeout?: number
  clearProxy?: boolean
}

// Log entry
export interface LogEntry {
  timestamp: number
  level: 'debug' | 'info' | 'warn' | 'error'
  tag: string
  message: string
}

// Event types
export interface ServiceEvents {
  stateChange: (newState: ServiceState, oldState: ServiceState) => void
  log: (entry: LogEntry) => void
  traffic: (snapshot: TrafficSnapshot) => void
  started: () => void
  stopped: () => void
  error: (error: string) => void
  unexpectedExit: (code: number | null, signal: string | null) => void
}

// Default values
export const DEFAULT_CORE_CONFIG: CoreConfig = {
  execPath: '',
  configDir: '',
  workDir: '',
  apiHost: '127.0.0.1',
  apiPort: 9090,
  apiSecret: ''
}

export const DEFAULT_BYPASS_LIST = [
  'localhost',
  '127.*',
  '10.*',
  '172.16.*',
  '172.17.*',
  '172.18.*',
  '172.19.*',
  '172.20.*',
  '172.21.*',
  '172.22.*',
  '172.23.*',
  '172.24.*',
  '172.25.*',
  '172.26.*',
  '172.27.*',
  '172.28.*',
  '172.29.*',
  '172.30.*',
  '172.31.*',
  '192.168.*',
  '<local>'
]

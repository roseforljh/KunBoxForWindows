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

export type HealthStatus = 'unknown' | 'healthy' | 'suspect' | 'failed' | 'recovering'

export type HealthEventKind =
  | 'selector_failed_over'
  | 'selector_no_backup'
  | 'fixed_node_failed'
  | 'main_node_needs_manual_switch'

export interface HealthEvent {
  kind: HealthEventKind
  selector?: string | null
  from?: string | null
  to?: string | null
  node?: string | null
  rule?: string | null
  message: string
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

export interface OutboundTls {
  enabled?: boolean
  server_name?: string
  insecure?: boolean
  disable_sni?: boolean
  alpn?: string[]
  ca?: string[]
  certificate?: string[]
  key?: string[]
  utls?: {
    enabled?: boolean
    fingerprint?: string
  }
  reality?: {
    enabled?: boolean
    public_key?: string
    short_id?: string
  }
  ech?: {
    enabled?: boolean
    config?: string[]
  }
}

export interface OutboundTransport {
  type?: string
  path?: string
  headers?: Record<string, string | string[]>
  host?: string[]
  service_name?: string
  max_early_data?: number
  early_data_header_name?: string
  mode?: string
  x_padding_bytes?: string
  sc_max_each_post_bytes?: number
  sc_min_posts_interval_ms?: number
  sc_max_buffered_posts?: number
  no_grpc_header?: boolean
  no_sse_header?: boolean
}

export interface OutboundMultiplex {
  enabled?: boolean
  protocol?: string
  max_connections?: number
  min_streams?: number
  max_streams?: number
  padding?: boolean
}

export interface SingBoxOutbound {
  tag?: string
  type?: string
  server?: string
  server_port?: number
  method?: string
  username?: string
  password?: string
  uuid?: string
  flow?: string
  security?: string
  alter_id?: number
  packet_encoding?: string
  plugin?: string
  plugin_opts?: string
  version?: string | number
  auth_str?: string
  server_ports?: string[]
  obfs?: string | { type?: string; password?: string }
  up_mbps?: number
  down_mbps?: number
  congestion_control?: string
  udp_relay_mode?: string
  heartbeat?: string
  zero_rtt_handshake?: boolean
  quic?: boolean
  network?: string | string[]
  insecure_concurrency?: number
  extra_headers?: Record<string, string | string[]>
  udp_over_tcp?: boolean | { enabled?: boolean; version?: number }
  idle_session_check_interval?: string
  idle_session_timeout?: string
  min_idle_session?: number
  user?: string
  private_key?: string | string[]
  private_key_passphrase?: string
  host_key?: string[]
  local_address?: string[]
  mtu?: number
  peers?: Array<{
    server?: string
    server_port?: number
    public_key?: string
    pre_shared_key?: string
    allowed_ips?: string[]
    persistent_keepalive_interval?: number
    reserved?: number[]
  }>
  hop_interval?: string
  detour?: string
  tcp_fast_open?: boolean
  tls?: OutboundTls
  transport?: OutboundTransport
  multiplex?: OutboundMultiplex
  x_kunbox_auto_selection_eligible?: boolean
  x_kunbox_metered_protected?: boolean
  [key: string]: unknown
}

export interface NodeWithProfile extends SingBoxOutbound {
  sourceProfileId: string
  sourceProfileName: string
}

export interface CustomProfileNodeSelection {
  sourceProfileId: string
  tag: string
}

export interface AppSettings {
  localPort: number
  socksPort: number
  allowLan: boolean
  systemProxy: boolean
  tunEnabled: boolean
  tunStack: 'system' | 'gvisor' | 'mixed'
  tunStrictRoute: boolean
  localDns: string
  remoteDns: string
  fakeDns: boolean
  bypassLan: boolean
  routingMode: 'rule' | 'global-proxy' | 'global-direct'
  defaultRule: 'direct' | 'proxy' | 'block'
  latencyTestUrl: string
  latencyTestTimeout: number
  healthMonitorEnabled: boolean
  mainNodeAutoFailover: boolean
  healthProbeIntervalSec: number
  autoConnect: boolean
  minimizeToTray: boolean
  startWithWindows: boolean
  startMinimized: boolean
  silentStart: boolean
  exitOnClose: boolean
  theme: 'dark' | 'light' | 'system'
  requireAdmin: boolean
  enableRuntimeLogs: boolean
}

export type NodeLatencyStatus = 'success' | 'timeout' | 'controller_unavailable' | 'proxy_failed' | 'local_test_failed'

export interface NodeLatencyResult {
  status: NodeLatencyStatus
  latencyMs?: number | null
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

export type DomainRuleType = 'domain' | 'domain_suffix' | 'domain_keyword'
export type OutboundMode = 'direct' | 'proxy' | 'block' | 'node' | 'profile'

export interface DomainRule {
  id: string
  name: string
  type: DomainRuleType
  value: string
  outboundMode: OutboundMode
  outboundValue?: string
  enabled: boolean
}

export interface CustomRules {
  domainRules: DomainRule[]
}

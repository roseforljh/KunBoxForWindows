import type { NodeWithProfile, OutboundMultiplex, SingBoxOutbound } from '@shared/types'

// Use the kernel's default protocol and a small connection pool. max_streams
// belongs to an alternative policy and must not be combined with these limits.
export const DEFAULT_MULTIPLEX = {
  enabled: true,
  protocol: 'h2mux',
  max_connections: 4,
  min_streams: 4,
  padding: false,
} satisfies OutboundMultiplex

export function enableMultiplex(current: OutboundMultiplex = {}): OutboundMultiplex {
  const byStreams = (current.max_streams ?? 0) > 0
  return {
    ...current,
    enabled: true,
    protocol: current.protocol || DEFAULT_MULTIPLEX.protocol,
    padding: current.padding ?? DEFAULT_MULTIPLEX.padding,
    max_connections: byStreams ? undefined : current.max_connections ?? DEFAULT_MULTIPLEX.max_connections,
    min_streams: byStreams ? undefined : current.min_streams ?? DEFAULT_MULTIPLEX.min_streams,
    max_streams: byStreams ? current.max_streams : undefined,
  }
}

export function setMultiplexPolicy(current: OutboundMultiplex, byStreams: boolean): OutboundMultiplex {
  return {
    ...current,
    max_connections: byStreams ? undefined : DEFAULT_MULTIPLEX.max_connections,
    min_streams: byStreams ? undefined : DEFAULT_MULTIPLEX.min_streams,
    max_streams: byStreams ? 16 : undefined,
  }
}

export const NODE_POLICY_KEYS = {
  autoSelectionEligible: 'x_kunbox_auto_selection_eligible',
  meteredProtected: 'x_kunbox_metered_protected',
} as const

export const NODE_RUNTIME_KEYS = [
  'latencyMs',
  'latencyStatus',
  'healthStatus',
  'isTimeout',
  'isTesting',
  'sourceProfileId',
  'sourceProfileName',
] as const

export function toEditableNode(node: SingBoxOutbound): SingBoxOutbound {
  const editable = structuredClone(node)
  for (const key of NODE_RUNTIME_KEYS) delete editable[key]
  if (editable.multiplex?.enabled) editable.multiplex = enableMultiplex(editable.multiplex)
  return editable
}

export function makeNodeReference(node: Pick<NodeWithProfile, 'sourceProfileId' | 'tag'>): string {
  return `${node.sourceProfileId}::${node.tag ?? ''}`
}

export function applyNodePolicies(
  node: SingBoxOutbound,
  autoSelectionEligible: boolean,
  meteredProtected: boolean,
): SingBoxOutbound {
  return {
    ...node,
    [NODE_POLICY_KEYS.autoSelectionEligible]: autoSelectionEligible && !meteredProtected,
    [NODE_POLICY_KEYS.meteredProtected]: meteredProtected,
  }
}

export function validateNodeForSave(node: SingBoxOutbound): string | null {
  if (!node.tag?.trim()) return '请输入节点名称'
  if (!node.type?.trim()) return '节点协议无效'
  if (node.type !== 'wireguard') {
    if (!node.server?.trim()) return '请输入服务器地址'
    const hopping = node.type === 'hysteria2' && !!node.server_ports?.length
    if (hopping && !node.server_ports?.every((value) => {
      if (!/^\d+(?::\d+)?$/.test(String(value))) return false
      const ports = String(value).split(':').map(Number)
      return ports.length <= 2 && ports.every((port) => Number.isInteger(port) && port >= 1 && port <= 65535)
        && (ports.length === 1 || ports[0] <= ports[1])
    })) return '端口跳跃格式无效，请填写 443 或 20000:30000，多个值用逗号分隔'
    if (!hopping && (!Number.isInteger(node.server_port) || (node.server_port ?? 0) < 1 || (node.server_port ?? 0) > 65535)) {
      return '端口必须在 1 到 65535 之间'
    }
    if (hopping) {
      const interval = durationMilliseconds(node.hop_interval || '30s')
      const maximum = node.hop_interval_max ? durationMilliseconds(node.hop_interval_max) : interval
      if (interval === null || maximum === null) return '跳跃间隔必须为正数并带单位，例如 30s 或 1m'
      if (maximum < interval) return '随机间隔上限不能小于跳跃间隔（默认 30s）'
    }
  }
  const mux = node.multiplex
  if (mux?.enabled) {
    if (node.type === 'vless' && node.transport?.type === 'xhttp') return 'XHTTP 由 Xray 管理连接复用，请关闭 sing-box 多路复用'
    if (mux.protocol && !['h2mux', 'smux', 'yamux'].includes(mux.protocol)) return '多路复用协议无效'
    for (const value of [mux.max_connections, mux.min_streams, mux.max_streams]) {
      if (value !== undefined && (!Number.isSafeInteger(value) || value < 0 || value > 2147483647)) {
        return '多路复用连接数和流数必须为 0 到 2147483647 之间的整数'
      }
    }
    if ((mux.max_streams ?? 0) > 0 && ((mux.max_connections ?? 0) > 0 || (mux.min_streams ?? 0) > 0)) {
      return '最大流数不能与最大连接数、最小流数同时设置'
    }
  }
  return null
}

function durationMilliseconds(value: string): number | null {
  const units: Record<string, number> = { ns: 0.000001, us: 0.001, 'µs': 0.001, 'μs': 0.001, ms: 1, s: 1000, m: 60000, h: 3600000 }
  const text = value.replace(/^\+/, '')
  const parts = [...text.matchAll(/(\d+(?:\.\d*)?|\.\d+)(ns|us|µs|μs|ms|s|m|h)/g)]
  if (parts.map(([match]) => match).join('') !== text) return null
  const result = parts.reduce((sum, [, amount, unit]) => sum + Number(amount) * units[unit], 0)
  return Number.isFinite(result) && result > 0 ? result : null
}

export function nodePolicyState(node: SingBoxOutbound) {
  const meteredProtected = node[NODE_POLICY_KEYS.meteredProtected] === true
  return {
    meteredProtected,
    autoSelectionEligible: !meteredProtected && node[NODE_POLICY_KEYS.autoSelectionEligible] !== false,
  }
}

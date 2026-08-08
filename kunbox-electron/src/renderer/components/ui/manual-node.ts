export const MANUAL_NODE_PROTOCOLS = [
  { id: 'socks5', code: 'S5', label: 'SOCKS5', description: '通用 SOCKS5 代理', defaultPort: '1080' },
  { id: 'http', code: 'HTTP', label: 'HTTP', description: 'HTTP 或 HTTPS 代理', defaultPort: '8080' },
  { id: 'shadowsocks', code: 'SS', label: 'Shadowsocks', description: '轻量加密代理', defaultPort: '8388' },
  { id: 'vmess', code: 'VM', label: 'VMess', description: 'V2Ray VMess 节点', defaultPort: '443' },
  { id: 'vless', code: 'VL', label: 'VLESS', description: '支持 TLS 与 Reality', defaultPort: '443' },
  { id: 'trojan', code: 'TR', label: 'Trojan', description: '基于 TLS 的代理', defaultPort: '443' },
  { id: 'hysteria2', code: 'HY2', label: 'Hysteria2', description: 'QUIC 高速代理', defaultPort: '443' },
  { id: 'hysteria', code: 'HY', label: 'Hysteria', description: 'Hysteria 旧版协议', defaultPort: '443' },
  { id: 'tuic', code: 'TU', label: 'TUIC', description: '低延迟 QUIC 代理', defaultPort: '443' },
  { id: 'anytls', code: 'AT', label: 'AnyTLS', description: '轻量 TLS 代理', defaultPort: '443' },
  { id: 'naive', code: 'NV', label: 'NaiveProxy', description: '基于 Chromium 网络栈', defaultPort: '443' },
] as const

export type ManualNodeProtocol = typeof MANUAL_NODE_PROTOCOLS[number]['id']
export type ManualTlsMode = 'none' | 'tls' | 'reality'
export type ManualTransport = 'tcp' | 'ws' | 'grpc' | 'xhttp'

export interface ManualNodeDraft {
  protocol: ManualNodeProtocol
  name: string
  server: string
  port: string
  username: string
  password: string
  uuid: string
  method: string
  security: string
  alterId: string
  tlsMode: ManualTlsMode
  serverName: string
  allowInsecure: boolean
  transport: ManualTransport
  path: string
  host: string
  serviceName: string
  flow: string
  publicKey: string
  shortId: string
  fingerprint: string
  auth: string
  upMbps: string
  downMbps: string
  congestionControl: string
  udpRelayMode: string
  alpn: string
  obfs: string
  obfsPassword: string
}

export interface ManualNodeDefinition {
  protocol: ManualNodeProtocol
  tag: string
  link: string
}

const TLS_REQUIRED_PROTOCOLS: ManualNodeProtocol[] = ['trojan', 'hysteria2', 'hysteria', 'tuic', 'anytls', 'naive']

export function createManualNodeDraft(protocol: ManualNodeProtocol): ManualNodeDraft {
  const option = MANUAL_NODE_PROTOCOLS.find((item) => item.id === protocol)
  const tlsMode: ManualTlsMode = TLS_REQUIRED_PROTOCOLS.includes(protocol)
    ? 'tls'
    : (protocol === 'vless' || protocol === 'vmess' ? 'tls' : 'none')

  return {
    protocol,
    name: '',
    server: '',
    port: option?.defaultPort || '443',
    username: '',
    password: '',
    uuid: '',
    method: 'aes-128-gcm',
    security: 'auto',
    alterId: '0',
    tlsMode,
    serverName: '',
    allowInsecure: false,
    transport: 'tcp',
    path: '/',
    host: '',
    serviceName: '',
    flow: '',
    publicKey: '',
    shortId: '',
    fingerprint: 'chrome',
    auth: '',
    upMbps: '100',
    downMbps: '100',
    congestionControl: 'bbr',
    udpRelayMode: 'native',
    alpn: 'h3',
    obfs: 'none',
    obfsPassword: '',
  }
}

function isValidPort(value: string): boolean {
  const port = Number(value)
  return Number.isInteger(port) && port >= 1 && port <= 65535
}

export function validateManualNodeDraft(draft: ManualNodeDraft): string | null {
  if (!draft.name.trim()) return '请输入节点名称'
  if (!draft.server.trim()) return '请输入服务器地址'
  if (!isValidPort(draft.port)) return '端口必须在 1 到 65535 之间'

  switch (draft.protocol) {
    case 'shadowsocks':
      if (!draft.method.trim()) return '请选择加密方式'
      if (!draft.password) return '请输入密码'
      break
    case 'vmess':
    case 'vless':
      if (!draft.uuid.trim()) return '请输入 UUID'
      break
    case 'trojan':
    case 'hysteria2':
    case 'anytls':
      if (!draft.password) return '请输入密码'
      break
    case 'tuic':
      if (!draft.uuid.trim()) return '请输入 UUID'
      if (!draft.password) return '请输入密码'
      break
    case 'naive':
      if (!draft.username.trim()) return '请输入用户名'
      if (!draft.password) return '请输入密码'
      break
    default:
      break
  }

  if (draft.tlsMode === 'reality' && !draft.publicKey.trim()) {
    return 'Reality 模式需要填写公钥'
  }
  if (draft.protocol === 'hysteria2' && draft.obfs !== 'none' && !draft.obfsPassword) {
    return '启用混淆后必须填写混淆密码'
  }
  return null
}

function formatHost(server: string): string {
  const host = server.trim()
  return host.includes(':') && !(host.startsWith('[') && host.endsWith(']')) ? `[${host}]` : host
}

function credentials(username: string, password: string): string {
  if (!username && !password) return ''
  return `${encodeURIComponent(username)}:${encodeURIComponent(password)}@`
}

function makeQuery(entries: Array<[string, string | undefined]>): string {
  const params = new URLSearchParams()
  entries.forEach(([key, value]) => {
    if (value !== undefined && value !== '') params.set(key, value)
  })
  const query = params.toString()
  return query ? `?${query}` : ''
}

function transportQuery(draft: ManualNodeDraft): Array<[string, string | undefined]> {
  const entries: Array<[string, string | undefined]> = [['type', draft.transport]]
  if (draft.transport === 'ws' || draft.transport === 'xhttp') {
    entries.push(['path', draft.path.trim() || '/'])
    entries.push(['host', draft.host.trim() || undefined])
  } else if (draft.transport === 'grpc') {
    entries.push(['serviceName', draft.serviceName.trim() || undefined])
  }
  return entries
}

function encodeBase64Utf8(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
}

export function buildManualNodeLink(draft: ManualNodeDraft): string {
  const error = validateManualNodeDraft(draft)
  if (error) throw new Error(error)

  const host = formatHost(draft.server)
  const port = Number(draft.port)
  const tag = encodeURIComponent(draft.name.trim())
  const sni = draft.serverName.trim() || draft.server.trim()

  switch (draft.protocol) {
    case 'socks5':
      return `socks5://${credentials(draft.username, draft.password)}${host}:${port}#${tag}`
    case 'http': {
      const scheme = draft.tlsMode === 'tls' ? 'https' : 'http'
      const query = makeQuery([
        ['sni', draft.tlsMode === 'tls' ? sni : undefined],
        ['insecure', draft.tlsMode === 'tls' && draft.allowInsecure ? '1' : undefined],
      ])
      return `${scheme}://${credentials(draft.username, draft.password)}${host}:${port}${query}#${tag}`
    }
    case 'shadowsocks':
      return `ss://${encodeURIComponent(`${draft.method}:${draft.password}`)}@${host}:${port}#${tag}`
    case 'vmess': {
      const config = {
        v: '2',
        ps: draft.name.trim(),
        add: draft.server.trim(),
        port,
        id: draft.uuid.trim(),
        aid: Number(draft.alterId) || 0,
        scy: draft.security,
        net: draft.transport,
        type: 'none',
        host: draft.host.trim(),
        path: draft.transport === 'grpc' ? draft.serviceName.trim() : draft.path.trim(),
        tls: draft.tlsMode === 'none' ? '' : 'tls',
        sni: draft.tlsMode === 'none' ? '' : sni,
        allowInsecure: draft.allowInsecure,
      }
      return `vmess://${encodeBase64Utf8(JSON.stringify(config))}`
    }
    case 'vless': {
      const query = makeQuery([
        ['flow', draft.flow.trim() || undefined],
        ...transportQuery(draft),
        ['security', draft.tlsMode],
        ['sni', draft.tlsMode === 'none' ? undefined : sni],
        ['insecure', draft.allowInsecure ? '1' : undefined],
        ['fp', draft.tlsMode === 'none' ? undefined : draft.fingerprint],
        ['pbk', draft.tlsMode === 'reality' ? draft.publicKey.trim() : undefined],
        ['sid', draft.tlsMode === 'reality' ? draft.shortId.trim() : undefined],
      ])
      return `vless://${encodeURIComponent(draft.uuid.trim())}@${host}:${port}${query}#${tag}`
    }
    case 'trojan': {
      const query = makeQuery([
        ...transportQuery(draft),
        ['sni', sni],
        ['allowInsecure', draft.allowInsecure ? '1' : undefined],
      ])
      return `trojan://${encodeURIComponent(draft.password)}@${host}:${port}${query}#${tag}`
    }
    case 'hysteria2': {
      const query = makeQuery([
        ['sni', sni],
        ['insecure', draft.allowInsecure ? '1' : undefined],
        ['obfs', draft.obfs === 'none' ? undefined : draft.obfs],
        ['obfs-password', draft.obfs === 'none' ? undefined : draft.obfsPassword],
      ])
      return `hysteria2://${encodeURIComponent(draft.password)}@${host}:${port}${query}#${tag}`
    }
    case 'hysteria': {
      const query = makeQuery([
        ['auth', draft.auth || undefined],
        ['peer', sni],
        ['insecure', draft.allowInsecure ? '1' : undefined],
        ['upmbps', draft.upMbps || undefined],
        ['downmbps', draft.downMbps || undefined],
      ])
      return `hysteria://${host}:${port}${query}#${tag}`
    }
    case 'tuic': {
      const query = makeQuery([
        ['sni', sni],
        ['allow_insecure', draft.allowInsecure ? '1' : undefined],
        ['congestion_control', draft.congestionControl],
        ['udp_relay_mode', draft.udpRelayMode],
        ['alpn', draft.alpn || undefined],
      ])
      return `tuic://${encodeURIComponent(draft.uuid.trim())}:${encodeURIComponent(draft.password)}@${host}:${port}${query}#${tag}`
    }
    case 'anytls': {
      const query = makeQuery([
        ['sni', sni],
        ['insecure', draft.allowInsecure ? '1' : undefined],
      ])
      return `anytls://${encodeURIComponent(draft.password)}@${host}:${port}${query}#${tag}`
    }
    case 'naive': {
      const query = makeQuery([
        ['sni', sni],
        ['insecure', draft.allowInsecure ? '1' : undefined],
      ])
      return `naive+https://${credentials(draft.username, draft.password)}${host}:${port}${query}#${tag}`
    }
  }
}

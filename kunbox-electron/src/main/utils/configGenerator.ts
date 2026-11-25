import type { AppSettings, SingBoxOutbound, DomainRule } from '../../shared/types'
import { DEFAULT_SETTINGS } from '../../shared/types'
import { CLASH_API_URL } from '../../shared/constants'
import { proxyListenHost } from './proxyPorts'
import {
  DYNAMIC_RULE_SET_TAGS,
  getDynamicRuleSetPath,
  getRuleSetLocalPath,
  isRuleSetLocal,
  loadRuleSetsFromFile,
  syncBypassLanRuleSet,
  syncCustomDomainRuleSets,
  type RuleSetItem
} from './ruleSetManager'
import { existsSync, readFileSync } from 'fs'
import { join } from 'path'

import { getDataDir } from './paths'

const DATA_DIR = getDataDir()

interface GeneratorInput {
  settings?: AppSettings
  outbounds: SingBoxOutbound[]
  activeNodeTag?: string
  domainRules?: DomainRule[]
  clashApiPort?: number
}

function loadDomainRules(): DomainRule[] {
  try {
    const file = join(DATA_DIR, 'custom_rules.json')
    if (existsSync(file)) {
      const data = JSON.parse(readFileSync(file, 'utf-8'))
      return data.domainRules || []
    }
  } catch {
    // ignore
  }
  return []
}

export function routingModeToClashMode(routingMode?: AppSettings['routingMode']): 'rule' | 'global' | 'direct' {
  if (routingMode === 'global-proxy') return 'global'
  if (routingMode === 'global-direct') return 'direct'
  return 'rule'
}

export function generateConfig(input: GeneratorInput): object {
  const { outbounds, activeNodeTag, clashApiPort } = input
  const settings = input.settings || DEFAULT_SETTINGS
  const ruleSets = loadRuleSetsFromFile()
  const domainRules = input.domainRules || loadDomainRules()

  // 启动前同步本地动态规则集文件，供 RayBox fswatch 持续监听热重载
  syncBypassLanRuleSet(settings.bypassLan)
  syncCustomDomainRuleSets(domainRules)

  const { outbounds: builtOutbounds, resolveRuleOutbound } = buildOutboundsWithRules(
    outbounds,
    activeNodeTag,
    settings,
    domainRules,
    ruleSets
  )

  return {
    log: buildLogConfig(settings),
    experimental: buildExperimentalConfig(clashApiPort, settings.routingMode),
    dns: buildDnsConfig(settings, ruleSets, domainRules),
    inbounds: buildInbounds(settings),
    outbounds: builtOutbounds,
    route: buildRouteConfig(settings, ruleSets, domainRules, resolveRuleOutbound)
  }
}

/** 仅用于精准延迟测速：不开 TUN，仅开启单端口 http inbound（按 auth_user 路由到各节点）与可选 clash_api。 */
export function generateLatencyTestConfig(
  nodes: SingBoxOutbound[],
  apiPort: number,
  proxyPort?: number
): object {
  const { proxyOutbounds, extraOutbounds, proxyTags } = resolveDetourOutbounds(nodes)
  const outbounds: object[] = [
    ...proxyOutbounds,
    ...extraOutbounds,
    { type: 'direct', tag: 'direct' }
  ]

  const inbounds: object[] = proxyPort
    ? [{
        type: 'http',
        tag: 'latency-in',
        listen: '127.0.0.1',
        listen_port: proxyPort,
        users: proxyTags.map((tag) => ({ username: tag, password: '' }))
      }]
    : []

  const routeRules: object[] = proxyPort
    ? proxyTags.map((tag) => ({
        inbound: ['latency-in'],
        auth_user: [tag],
        outbound: tag
      }))
    : []

  return {
    log: { disabled: true, level: 'error' },
    experimental: {
      clash_api: {
        external_controller: `127.0.0.1:${apiPort}`
      }
    },
    dns: {
      servers: [{ tag: 'dns-bootstrap', type: 'udp', server: '223.5.5.5', server_port: 53 }],
      final: 'dns-bootstrap'
    },
    inbounds,
    outbounds,
    route: {
      rules: routeRules,
      final: 'direct',
      auto_detect_interface: true,
      default_domain_resolver: 'dns-bootstrap'
    }
  }
}

function buildLogConfig(settings: AppSettings): object {
  // 避免 TUN 高并发下每条连接/DNS 包都刷 INFO 日志拖慢吞吐与 IPC；保留 warn/error 排障与启动错误捕获
  return {
    disabled: false,
    level: settings.enableRuntimeLogs ? 'warn' : 'error',
    timestamp: true
  }
}

function buildExperimentalConfig(clashApiPort?: number, routingMode?: AppSettings['routingMode']): object {
  const controller = clashApiPort ? `127.0.0.1:${clashApiPort}` : new URL(CLASH_API_URL).host
  return {
    clash_api: {
      external_controller: controller,
      default_mode: routingModeToClashMode(routingMode)
    },
    cache_file: {
      enabled: true,
      path: 'cache.db'
    }
  }
}

function buildDnsConfig(settings: AppSettings, ruleSets: RuleSetItem[], domainRules: DomainRule[]): object {
  const servers: object[] = []
  const rules: object[] = []

  servers.push({
    tag: 'dns-bootstrap',
    type: 'udp',
    server: '223.5.5.5',
    server_port: 53
  })

  const localDns = settings.localDns || 'https://dns.alidns.com/dns-query'
  servers.push(buildDnsServer('local-dns', localDns))

  const remoteDns = settings.remoteDns || 'https://dns.google/dns-query'
  servers.push(buildDnsServer('remote-dns', remoteDns, 'PROXY'))

  if (settings.fakeDns) {
    servers.push({
      tag: 'fakeip-dns',
      type: 'fakeip',
      inet4_range: '198.18.0.0/15',
      inet6_range: 'fc00::/18'
    })
  }

  rules.push({
    clash_mode: 'direct',
    action: 'route',
    server: 'local-dns'
  })

  rules.push({
    clash_mode: 'global',
    action: 'route',
    server: settings.fakeDns ? 'fakeip-dns' : 'remote-dns'
  })

  // 用户自定义域名规则（direct / proxy / block）通过本地规则集热加载，且优先于内置广告拦截规则集
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customDirect],
    action: 'route',
    server: 'local-dns'
  })
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customProxy],
    action: 'route',
    server: settings.fakeDns ? 'fakeip-dns' : 'remote-dns'
  })
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customBlock],
    action: 'reject'
  })

  // 指定节点 / 订阅组的自定义域名规则保留内联匹配
  for (const rule of domainRules) {
    if (!rule.enabled) continue
    if (rule.outboundMode !== 'node' && rule.outboundMode !== 'profile') continue
    const matcher = rule.type === 'domain'
      ? { domain: [rule.value] }
      : rule.type === 'domain_suffix'
        ? { domain_suffix: [rule.value] }
        : rule.type === 'domain_keyword'
          ? { domain_keyword: [rule.value] }
          : null
    if (!matcher) continue

    rules.push({ ...matcher, action: 'route', server: settings.fakeDns ? 'fakeip-dns' : 'remote-dns' })
  }

  // 按规则集顺序逐条生成 DNS 规则（自定义业务规则集优先于内置通用规则集，且排除 geoip 类 IP 规则集）
  const isDomainRuleSet = (r: RuleSetItem) => !r.tag.toLowerCase().includes('geoip')
  const orderedRuleSets = [
    ...ruleSets.filter((rs) => !rs.isBuiltIn),
    ...ruleSets.filter((rs) => rs.isBuiltIn)
  ]
  for (const rs of orderedRuleSets) {
    if (!rs.enabled || !isDomainRuleSet(rs)) continue
    if (rs.type === 'remote' && !isRuleSetLocal(rs.tag)) continue

    if (rs.outboundMode === 'block') {
      rules.push({
        rule_set: [rs.tag],
        action: 'reject'
      })
    } else if (rs.outboundMode === 'direct') {
      rules.push({
        rule_set: [rs.tag],
        action: 'route',
        server: 'local-dns'
      })
    } else {
      rules.push({
        rule_set: [rs.tag],
        action: 'route',
        server: settings.fakeDns ? 'fakeip-dns' : 'remote-dns'
      })
    }
  }

  if (settings.fakeDns) {
    rules.push({
      query_type: ['A', 'AAAA'],
      action: 'route',
      server: 'fakeip-dns'
    })
  }

  return {
    servers,
    rules,
    final: 'remote-dns',
    strategy: 'ipv4_only',
    cache_capacity: 4096,
    optimistic: {
      enabled: true
    }
  }
}

function buildDnsServer(tag: string, address: string, detour?: string): object {
  const extra = detour ? { detour } : {}
  if (address.startsWith('https://')) {
    const url = new URL(address)
    return {
      tag,
      type: 'https',
      server: url.hostname,
      server_port: url.port ? parseInt(url.port) : 443,
      path: url.pathname || '/dns-query',
      domain_resolver: 'dns-bootstrap',
      ...extra
    }
  } else if (address.startsWith('tls://')) {
    const server = address.replace('tls://', '').split(':')[0]
    const port = address.includes(':') ? parseInt(address.split(':')[1]) : 853
    return {
      tag,
      type: 'tls',
      server,
      server_port: port,
      domain_resolver: 'dns-bootstrap',
      ...extra
    }
  } else if (address.startsWith('quic://')) {
    const server = address.replace('quic://', '').split(':')[0]
    const port = address.includes(':') ? parseInt(address.split(':')[1]) : 853
    return {
      tag,
      type: 'quic',
      server,
      server_port: port,
      domain_resolver: 'dns-bootstrap',
      ...extra
    }
  } else {
    const parts = address.split(':')
    return {
      tag,
      type: 'udp',
      server: parts[0],
      server_port: parts[1] ? parseInt(parts[1]) : 53,
      ...extra
    }
  }
}

function buildInbounds(settings: AppSettings): object[] {
  const inbounds: object[] = []
  const listen = proxyListenHost(settings.allowLan)

  if (settings.localPort > 0) {
    inbounds.push({
      type: 'mixed',
      tag: 'mixed-in',
      listen,
      listen_port: settings.localPort
    })
  }

  if (settings.socksPort > 0 && settings.socksPort !== settings.localPort) {
    inbounds.push({
      type: 'socks',
      tag: 'socks-in',
      listen,
      listen_port: settings.socksPort
    })
  }

  if (settings.tunEnabled) {
    // Windows Wintun 不显式覆盖 MTU，让 RayBox 内核启用桌面端默认最优 65535 大帧聚合以提升吞吐；
    // 仅配置 IPv4 地址，与 dns.strategy=ipv4_only 对齐，避免应用 Happy Eyeballs 探测 IPv6 超时卡顿
    inbounds.push({
      type: 'tun',
      tag: 'tun-in',
      interface_name: 'kunbox-tun',
      address: ['172.19.0.1/30'],
      auto_route: true,
      strict_route: settings.tunStrictRoute ?? false,
      stack: settings.tunStack || 'mixed',
      udp_timeout: '5m'
    })
  }

  return inbounds
}

export function isProxyType(type?: string): boolean {
  if (!type) return false
  return [
    'shadowsocks',
    'vmess',
    'vless',
    'trojan',
    'hysteria',
    'hysteria2',
    'tuic',
    'anytls',
    'naive',
    'http',
    'socks',
    'wireguard',
    'ssh',
    'shadowtls',
    'xray'
  ].includes(type.toLowerCase())
}

function loadProfileNodesById(profileId: string, cache: Map<string, SingBoxOutbound[]>): SingBoxOutbound[] {
  const cached = cache.get(profileId)
  if (cached) return cached
  try {
    const file = join(getDataDir(), 'configs', `${profileId}.json`)
    if (existsSync(file)) {
      const parsed = JSON.parse(readFileSync(file, 'utf-8'))
      const list = Array.isArray(parsed) ? parsed as SingBoxOutbound[] : []
      cache.set(profileId, list)
      return list
    }
  } catch {
    // 忽略损坏的订阅文件
  }
  cache.set(profileId, [])
  return []
}

/** 解析节点前置代理与去重同名 tag，确保所有 outbound tag 唯一且依赖节点全部注入。 */
function resolveDetourOutbounds(nodes: SingBoxOutbound[]): {
  proxyOutbounds: object[]
  extraOutbounds: object[]
  proxyTags: string[]
  usedTags: Set<string>
  profileCache: Map<string, SingBoxOutbound[]>
  resolveNodeRef: (rawRef: string, selfTag?: string, visiting?: Set<string>) => string | undefined
} {
  const profileCache = new Map<string, SingBoxOutbound[]>()
  const primaryNodes: SingBoxOutbound[] = []
  const primaryTagSet = new Set<string>()
  const usedTags = new Set<string>(['PROXY', 'auto', 'direct', 'block'])

  // 自动对重复 tag 去重，避免 sing-box 因重复 outbound tag 启动失败
  for (const node of nodes) {
    if (!isProxyType(node.type)) continue
    let tag = node.tag?.trim() || `${node.type || 'node'}`
    if (usedTags.has(tag)) {
      let idx = 2
      while (usedTags.has(`${tag} (${idx})`)) idx += 1
      tag = `${tag} (${idx})`
    }
    usedTags.add(tag)
    primaryTagSet.add(tag)
    primaryNodes.push(tag === node.tag ? node : { ...node, tag })
  }

  const extraTagMap = new Map<string, string>()
  const extraOutbounds: object[] = []

  const resolveNodeRef = (rawRef: string, selfTag?: string, visiting: Set<string> = new Set()): string | undefined => {
    const trimmed = rawRef.trim()
    if (!trimmed) return undefined

    const sepIndex = trimmed.indexOf('::')
    if (sepIndex === -1) {
      if (trimmed === selfTag || !primaryTagSet.has(trimmed)) return undefined
      return trimmed
    }

    const refProfileId = trimmed.slice(0, sepIndex)
    const refTag = trimmed.slice(sepIndex + 2)
    if (!refTag) return undefined

    // 目标就在当前订阅内且不是自身
    if (primaryTagSet.has(refTag)) {
      const sameProfileNodes = refProfileId ? loadProfileNodesById(refProfileId, profileCache) : []
      const inSameProfile = sameProfileNodes.some((item) => item.tag === refTag)
      if (!refProfileId || inSameProfile) {
        if (refTag === selfTag) return undefined
        return refTag
      }
    }

    // 跨订阅引用：加载目标订阅节点并注入 extraOutbounds
    const existingExtraTag = extraTagMap.get(trimmed)
    if (existingExtraTag) return existingExtraTag
    if (visiting.has(trimmed)) return undefined

    const targetNode = loadProfileNodesById(refProfileId, profileCache).find(
      (item) => item.tag === refTag && isProxyType(item.type)
    )
    if (!targetNode) return undefined

    const assignedTag = usedTags.has(refTag) ? `ref::${refProfileId}::${refTag}` : refTag
    usedTags.add(assignedTag)
    extraTagMap.set(trimmed, assignedTag)

    visiting.add(trimmed)
    const nextDetour = typeof targetNode.detour === 'string'
      ? resolveNodeRef(targetNode.detour, assignedTag, visiting)
      : undefined
    visiting.delete(trimmed)

    const sanitized = kernelOutbound({ ...targetNode, tag: assignedTag, detour: nextDetour }) as Record<string, unknown>
    if (!nextDetour) delete sanitized.detour
    extraOutbounds.push(sanitized)
    return assignedTag
  }

  const proxyOutbounds: object[] = []
  const proxyTags: string[] = []

  for (const node of primaryNodes) {
    const nextDetour = typeof node.detour === 'string'
      ? resolveNodeRef(node.detour, node.tag, new Set(node.tag ? [node.tag] : []))
      : undefined
    const sanitized = kernelOutbound({ ...node, detour: nextDetour }) as Record<string, unknown>
    if (!nextDetour) delete sanitized.detour
    proxyOutbounds.push(sanitized)
    if (node.tag) {
      proxyTags.push(node.tag)
    }
  }

  return { proxyOutbounds, extraOutbounds, proxyTags, usedTags, profileCache, resolveNodeRef }
}

type RuleOutboundResolver = (mode: string, value?: string) => string

function buildOutboundsWithRules(
  nodes: SingBoxOutbound[],
  activeNodeTag: string | undefined,
  settings: AppSettings,
  domainRules: DomainRule[],
  ruleSets: RuleSetItem[]
): { outbounds: object[]; resolveRuleOutbound: RuleOutboundResolver } {
  const { proxyOutbounds, extraOutbounds, proxyTags, usedTags, profileCache, resolveNodeRef } = resolveDetourOutbounds(nodes)
  const profileGroupTags = new Map<string, string>()
  const profileGroupOutbounds: object[] = []
  const testUrl = settings.latencyTestUrl || 'http://cp.cloudflare.com/generate_204'

  const ensureProfileGroup = (profileId: string): string | undefined => {
    const existing = profileGroupTags.get(profileId)
    if (existing) return existing
    const profileNodes = loadProfileNodesById(profileId, profileCache).filter((n) => isProxyType(n.type) && n.tag)
    if (!profileNodes.length) return undefined

    const memberTags: string[] = []
    for (const item of profileNodes) {
      const resolved = resolveNodeRef(`${profileId}::${item.tag}`)
      if (resolved) memberTags.push(resolved)
    }
    if (!memberTags.length) return undefined

    const groupTag = `profile::${profileId}`
    if (!usedTags.has(groupTag)) {
      usedTags.add(groupTag)
      profileGroupOutbounds.push(
        memberTags.length === 1
          ? { type: 'selector', tag: groupTag, outbounds: memberTags, default: memberTags[0] }
          : { type: 'urltest', tag: groupTag, outbounds: memberTags, url: testUrl, interval: '300s', tolerance: 50 }
      )
    }
    profileGroupTags.set(profileId, groupTag)
    return groupTag
  }

  const resolveRuleOutbound: RuleOutboundResolver = (mode, value) => {
    if (mode === 'direct') return 'direct'
    if (mode === 'node' && value) {
      return resolveNodeRef(value) || 'PROXY'
    }
    if (mode === 'profile' && value) {
      return ensureProfileGroup(value) || 'PROXY'
    }
    return 'PROXY'
  }

  // 预解析所有启用规则引用的节点与配置组，确保 extraOutbounds 与 profileGroupOutbounds 完整生成
  for (const rule of domainRules) {
    if (rule.enabled && (rule.outboundMode === 'node' || rule.outboundMode === 'profile')) {
      resolveRuleOutbound(rule.outboundMode, rule.outboundValue)
    }
  }
  for (const ruleSet of ruleSets) {
    if (ruleSet.enabled && (ruleSet.outboundMode === 'node' || ruleSet.outboundMode === 'profile')) {
      resolveRuleOutbound(ruleSet.outboundMode, ruleSet.outboundValue)
    }
  }

  const outbounds: object[] = [...proxyOutbounds, ...extraOutbounds, ...profileGroupOutbounds]
  const defaultTag = (activeNodeTag && proxyTags.includes(activeNodeTag)) ? activeNodeTag : proxyTags[0]

  if (proxyTags.length > 0) {
    outbounds.unshift({
      type: 'selector',
      tag: 'PROXY',
      outbounds: proxyTags,
      default: defaultTag,
      interrupt_exist_connections: true
    })
  }

  if (proxyTags.length > 1) {
    outbounds.push({
      type: 'urltest',
      tag: 'auto',
      outbounds: proxyTags,
      url: testUrl,
      interval: '300s',
      tolerance: 50
    })
  }

  outbounds.push({ type: 'direct', tag: 'direct' })

  return { outbounds, resolveRuleOutbound }
}

const XRAY_OUTER_DIAL_KEYS = [
  'tag',
  'detour',
  'bind_interface',
  'inet4_bind_address',
  'inet6_bind_address',
  'bind_address_no_port',
  'protect_path',
  'routing_mark',
  'reuse_addr',
  'netns',
  'tcp_fast_open',
  'tcp_multi_path',
  'disable_tcp_keep_alive',
  'tcp_keep_alive',
  'tcp_keep_alive_interval',
  'udp_fragment',
  'connect_timeout',
  'domain_resolver',
  'network_strategy',
  'network_type',
  'fallback_network_type',
  'fallback_delay',
  'domain_strategy'
] as const

function vlessEncryption(node: SingBoxOutbound): string {
  const direct = typeof node.encryption === 'string' ? node.encryption.trim() : ''
  if (direct) return direct
  const extra = node.transport?.extra
  if (extra && typeof extra === 'object' && !Array.isArray(extra)) {
    const legacy = typeof extra.encryption === 'string' ? extra.encryption.trim() : ''
    if (legacy) return legacy
  }
  return 'none'
}

/** 按 RayBox 契约识别需走原生 type=xray 出站的节点（XHTTP/SplitHTTP 或启用 VLESS Encryption）。 */
function isNativeXrayNode(node: SingBoxOutbound): boolean {
  const protocol = node.type?.toLowerCase()
  if (protocol !== 'vless' && protocol !== 'vmess' && protocol !== 'trojan') return false
  const transportType = node.transport?.type?.toLowerCase()
  if (transportType === 'xhttp' || transportType === 'splithttp') return true
  return protocol === 'vless' && vlessEncryption(node).toLowerCase() !== 'none'
}

function rejectXraySpecialFields(value: unknown, context: string): void {
  if (Array.isArray(value)) {
    for (const item of value) rejectXraySpecialFields(item, context)
    return
  }
  if (!value || typeof value !== 'object') return
  for (const [key, nested] of Object.entries(value as Record<string, unknown>)) {
    const lower = key.toLowerCase()
    if (lower === 'rx' || lower === 'oob') {
      throw new Error(`${context} 的 RX/OOB 需要明确的 Cloudflare 上下文`)
    }
    rejectXraySpecialFields(nested, context)
  }
}

function buildXrayTlsSettings(node: SingBoxOutbound, tag: string): { security: string; settings?: Record<string, unknown> } {
  const tls = node.tls
  if (!tls || !tls.enabled) return { security: 'none' }

  const realityEnabled = tls.reality?.enabled === true
  const settings: Record<string, unknown> = {}
  if (tls.server_name) settings.serverName = tls.server_name
  if (tls.utls && tls.utls.enabled !== false && tls.utls.fingerprint) {
    settings.fingerprint = tls.utls.fingerprint
  }

  if (realityEnabled) {
    if (tls.insecure === true || (Array.isArray(tls.alpn) && tls.alpn.length > 0)) {
      throw new Error(`节点 ${tag} 的 Reality 不支持 insecure 或 ALPN`)
    }
    if (tls.certificate && tls.certificate.length > 0) {
      throw new Error(`节点 ${tag} 的 Reality 不支持自定义证书`)
    }
    const publicKey = tls.reality?.public_key?.trim()
    if (!publicKey) throw new Error(`节点 ${tag} 的 Reality 缺少公钥`)
    settings.publicKey = publicKey
    if (tls.reality?.short_id) settings.shortId = tls.reality.short_id
    if (!settings.fingerprint) settings.fingerprint = 'chrome'
    return { security: 'reality', settings }
  }

  if (typeof tls.insecure === 'boolean') settings.allowInsecure = tls.insecure
  if (Array.isArray(tls.alpn) && tls.alpn.length > 0) settings.alpn = tls.alpn
  if (tls.certificate && tls.certificate.length > 0) {
    const certs = Array.isArray(tls.certificate) ? tls.certificate : [tls.certificate]
    if (certs.some((cert) => typeof cert !== 'string' || !cert.includes('BEGIN CERTIFICATE'))) {
      throw new Error(`节点 ${tag} 的 TLS certificate 必须包含 PEM 证书`)
    }
    // 对齐 RayBox streamconn/stream.go：使用自签受信 CA 验证时关闭系统根证书回退
    settings.disableSystemRoot = true
    settings.certificates = [{ usage: 'verify', certificate: certs }]
  }
  return { security: 'tls', settings }
}

/** 对齐 RayBox streamconn/stream.go：Xray 传输层 headers 仅接受单值 string map。 */
function toSingleValueHeaders(
  headers: Record<string, string | string[]> | undefined,
  tag: string,
  excludeHost = false
): Record<string, string> | undefined {
  if (!headers) return undefined
  const out: Record<string, string> = {}
  for (const [key, value] of Object.entries(headers)) {
    if (excludeHost && key.toLowerCase() === 'host') continue
    if (Array.isArray(value)) {
      if (value.length !== 1) {
        throw new Error(`节点 ${tag} 的传输头 ${key} 仅支持单个值`)
      }
      out[key] = value[0]
    } else if (typeof value === 'string') {
      out[key] = value
    }
  }
  return Object.keys(out).length > 0 ? out : undefined
}

function buildXrayStreamSettings(node: SingBoxOutbound, tag: string): Record<string, unknown> {
  const transport = node.transport
  const rawType = transport?.type?.toLowerCase() || 'tcp'
  const stream: Record<string, unknown> = {}

  if (rawType === 'tcp') {
    stream.network = 'tcp'
  } else if (rawType === 'ws') {
    const wsSettings: Record<string, unknown> = {}
    let wsPath = transport?.path || ''
    if (transport?.max_early_data) {
      if (transport.early_data_header_name && transport.early_data_header_name !== 'Sec-WebSocket-Protocol') {
        throw new Error(`节点 ${tag} 的 WebSocket early data 仅支持 Sec-WebSocket-Protocol`)
      }
      const parsedUrl = new URL(wsPath || '/', 'http://placeholder.invalid')
      parsedUrl.searchParams.set('ed', String(transport.max_early_data))
      wsPath = `${parsedUrl.pathname}${parsedUrl.search}`
    }
    if (wsPath) wsSettings.path = wsPath
    const headers = toSingleValueHeaders(transport?.headers, tag)
    if (headers) wsSettings.headers = headers
    stream.network = 'ws'
    stream.wsSettings = wsSettings
  } else if (rawType === 'httpupgrade') {
    const httpupgradeSettings: Record<string, unknown> = {}
    const hostFromField = Array.isArray(transport?.host) ? transport.host[0] : transport?.host
    const headerHostRaw = transport?.headers?.Host ?? transport?.headers?.host
    const hostFromHeader = Array.isArray(headerHostRaw) ? headerHostRaw[0] : headerHostRaw
    const host = (typeof hostFromField === 'string' && hostFromField.trim())
      || (typeof hostFromHeader === 'string' && hostFromHeader.trim())
      || ''
    if (host) httpupgradeSettings.host = host
    if (transport?.path) httpupgradeSettings.path = transport.path
    const headers = toSingleValueHeaders(transport?.headers, tag, true)
    if (headers) httpupgradeSettings.headers = headers
    stream.network = 'httpupgrade'
    stream.httpupgradeSettings = httpupgradeSettings
  } else if (rawType === 'grpc') {
    const grpcSettings: Record<string, unknown> = {}
    if (transport?.service_name) grpcSettings.serviceName = transport.service_name
    stream.network = 'grpc'
    stream.grpcSettings = grpcSettings
  } else if (rawType === 'xhttp' || rawType === 'splithttp') {
    const xhttpSettings: Record<string, unknown> = {}
    if (transport?.path) xhttpSettings.path = transport.path

    // 优先读取标准 transport.host，同时兼容历史导入写入 transport.headers.Host 的节点
    const hostFromField = Array.isArray(transport?.host)
      ? (transport.host.length > 1 ? (() => { throw new Error(`节点 ${tag} 的 XHTTP 仅支持一个 host`) })() : transport.host[0])
      : transport?.host
    const headerHostRaw = transport?.headers?.Host ?? transport?.headers?.host
    const hostFromHeader = Array.isArray(headerHostRaw) ? headerHostRaw[0] : headerHostRaw
    const host = (typeof hostFromField === 'string' && hostFromField.trim())
      || (typeof hostFromHeader === 'string' && hostFromHeader.trim())
      || ''
    if (host) xhttpSettings.host = host

    // 对齐 RayBox option.XHTTPOptions / SplitHTTPConfig：透传非 Host 自定义请求头
    const headers = toSingleValueHeaders(transport?.headers, tag, true)
    if (headers) xhttpSettings.headers = headers

    if (transport?.mode) xhttpSettings.mode = transport.mode
    if (transport?.sc_max_each_post_bytes !== undefined) xhttpSettings.scMaxEachPostBytes = transport.sc_max_each_post_bytes
    if (transport?.sc_min_posts_interval_ms !== undefined) xhttpSettings.scMinPostsIntervalMs = transport.sc_min_posts_interval_ms
    if (transport?.sc_max_buffered_posts !== undefined) xhttpSettings.scMaxBufferedPosts = transport.sc_max_buffered_posts
    if (transport?.x_padding_bytes) xhttpSettings.xPaddingBytes = transport.x_padding_bytes
    if (transport?.no_grpc_header !== undefined) xhttpSettings.noGRPCHeader = transport.no_grpc_header
    if (transport?.no_sse_header !== undefined) xhttpSettings.noSSEHeader = transport.no_sse_header

    if (transport?.extra !== undefined) {
      let extraObj: Record<string, unknown>
      if (typeof transport.extra === 'string') {
        try {
          const parsed = JSON.parse(transport.extra)
          if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
            throw new Error(`节点 ${tag} 的 XHTTP extra 必须是 JSON 对象`)
          }
          extraObj = parsed as Record<string, unknown>
        } catch (err) {
          throw err instanceof Error ? err : new Error(`节点 ${tag} 的 XHTTP extra 不是有效 JSON 对象`)
        }
      } else if (transport.extra && typeof transport.extra === 'object' && !Array.isArray(transport.extra)) {
        extraObj = { ...transport.extra }
      } else {
        throw new Error(`节点 ${tag} 的 XHTTP extra 必须是对象`)
      }
      delete extraObj.encryption
      rejectXraySpecialFields(extraObj, `节点 ${tag} XHTTP`)
      if (Object.keys(extraObj).length > 0) {
        xhttpSettings.extra = extraObj
      }
    }

    stream.network = 'xhttp'
    stream.xhttpSettings = xhttpSettings
  } else {
    throw new Error(`节点 ${tag} 不支持的 Xray transport: ${rawType}`)
  }

  const { security, settings } = buildXrayTlsSettings(node, tag)
  stream.security = security
  if (security !== 'none' && settings) {
    stream[`${security}Settings`] = settings
  }
  return stream
}

/** 将 Xray 专属节点转换为 RayBox 原生 type=xray 出站，保留外层 sing-box 拨号字段。 */
export function buildNativeXrayOutbound(node: SingBoxOutbound): Record<string, unknown> {
  const tag = node.tag || 'xray'
  const protocol = node.type?.toLowerCase()
  if (protocol !== 'vless' && protocol !== 'vmess' && protocol !== 'trojan') {
    throw new Error(`节点 ${tag} 不支持原生 Xray`)
  }
  if (node.multiplex?.enabled) {
    throw new Error(`节点 ${tag} 的 Xray 出站不支持 sing-box 多路复用`)
  }
  if (node.packet_encoding && node.packet_encoding !== 'xudp') {
    throw new Error(`节点 ${tag} 的 Xray 出站不支持 packet_encoding=${node.packet_encoding}`)
  }
  if (!node.server || !node.server_port) {
    throw new Error(`节点 ${tag} 缺少服务器地址或端口`)
  }

  let settings: Record<string, unknown>
  if (protocol === 'trojan') {
    if (!node.password) throw new Error(`节点 ${tag} 缺少 Trojan 密码`)
    settings = {
      servers: [{ address: node.server, port: node.server_port, password: node.password }]
    }
  } else {
    if (!node.uuid) throw new Error(`节点 ${tag} 缺少 UUID`)
    const user: Record<string, unknown> = { id: node.uuid }
    if (protocol === 'vless') {
      user.encryption = vlessEncryption(node)
      if (node.flow) user.flow = node.flow
    } else {
      if ((node.alter_id ?? 0) !== 0) {
        throw new Error(`节点 ${tag} 的 VMess alter_id 不受原生 Xray 支持`)
      }
      if (node.security) user.security = node.security
    }
    settings = {
      vnext: [{ address: node.server, port: node.server_port, users: [user] }]
    }
  }

  const outer: Record<string, unknown> = { type: 'xray' }
  for (const key of XRAY_OUTER_DIAL_KEYS) {
    if (node[key] !== undefined) outer[key] = node[key]
  }
  outer.outbound = {
    protocol,
    settings,
    streamSettings: buildXrayStreamSettings(node, tag)
  }
  return outer
}

/** 订阅里带的 x_kunbox_* 是客户端内部标记，内核碰到未知字段会直接拒绝整份配置；Xray 专属节点转原生 type=xray。 */
function kernelOutbound(node: SingBoxOutbound): object {
  if (isNativeXrayNode(node)) {
    return buildNativeXrayOutbound(node)
  }
  return Object.fromEntries(Object.entries(node).filter(([key]) => !key.startsWith('x_')))
}

function buildRouteConfig(
  settings: AppSettings,
  ruleSets: RuleSetItem[],
  domainRules: DomainRule[],
  resolveRuleOutbound: RuleOutboundResolver
): object {
  const rules: object[] = []

  // 1. 53 端口 DNS 流量零延迟直接劫持，不经过嗅探器
  rules.push({
    port: [53],
    action: 'hijack-dns'
  })

  // 2. 绕过局域网通过本地规则集热开关，私网连接直接直出，不浪费嗅探开销
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.bypassLan],
    outbound: 'direct'
  })

  // 3. 精准协议嗅探并缩短超时（默认 300ms -> 50ms），避免服务端先发或慢首包协议被卡顿
  rules.push({
    action: 'sniff',
    sniffer: ['tls', 'http', 'quic', 'dns'],
    timeout: '50ms'
  })

  // 4. 兜底非 53 端口嗅探出的 DNS 协议流量
  rules.push({
    protocol: ['dns'],
    action: 'hijack-dns'
  })

  // Clash 模式热切换规则（支持通过 PATCH /configs 零重启切换直连 / 全局 / 规则模式）
  rules.push({
    clash_mode: 'direct',
    outbound: 'direct'
  })
  rules.push({
    clash_mode: 'global',
    outbound: 'PROXY'
  })

  // 自定义域名分流规则：direct / proxy / block 走本地规则集热重载，node / profile 保留精确出站绑定
  for (const rule of domainRules.filter(r => r.enabled && (r.outboundMode === 'node' || r.outboundMode === 'profile'))) {
    const matcher = rule.type === 'domain'
      ? { domain: [rule.value] }
      : rule.type === 'domain_suffix'
        ? { domain_suffix: [rule.value] }
        : rule.type === 'domain_keyword'
          ? { domain_keyword: [rule.value] }
          : null
    if (!matcher) continue

    rules.push({ ...matcher, outbound: resolveRuleOutbound(rule.outboundMode, rule.outboundValue) })
  }

  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customDirect],
    outbound: 'direct'
  })
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customProxy],
    outbound: 'PROXY'
  })
  rules.push({
    rule_set: [DYNAMIC_RULE_SET_TAGS.customBlock],
    action: 'reject'
  })

  // 用户自定义业务规则集始终优先于内置通用规则集（geosite-cn / geoip-cn / 广告拦截），防止被内置大合集误拦截或误直连
  const orderedRuleSets = [
    ...ruleSets.filter((rs) => !rs.isBuiltIn),
    ...ruleSets.filter((rs) => rs.isBuiltIn)
  ]
  const availableRuleSets = orderedRuleSets.filter(r => {
    if (!r.enabled) return false
    if (r.type === 'remote') {
      return isRuleSetLocal(r.tag)
    }
    return true
  })

  for (const ruleSet of availableRuleSets) {
    if (ruleSet.outboundMode === 'block') {
      rules.push({
        rule_set: [ruleSet.tag],
        action: 'reject'
      })
      continue
    }

    rules.push({
      rule_set: [ruleSet.tag],
      outbound: resolveRuleOutbound(ruleSet.outboundMode, ruleSet.outboundValue)
    })
  }

  const dynamicRuleSets = [
    DYNAMIC_RULE_SET_TAGS.bypassLan,
    DYNAMIC_RULE_SET_TAGS.customDirect,
    DYNAMIC_RULE_SET_TAGS.customProxy,
    DYNAMIC_RULE_SET_TAGS.customBlock
  ].map((tag) => ({
    type: 'local',
    tag,
    format: 'source',
    path: getDynamicRuleSetPath(tag)
  }))

  return {
    rules,
    final: 'PROXY',
    auto_detect_interface: true,
    default_domain_resolver: 'dns-bootstrap',
    rule_set: [...dynamicRuleSets, ...buildRuleSets(availableRuleSets)]
  }
}

function buildRuleSets(ruleSets: RuleSetItem[]): object[] {
  return ruleSets.map(r => ({
    type: 'local',
    tag: r.tag,
    format: r.format,
    path: r.type === 'remote' ? getRuleSetLocalPath(r.tag) : (r.path || r.url)
  }))
}

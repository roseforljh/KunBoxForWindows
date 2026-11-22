import type { AppSettings, SingBoxOutbound, DomainRule } from '../../shared/types'
import { DEFAULT_SETTINGS } from '../../shared/types'
import { CLASH_API_URL } from '../../shared/constants'
import { proxyListenHost } from './proxyPorts'
import { loadRuleSetsFromFile, isRuleSetLocal, getRuleSetLocalPath, type RuleSetItem } from './ruleSetManager'
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

export function generateConfig(input: GeneratorInput): object {
  const { outbounds, activeNodeTag, clashApiPort } = input
  const settings = input.settings || DEFAULT_SETTINGS
  const ruleSets = loadRuleSetsFromFile()
  const domainRules = input.domainRules || loadDomainRules()
  const { outbounds: builtOutbounds, resolveRuleOutbound } = buildOutboundsWithRules(
    outbounds,
    activeNodeTag,
    settings,
    domainRules,
    ruleSets
  )

  return {
    log: buildLogConfig(),
    experimental: buildExperimentalConfig(clashApiPort),
    dns: buildDnsConfig(settings, ruleSets, domainRules),
    inbounds: buildInbounds(settings),
    outbounds: builtOutbounds,
    route: buildRouteConfig(settings, ruleSets, domainRules, resolveRuleOutbound)
  }
}

/** 仅用于关闭 VPN 时离线测速：不开 TUN、不监听本地代理端口，只开启随机端口 clash_api。 */
export function generateLatencyTestConfig(nodes: SingBoxOutbound[], apiPort: number): object {
  const { proxyOutbounds, extraOutbounds } = resolveDetourOutbounds(nodes)
  const outbounds: object[] = [
    ...proxyOutbounds,
    ...extraOutbounds,
    { type: 'direct', tag: 'direct' }
  ]

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
    inbounds: [],
    outbounds,
    route: {
      final: 'direct',
      auto_detect_interface: true,
      default_domain_resolver: 'dns-bootstrap'
    }
  }
}

function buildLogConfig(): object {
  return {
    disabled: false,
    level: 'info',
    timestamp: true
  }
}

function buildExperimentalConfig(clashApiPort?: number): object {
  const controller = clashApiPort ? `127.0.0.1:${clashApiPort}` : new URL(CLASH_API_URL).host
  return {
    clash_api: {
      external_controller: controller,
      default_mode: 'rule'
    },
    cache_file: {
      enabled: true,
      path: 'cache.db'
    }
  }
}

function buildDnsConfig(settings: AppSettings, ruleSets: RuleSetItem[], _domainRules: DomainRule[]): object {
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
  servers.push(buildDnsServer('remote-dns', remoteDns))

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

  // DNS 规则仅允许域名类规则集，排除 geoip 等 IP 规则集，避免触发 sing-box 1.14 Legacy Address Filter 报错
  const isDomainRuleSet = (r: RuleSetItem) => !r.tag.toLowerCase().includes('geoip')

  const cnRuleSets = ruleSets.filter(r =>
    r.enabled && r.outboundMode === 'direct' && r.tag.includes('cn') && isDomainRuleSet(r) &&
    (r.type !== 'remote' || isRuleSetLocal(r.tag))
  )
  if (cnRuleSets.length > 0) {
    rules.push({
      rule_set: cnRuleSets.map(r => r.tag),
      action: 'route',
      server: 'local-dns'
    })
  }

  const blockRuleSets = ruleSets.filter(r =>
    r.enabled && r.outboundMode === 'block' && isDomainRuleSet(r) &&
    (r.type !== 'remote' || isRuleSetLocal(r.tag))
  )
  if (blockRuleSets.length > 0) {
    rules.push({
      rule_set: blockRuleSets.map(r => r.tag),
      action: 'reject'
    })
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
    strategy: 'ipv4_only'
  }
}

function buildDnsServer(tag: string, address: string): object {
  if (address.startsWith('https://')) {
    const url = new URL(address)
    return {
      tag,
      type: 'https',
      server: url.hostname,
      server_port: url.port ? parseInt(url.port) : 443,
      path: url.pathname || '/dns-query',
      domain_resolver: 'dns-bootstrap'
    }
  } else if (address.startsWith('tls://')) {
    const server = address.replace('tls://', '').split(':')[0]
    const port = address.includes(':') ? parseInt(address.split(':')[1]) : 853
    return {
      tag,
      type: 'tls',
      server,
      server_port: port,
      domain_resolver: 'dns-bootstrap'
    }
  } else if (address.startsWith('quic://')) {
    const server = address.replace('quic://', '').split(':')[0]
    const port = address.includes(':') ? parseInt(address.split(':')[1]) : 853
    return {
      tag,
      type: 'quic',
      server,
      server_port: port,
      domain_resolver: 'dns-bootstrap'
    }
  } else {
    const parts = address.split(':')
    return {
      tag,
      type: 'udp',
      server: parts[0],
      server_port: parts[1] ? parseInt(parts[1]) : 53
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
    inbounds.push({
      type: 'tun',
      tag: 'tun-in',
      interface_name: 'kunbox-tun',
      address: ['172.19.0.1/30', 'fdfe:dcba:9876::1/126'],
      mtu: 9000,
      auto_route: true,
      strict_route: settings.tunStrictRoute ?? true,
      stack: settings.tunStack || 'system'
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
    'http',
    'socks',
    'wireguard',
    'ssh',
    'shadowtls'
  ].includes(type)
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
      interrupt_exist_connections: false
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

/** 订阅里带的 x_kunbox_* 是客户端内部标记，内核碰到未知字段会直接拒绝整份配置。 */
function kernelOutbound(node: SingBoxOutbound): object {
  return Object.fromEntries(Object.entries(node).filter(([key]) => !key.startsWith('x_')))
}

function buildRouteConfig(
  settings: AppSettings,
  ruleSets: RuleSetItem[],
  domainRules: DomainRule[],
  resolveRuleOutbound: RuleOutboundResolver
): object {
  const rules: object[] = []

  if (settings.tunEnabled) {
    rules.push({ inbound: ['tun-in'], action: 'sniff' })
  }

  rules.push({
    protocol: ['dns'],
    action: 'hijack-dns'
  })

  if (settings.bypassLan) {
    rules.push({
      ip_is_private: true,
      outbound: 'direct'
    })
  }

  // 自定义域名分流规则（统一支持 direct / proxy / node / profile / reject）
  for (const rule of domainRules.filter(r => r.enabled)) {
    const matcher = rule.type === 'domain'
      ? { domain: [rule.value] }
      : rule.type === 'domain_suffix'
        ? { domain_suffix: [rule.value] }
        : rule.type === 'domain_keyword'
          ? { domain_keyword: [rule.value] }
          : null
    if (!matcher) continue

    if (rule.outboundMode === 'block') {
      rules.push({ ...matcher, action: 'reject' })
    } else {
      rules.push({ ...matcher, outbound: resolveRuleOutbound(rule.outboundMode, rule.outboundValue) })
    }
  }

  // Filter available rule sets
  const availableRuleSets = ruleSets.filter(r => {
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

  const finalOutbound = settings.routingMode === 'global-direct' ? 'direct' : 'PROXY'

  return {
    rules,
    final: finalOutbound,
    auto_detect_interface: true,
    default_domain_resolver: 'dns-bootstrap',
    rule_set: buildRuleSets(availableRuleSets)
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

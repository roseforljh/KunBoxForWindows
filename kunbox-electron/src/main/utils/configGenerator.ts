import type { AppSettings, SingBoxOutbound, DomainRule } from '../../shared/types'
import { DEFAULT_SETTINGS } from '../../shared/types'
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
  const { outbounds, activeNodeTag } = input
  const settings = input.settings || DEFAULT_SETTINGS
  const ruleSets = loadRuleSetsFromFile()
  const domainRules = input.domainRules || loadDomainRules()

  return {
    log: buildLogConfig(),
    experimental: buildExperimentalConfig(),
    dns: buildDnsConfig(settings, ruleSets, domainRules),
    inbounds: buildInbounds(settings),
    outbounds: buildOutbounds(outbounds, activeNodeTag, settings),
    route: buildRouteConfig(settings, ruleSets, domainRules)
  }
}

function buildLogConfig(): object {
  return {
    disabled: false,
    level: 'info',
    timestamp: true
  }
}

function buildExperimentalConfig(): object {
  return {
    clash_api: {
      external_controller: '127.0.0.1:9090',
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
      type: 'fakeip'
    })
  }

  rules.push({
    clash_mode: 'direct',
    action: 'route',
    server: 'local-dns'
  })

  const cnRuleSets = ruleSets.filter(r =>
    r.enabled && r.outboundMode === 'direct' && r.tag.includes('cn') &&
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
    r.enabled && r.outboundMode === 'block' &&
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

  const dnsConfig: Record<string, unknown> = {
    servers,
    rules,
    final: 'remote-dns',
    strategy: 'ipv4_only'
  }

  if (settings.fakeDns) {
    dnsConfig.fakeip = {
      enabled: true,
      inet4_range: '198.18.0.0/15',
      inet6_range: 'fc00::/18'
    }
  }

  return dnsConfig
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

  if (settings.localPort > 0) {
    inbounds.push({
      type: 'mixed',
      tag: 'mixed-in',
      listen: settings.allowLan ? '0.0.0.0' : '127.0.0.1',
      listen_port: settings.localPort
    })
  }

  if (settings.socksPort > 0 && settings.socksPort !== settings.localPort) {
    inbounds.push({
      type: 'socks',
      tag: 'socks-in',
      listen: settings.allowLan ? '0.0.0.0' : '127.0.0.1',
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

function buildOutbounds(
  nodes: SingBoxOutbound[],
  activeNodeTag: string | undefined,
  settings: AppSettings
): object[] {
  const outbounds: object[] = []
  const proxyTags: string[] = []

  for (const node of nodes) {
    if (isProxyType(node.type)) {
      outbounds.push(kernelOutbound(node))
      if (node.tag) {
        proxyTags.push(node.tag)
      }
    }
  }

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
      url: settings.latencyTestUrl || 'https://www.gstatic.com/generate_204',
      interval: '300s',
      tolerance: 50
    })
  }

  outbounds.push({ type: 'direct', tag: 'direct' })
  outbounds.push({ type: 'block', tag: 'block' })

  return outbounds
}

/** 订阅里带的 x_kunbox_* 是客户端内部标记，内核碰到未知字段会直接拒绝整份配置。 */
function kernelOutbound(node: SingBoxOutbound): object {
  return Object.fromEntries(Object.entries(node).filter(([key]) => !key.startsWith('x_')))
}

function buildRouteConfig(settings: AppSettings, ruleSets: RuleSetItem[], domainRules: DomainRule[]): object {
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

  // Add custom domain rules
  for (const rule of domainRules.filter(r => r.enabled)) {
    const outbound = rule.outboundMode === 'direct' ? 'direct' :
                     rule.outboundMode === 'block' ? 'block' : 'PROXY'
    if (rule.type === 'domain') {
      rules.push({ domain: [rule.value], outbound })
    } else if (rule.type === 'domain_suffix') {
      rules.push({ domain_suffix: [rule.value], outbound })
    } else if (rule.type === 'domain_keyword') {
      rules.push({ domain_keyword: [rule.value], outbound })
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

    const outbound = ruleSet.outboundMode === 'direct' ? 'direct' : 'PROXY'
    rules.push({
      rule_set: [ruleSet.tag],
      outbound
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

import type { AppSettings, SingBoxOutbound } from '../../shared/types'
import { DEFAULT_SETTINGS } from '../../shared/types'
import { loadRuleSets, type RuleSetItem } from '../ipc/rulesets'
import { isRuleSetLocal, getRuleSetLocalPath } from './ruleSetManager'

interface GeneratorInput {
  settings?: AppSettings
  outbounds: SingBoxOutbound[]
  activeNodeTag?: string
}

export function generateConfig(input: GeneratorInput): object {
  const { outbounds, activeNodeTag } = input
  // Use default settings if not provided
  const settings = input.settings || DEFAULT_SETTINGS
  // Load user configured rule sets
  const ruleSets = loadRuleSets()

  return {
    log: buildLogConfig(),
    experimental: buildExperimentalConfig(),
    dns: buildDnsConfig(settings, ruleSets),
    inbounds: buildInbounds(settings),
    outbounds: buildOutbounds(outbounds, activeNodeTag, settings),
    route: buildRouteConfig(settings, ruleSets)
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

function buildDnsConfig(settings: AppSettings, ruleSets: RuleSetItem[]): object {
  const servers: object[] = []
  const rules: object[] = []

  // Bootstrap DNS - UDP format for sing-box 1.12.0+
  servers.push({
    tag: 'dns-bootstrap',
    type: 'udp',
    server: '223.5.5.5',
    server_port: 53
  })

  // Local DNS for China domains
  const localDns = settings.localDns || 'https://dns.alidns.com/dns-query'
  servers.push(buildDnsServer('local-dns', localDns))

  // Remote DNS for foreign domains
  const remoteDns = settings.remoteDns || 'https://dns.google/dns-query'
  servers.push(buildDnsServer('remote-dns', remoteDns))

  if (settings.fakeDns) {
    servers.push({
      tag: 'fakeip-dns',
      type: 'fakeip'
    })
  }

  // DNS rules for sing-box 1.12.0+
  rules.push({
    clash_mode: 'direct',
    action: 'route',
    server: 'local-dns'
  })

  // Use CN rulesets for local DNS (only if they have local cache)
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

  // Block ads using reject action (only if they have local cache)
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
  // Parse URL to extract server info
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
    // UDP DNS
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

  if (settings.tunEnabled) {
    inbounds.push({
      type: 'tun',
      tag: 'tun-in',
      interface_name: 'kunbox-tun',
      inet4_address: ['172.19.0.1/30'],
      inet6_address: ['fdfe:dcba:9876::1/126'],
      mtu: 9000,
      auto_route: true,
      strict_route: true,
      stack: settings.tunStack,
      sniff: true,
      sniff_override_destination: false
    })
  }

  return inbounds
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
      outbounds.push(node)
      if (node.tag) {
        proxyTags.push(node.tag)
      }
    }
  }

  const defaultTag = activeNodeTag || proxyTags[0]

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
      url: settings.latencyTestUrl,
      interval: '300s',
      tolerance: 50
    })
  }

  outbounds.push({ type: 'direct', tag: 'direct' })

  return outbounds
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

function buildRouteConfig(settings: AppSettings, ruleSets: RuleSetItem[]): object {
  const rules: object[] = []

  // DNS hijacking - use action instead of dns-out outbound
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

  // Filter to only rulesets that have local cache
  const availableRuleSets = ruleSets.filter(r => {
    if (!r.enabled) return false
    if (r.type === 'remote') {
      return isRuleSetLocal(r.tag)
    }
    return true // Local rulesets are always available
  })
  
  // Add rules from available rule sets only
  for (const ruleSet of availableRuleSets) {
    let outbound: string
    switch (ruleSet.outboundMode) {
      case 'direct':
        outbound = 'direct'
        break
      case 'proxy':
        outbound = 'PROXY'
        break
      case 'block':
        rules.push({
          rule_set: [ruleSet.tag],
          action: 'reject'
        })
        continue
      case 'node':
        outbound = ruleSet.outboundValue || 'PROXY'
        break
      case 'profile':
        outbound = 'PROXY'
        break
      default:
        outbound = 'PROXY'
    }
    
    if (ruleSet.outboundMode !== 'block') {
      rules.push({
        rule_set: [ruleSet.tag],
        outbound
      })
    }
  }

  return {
    rules,
    final: 'PROXY',
    auto_detect_interface: true,
    default_domain_resolver: 'dns-bootstrap',
    rule_set: buildRuleSets(availableRuleSets)
  }
}

function buildRuleSets(ruleSets: RuleSetItem[]): object[] {
  // All rulesets passed here should already be validated as available
  return ruleSets.map(r => ({
    type: 'local',
    tag: r.tag,
    format: r.format,
    path: r.type === 'remote' ? getRuleSetLocalPath(r.tag) : r.url
  }))
}

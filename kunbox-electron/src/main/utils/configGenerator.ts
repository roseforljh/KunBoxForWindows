import type { AppSettings, SingBoxOutbound } from '../../shared/types'

interface GeneratorInput {
  settings: AppSettings
  outbounds: SingBoxOutbound[]
  activeNodeTag?: string
}

export function generateConfig(input: GeneratorInput): object {
  const { settings, outbounds, activeNodeTag } = input

  return {
    log: buildLogConfig(),
    experimental: buildExperimentalConfig(),
    dns: buildDnsConfig(settings),
    inbounds: buildInbounds(settings),
    outbounds: buildOutbounds(outbounds, activeNodeTag, settings),
    route: buildRouteConfig(settings)
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

function buildDnsConfig(settings: AppSettings): object {
  const servers: object[] = []
  const rules: object[] = []

  servers.push({
    tag: 'dns-bootstrap',
    address: '223.5.5.5',
    detour: 'direct'
  })

  const localDns = settings.localDns || 'https://dns.alidns.com/dns-query'
  const localDnsServer: Record<string, unknown> = {
    tag: 'local-dns',
    address: localDns,
    detour: 'direct'
  }
  if (needsResolver(localDns)) {
    localDnsServer.address_resolver = 'dns-bootstrap'
  }
  servers.push(localDnsServer)

  const remoteDns = settings.remoteDns || 'https://dns.google/dns-query'
  const remoteDnsServer: Record<string, unknown> = {
    tag: 'remote-dns',
    address: remoteDns,
    detour: 'PROXY'
  }
  if (needsResolver(remoteDns)) {
    remoteDnsServer.address_resolver = 'dns-bootstrap'
  }
  servers.push(remoteDnsServer)

  servers.push({
    tag: 'block-dns',
    address: 'rcode://success'
  })

  if (settings.fakeDns) {
    servers.push({
      tag: 'fakeip-dns',
      address: 'fakeip'
    })
  }

  rules.push({
    outbound: ['any'],
    server: 'dns-bootstrap'
  })

  rules.push({
    outbound: ['direct'],
    server: 'local-dns'
  })

  rules.push({
    rule_set: ['geosite-cn'],
    server: 'local-dns'
  })

  if (settings.blockAds) {
    rules.push({
      rule_set: ['geosite-ads'],
      server: 'block-dns',
      disable_cache: true
    })
  }

  if (settings.fakeDns) {
    rules.push({
      query_type: ['A', 'AAAA'],
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

function needsResolver(address: string): boolean {
  return (
    address.startsWith('https://') ||
    address.startsWith('tls://') ||
    address.startsWith('quic://') ||
    address.startsWith('h3://')
  )
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
  outbounds.push({ type: 'block', tag: 'block' })
  outbounds.push({ type: 'dns', tag: 'dns-out' })

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

function buildRouteConfig(settings: AppSettings): object {
  const rules: object[] = []

  rules.push({
    protocol: ['dns'],
    outbound: 'dns-out'
  })

  if (settings.bypassLan) {
    rules.push({
      ip_is_private: true,
      outbound: 'direct'
    })
  }

  rules.push({
    rule_set: ['geosite-cn'],
    outbound: 'direct'
  })

  rules.push({
    rule_set: ['geoip-cn'],
    outbound: 'direct'
  })

  return {
    rules,
    final: 'PROXY',
    auto_detect_interface: true,
    rule_set: buildRuleSets()
  }
}

function buildRuleSets(): object[] {
  return [
    {
      type: 'remote',
      tag: 'geosite-cn',
      format: 'binary',
      url: 'https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs',
      download_detour: 'direct'
    },
    {
      type: 'remote',
      tag: 'geoip-cn',
      format: 'binary',
      url: 'https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs',
      download_detour: 'direct'
    }
  ]
}

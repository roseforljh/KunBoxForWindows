import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync, unlinkSync } from 'fs'
import { randomUUID } from 'crypto'
import { spawn, ChildProcess } from 'child_process'
import axios from 'axios'
import yaml from 'js-yaml'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import type { Profile, SingBoxOutbound } from '../../shared/types'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')
const TEMP_TEST_DIR = join(DATA_DIR, 'temp_test')

// Temporary sing-box process for latency testing
let tempSingboxProcess: ChildProcess | null = null
let tempSingboxPort = 19090 // Use different port than main VPN

interface ProfilesData {
  profiles: Profile[]
  activeProfileId: string | null
  activeNodeTag: string | null
}

let data: ProfilesData = {
  profiles: [],
  activeProfileId: null,
  activeNodeTag: null
}

function load() {
  try {
    if (existsSync(PROFILES_FILE)) {
      const json = readFileSync(PROFILES_FILE, 'utf-8')
      data = JSON.parse(json)
    }
  } catch (error) {
    log.error('Failed to load profiles:', error)
  }
}

function save() {
  try {
    mkdirSync(DATA_DIR, { recursive: true })
    writeFileSync(PROFILES_FILE, JSON.stringify(data, null, 2))
  } catch (error) {
    log.error('Failed to save profiles:', error)
  }
}

const autoUpdateTimers = new Map<string, NodeJS.Timeout>()

function scheduleAutoUpdate(profile: Profile) {
  const existingTimer = autoUpdateTimers.get(profile.id)
  if (existingTimer) {
    clearInterval(existingTimer)
    autoUpdateTimers.delete(profile.id)
  }

  if (profile.autoUpdateInterval <= 0 || !profile.enabled || !profile.url) {
    return
  }

  const intervalMs = profile.autoUpdateInterval * 60 * 1000
  const timer = setInterval(async () => {
    try {
      const currentProfile = data.profiles.find(p => p.id === profile.id)
      if (!currentProfile || !currentProfile.enabled || currentProfile.autoUpdateInterval <= 0) {
        clearInterval(timer)
        autoUpdateTimers.delete(profile.id)
        return
      }

      log.info(`Auto-updating subscription: ${currentProfile.name}`)
      const nodes = await fetchSubscription(currentProfile.url)
      currentProfile.lastUpdate = Date.now()
      currentProfile.nodeCount = nodes.length
      saveProfileNodes(currentProfile.id, nodes)
      save()
      log.info(`Auto-update completed: ${currentProfile.name}, ${nodes.length} nodes`)
    } catch (error) {
      log.error(`Auto-update failed for ${profile.name}:`, error)
    }
  }, intervalMs)

  autoUpdateTimers.set(profile.id, timer)
  log.info(`Scheduled auto-update for ${profile.name} every ${profile.autoUpdateInterval} minutes`)
}

function rescheduleAllAutoUpdates() {
  for (const profile of data.profiles) {
    scheduleAutoUpdate(profile)
  }
}

async function prewarmDns(configJson: string, dnsServer?: string | null): Promise<void> {
  const domains = extractDomainsFromConfig(configJson)
  if (domains.size === 0) return

  log.info(`DNS prewarm: resolving ${domains.size} domains...`)
  const startTime = Date.now()

  const { lookup } = await import('dns/promises')
  const results = await Promise.allSettled(
    Array.from(domains).map(domain =>
      lookup(domain, { family: 4 }).catch(() => null)
    )
  )

  const resolved = results.filter(r => r.status === 'fulfilled' && r.value).length
  const duration = Date.now() - startTime
  log.info(`DNS prewarm completed: ${resolved}/${domains.size} resolved in ${duration}ms`)
}

function extractDomainsFromConfig(configJson: string): Set<string> {
  const domains = new Set<string>()
  const serverRegex = /"server"\s*:\s*"([^"]+)"/g
  let match: RegExpExecArray | null

  while ((match = serverRegex.exec(configJson)) !== null) {
    const server = match[1]
    if (server && !isIpAddress(server) && isValidDomain(server)) {
      domains.add(server)
    }
  }

  return domains
}

function isIpAddress(host: string): boolean {
  return /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(host) ||
    (host.includes(':') && /^[0-9a-fA-F:]+$/.test(host)) ||
    (host.startsWith('[') && host.endsWith(']'))
}

function isValidDomain(host: string): boolean {
  if (!host.includes('.')) return false
  if (host.startsWith('.') || host.endsWith('.')) return false
  return /^[a-zA-Z0-9][a-zA-Z0-9\-.]*[a-zA-Z0-9]$/.test(host)
}

export { prewarmDns }

function loadProfileNodes(profileId: string): SingBoxOutbound[] {
  try {
    const configFile = join(CONFIGS_DIR, `${profileId}.json`)
    if (!existsSync(configFile)) return []
    const json = readFileSync(configFile, 'utf-8')
    return JSON.parse(json)
  } catch {
    return []
  }
}

function saveProfileNodes(profileId: string, nodes: SingBoxOutbound[]) {
  try {
    mkdirSync(CONFIGS_DIR, { recursive: true })
    const configFile = join(CONFIGS_DIR, `${profileId}.json`)
    writeFileSync(configFile, JSON.stringify(nodes, null, 2))
  } catch (error) {
    log.error('Failed to save nodes:', error)
  }
}

async function fetchSubscription(url: string): Promise<SingBoxOutbound[]> {
  const response = await axios.get(url, { timeout: 30000 })
  const content = response.data

  if (typeof content === 'object' && content.proxies) {
    return parseClashConfig(content)
  }

  if (typeof content === 'string') {
    return parseContent(content)
  }

  return []
}

function parseContent(content: string): SingBoxOutbound[] {
  try {
    const yamlContent = yaml.load(content) as { proxies?: unknown[] }
    if (yamlContent?.proxies) {
      return parseClashConfig(yamlContent)
    }
  } catch {
    // Not YAML
  }

  try {
    const jsonContent = JSON.parse(content)
    if (jsonContent?.proxies) {
      return parseClashConfig(jsonContent)
    }
    if (jsonContent?.outbounds) {
      return jsonContent.outbounds.filter((o: SingBoxOutbound) => 
        o.type && !['direct', 'block', 'dns', 'selector', 'urltest'].includes(o.type)
      )
    }
  } catch {
    // Not JSON
  }

  try {
    const decoded = Buffer.from(content, 'base64').toString('utf-8')
    const lines = decoded.split('\n').filter(l => l.trim())
    const nodes = lines.map(parseNodeLink).filter((n): n is SingBoxOutbound => n !== null)
    if (nodes.length > 0) return nodes
  } catch {
    // Not base64
  }

  const lines = content.split('\n').filter(l => l.trim())
  return lines.map(parseNodeLink).filter((n): n is SingBoxOutbound => n !== null)
}

function parseClashConfig(config: { proxies?: unknown[] }): SingBoxOutbound[] {
  if (!config.proxies) return []

  return config.proxies.map((proxy: unknown) => {
    const p = proxy as Record<string, unknown>
    const type = mapClashType(p.type as string)
    
    const base: SingBoxOutbound = {
      tag: p.name as string,
      type,
      server: p.server as string,
      server_port: p.port as number
    }

    // Common fields
    if (p.password) base.password = p.password as string
    if (p.uuid) base.uuid = p.uuid as string
    if (p.method) base.method = p.method as string
    if (p.flow) base.flow = p.flow as string

    // TLS configuration
    if (p.tls === true || p.network === 'ws' || p.network === 'grpc' || p.network === 'h2') {
      // For WebSocket nodes using CDN (like Cloudflare Argo), set insecure to true by default
      // as they often use self-signed or CDN certificates
      const isWsWithCdn = p.network === 'ws' && 
        (p.server as string)?.includes('.') && 
        !isDirectIp(p.server as string)
      
      base.tls = {
        enabled: true,
        server_name: (p.servername || p.sni || p.server) as string,
        insecure: p['skip-cert-verify'] === true || isWsWithCdn
      }

      // ALPN
      if (p.alpn && Array.isArray(p.alpn)) {
        base.tls.alpn = p.alpn as string[]
      }

      // Client fingerprint (uTLS)
      if (p['client-fingerprint']) {
        base.tls.utls = {
          enabled: true,
          fingerprint: p['client-fingerprint'] as string
        }
      }

      // Reality
      const realityOpts = p['reality-opts'] as Record<string, unknown> | undefined
      if (realityOpts) {
        base.tls.reality = {
          enabled: true,
          public_key: realityOpts['public-key'] as string,
          short_id: realityOpts['short-id'] as string
        }
      }
    }

    // Transport configuration
    if (p.network === 'ws') {
      const wsOpts = p['ws-opts'] as Record<string, unknown> | undefined
      let path = (wsOpts?.path as string) || '/'
      
      base.transport = {
        type: 'ws',
        headers: wsOpts?.headers as Record<string, string> | undefined
      }
      
      // Parse early data from path (e.g., /path?ed=2560) and REMOVE it from path
      // This is how SubStore handles it
      const edMatch = path.match(/^(.*?)(?:\?ed=(\d+))?$/)
      if (edMatch) {
        path = edMatch[1] || '/'
        if (edMatch[2]) {
          base.transport.max_early_data = parseInt(edMatch[2])
          base.transport.early_data_header_name = 'Sec-WebSocket-Protocol'
        }
      }
      
      // Also check ws-opts for max-early-data
      if (!base.transport.max_early_data && wsOpts?.['max-early-data']) {
        base.transport.max_early_data = wsOpts['max-early-data'] as number
        base.transport.early_data_header_name = wsOpts?.['early-data-header-name'] as string || 'Sec-WebSocket-Protocol'
      }
      
      base.transport.path = path
      
      // Ensure Host header is set if not present
      if (!base.transport.headers?.Host && p.servername) {
        base.transport.headers = {
          ...base.transport.headers,
          Host: p.servername as string
        }
      }
    } else if (p.network === 'grpc') {
      const grpcOpts = p['grpc-opts'] as Record<string, unknown> | undefined
      base.transport = {
        type: 'grpc',
        service_name: grpcOpts?.['grpc-service-name'] as string || ''
      }
    } else if (p.network === 'h2') {
      const h2Opts = p['h2-opts'] as Record<string, unknown> | undefined
      base.transport = {
        type: 'http',
        path: (h2Opts?.path as string[])?.join(',') || '/',
        host: h2Opts?.host as string[] | undefined
      }
    }

    // VMess specific
    if (type === 'vmess') {
      base.uuid = p.uuid as string
      base.security = (p.cipher as string) || 'auto'
      if (p.alterId) base.alter_id = p.alterId as number
    }

    // Hysteria2 specific  
    if (type === 'hysteria2') {
      if (!base.tls) {
        base.tls = {
          enabled: true,
          server_name: (p.sni || p.server) as string,
          insecure: p['skip-cert-verify'] === true
        }
      }
      if (p.alpn && Array.isArray(p.alpn)) {
        base.tls.alpn = p.alpn as string[]
      }
    }

    // VLESS specific - packet encoding
    if (type === 'vless') {
      base.packet_encoding = 'xudp'
    }

    return base
  }).filter(n => n.tag && n.server)
}

// Check if the server is a direct IP address (not a domain)
function isDirectIp(server: string): boolean {
  // IPv4 pattern
  const ipv4Regex = /^(\d{1,3}\.){3}\d{1,3}$/
  // IPv6 pattern (simplified)
  const ipv6Regex = /^[\da-fA-F:]+$/
  return ipv4Regex.test(server) || ipv6Regex.test(server)
}

function mapClashType(type: string): string {
  const map: Record<string, string> = {
    ss: 'shadowsocks',
    ssr: 'shadowsocksr',
    vmess: 'vmess',
    vless: 'vless',
    trojan: 'trojan',
    hysteria: 'hysteria',
    hysteria2: 'hysteria2',
    tuic: 'tuic',
    http: 'http',
    socks5: 'socks'
  }
  return map[type.toLowerCase()] || type
}

function parseNodeLink(link: string): SingBoxOutbound | null {
  try {
    if (link.startsWith('ss://')) {
      const [encoded, tag] = link.slice(5).split('#')
      const decoded = Buffer.from(encoded.split('@')[0], 'base64').toString()
      const [method, password] = decoded.split(':')
      const hostPart = encoded.split('@')[1]?.split(':')
      return {
        tag: decodeURIComponent(tag || 'SS'),
        type: 'shadowsocks',
        server: hostPart?.[0] || '',
        server_port: parseInt(hostPart?.[1] || '0'),
        method,
        password
      }
    }
    // Add more protocol parsers as needed
    return null
  } catch {
    return null
  }
}

export function initProfileHandlers() {
  mkdirSync(DATA_DIR, { recursive: true })
  mkdirSync(CONFIGS_DIR, { recursive: true })
  load()
  rescheduleAllAutoUpdates()

  ipcMain.handle(IPC_CHANNELS.PROFILE_LIST, () => {
    return data.profiles
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_ADD, async (_, url: string, name?: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }) => {
    const nodes = await fetchSubscription(url)
    const profile: Profile = {
      id: randomUUID(),
      name: name || new URL(url).hostname,
      url,
      lastUpdate: Date.now(),
      nodeCount: nodes.length,
      enabled: true,
      autoUpdateInterval: settings?.autoUpdateInterval || 0,
      dnsPreResolve: settings?.dnsPreResolve || false,
      dnsServer: settings?.dnsServer || null
    }

    data.profiles.push(profile)
    saveProfileNodes(profile.id, nodes)

    if (!data.activeProfileId) {
      data.activeProfileId = profile.id
      if (nodes.length > 0) {
        data.activeNodeTag = nodes[0].tag || null
      }
    }

    scheduleAutoUpdate(profile)
    save()
    return profile
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_IMPORT_CONTENT, async (_, name: string, content: string, settings?: { autoUpdateInterval: number; dnsPreResolve: boolean; dnsServer: string | null }) => {
    const nodes = parseContent(content)
    if (nodes.length === 0) {
      throw new Error('No valid nodes found in content')
    }

    const profile: Profile = {
      id: randomUUID(),
      name: name || 'Imported',
      url: '',
      lastUpdate: Date.now(),
      nodeCount: nodes.length,
      enabled: true,
      autoUpdateInterval: settings?.autoUpdateInterval || 0,
      dnsPreResolve: settings?.dnsPreResolve || false,
      dnsServer: settings?.dnsServer || null
    }

    data.profiles.push(profile)
    saveProfileNodes(profile.id, nodes)

    if (!data.activeProfileId) {
      data.activeProfileId = profile.id
      if (nodes.length > 0) {
        data.activeNodeTag = nodes[0].tag || null
      }
    }

    save()
    return profile
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_UPDATE, async (_, id: string) => {
    const profile = data.profiles.find(p => p.id === id)
    if (!profile) throw new Error('Profile not found')

    const nodes = await fetchSubscription(profile.url)
    profile.lastUpdate = Date.now()
    profile.nodeCount = nodes.length

    saveProfileNodes(profile.id, nodes)
    save()
    return profile
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_DELETE, (_, id: string) => {
    data.profiles = data.profiles.filter(p => p.id !== id)

    const configFile = join(CONFIGS_DIR, `${id}.json`)
    if (existsSync(configFile)) {
      unlinkSync(configFile)
    }

    if (data.activeProfileId === id) {
      data.activeProfileId = data.profiles[0]?.id || null
      data.activeNodeTag = null
    }

    save()
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_SET_ACTIVE, (_, id: string) => {
    if (!data.profiles.find(p => p.id === id)) {
      throw new Error('Profile not found')
    }

    data.activeProfileId = id
    const nodes = loadProfileNodes(id)
    data.activeNodeTag = nodes[0]?.tag || null
    save()
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_REFRESH, async (_, id: string) => {
    return ipcMain.handle(IPC_CHANNELS.PROFILE_UPDATE, _, id)
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_EDIT, (_, id: string, updates: { name: string; url: string; autoUpdateInterval?: number; dnsPreResolve?: boolean; dnsServer?: string | null }) => {
    const profile = data.profiles.find(p => p.id === id)
    if (!profile) throw new Error('Profile not found')

    profile.name = updates.name
    profile.url = updates.url
    if (updates.autoUpdateInterval !== undefined) {
      profile.autoUpdateInterval = updates.autoUpdateInterval
    }
    if (updates.dnsPreResolve !== undefined) {
      profile.dnsPreResolve = updates.dnsPreResolve
    }
    if (updates.dnsServer !== undefined) {
      profile.dnsServer = updates.dnsServer
    }
    scheduleAutoUpdate(profile)
    save()
    return profile
  })

  ipcMain.handle(IPC_CHANNELS.PROFILE_SET_ENABLED, (_, id: string, enabled: boolean) => {
    const profile = data.profiles.find(p => p.id === id)
    if (!profile) throw new Error('Profile not found')

    profile.enabled = enabled
    save()
  })

  ipcMain.handle(IPC_CHANNELS.NODE_LIST, () => {
    if (!data.activeProfileId) return []
    return loadProfileNodes(data.activeProfileId)
  })

  ipcMain.handle(IPC_CHANNELS.NODE_SET_ACTIVE, (_, tag: string) => {
    data.activeNodeTag = tag
    save()
  })

  ipcMain.handle(IPC_CHANNELS.NODE_TEST_LATENCY, async (_, tag: string) => {
    // Test latency through the local proxy (like Android version)
    return testProxyLatency(tag)
  })

  ipcMain.handle(IPC_CHANNELS.NODE_TEST_ALL, async () => {
    const nodes = data.activeProfileId ? loadProfileNodes(data.activeProfileId) : []
    const results: Record<string, number> = {}
    
    // Test all nodes in parallel with concurrency limit
    const CONCURRENCY = 5
    const chunks: SingBoxOutbound[][] = []
    for (let i = 0; i < nodes.length; i += CONCURRENCY) {
      chunks.push(nodes.slice(i, i + CONCURRENCY))
    }
    
    for (const chunk of chunks) {
      const promises = chunk.map(async (node) => {
        if (!node.tag) {
          return
        }
        const latency = await testProxyLatency(node.tag)
        results[node.tag] = latency
      })
      await Promise.all(promises)
    }
    
    return results
  })

  ipcMain.handle(IPC_CHANNELS.NODE_DELETE, (_, tag: string) => {
    if (!data.activeProfileId) throw new Error('No active profile')
    
    const nodes = loadProfileNodes(data.activeProfileId)
    const filteredNodes = nodes.filter(n => n.tag !== tag)
    
    if (filteredNodes.length === nodes.length) {
      throw new Error('Node not found')
    }
    
    saveProfileNodes(data.activeProfileId, filteredNodes)
    
    const profile = data.profiles.find(p => p.id === data.activeProfileId)
    if (profile) {
      profile.nodeCount = filteredNodes.length
    }
    
    if (data.activeNodeTag === tag) {
      data.activeNodeTag = filteredNodes[0]?.tag || null
    }
    
    save()
  })

  ipcMain.handle(IPC_CHANNELS.NODE_ADD, (_, link: string, target?: { type: 'existing'; profileId: string } | { type: 'new'; profileName: string }) => {
    const node = parseNodeLink(link)
    if (!node) throw new Error('Invalid node link')

    let targetProfileId: string

    if (!target || target.type === 'existing') {
      targetProfileId = target?.profileId || data.activeProfileId || ''
      if (!targetProfileId) throw new Error('No target profile')
      if (!data.profiles.find(p => p.id === targetProfileId)) {
        throw new Error('Profile not found')
      }
    } else {
      const newProfile: Profile = {
        id: randomUUID(),
        name: target.profileName,
        url: '',
        lastUpdate: Date.now(),
        nodeCount: 0,
        enabled: true,
        autoUpdateInterval: 0,
        dnsPreResolve: false,
        dnsServer: null
      }
      data.profiles.push(newProfile)
      targetProfileId = newProfile.id

      if (!data.activeProfileId) {
        data.activeProfileId = newProfile.id
      }
    }

    const nodes = loadProfileNodes(targetProfileId)
    nodes.push(node)
    saveProfileNodes(targetProfileId, nodes)

    const profile = data.profiles.find(p => p.id === targetProfileId)
    if (profile) {
      profile.nodeCount = nodes.length
    }

    save()
    return node
  })

  ipcMain.handle(IPC_CHANNELS.NODE_EXPORT, (_, tag: string) => {
    if (!data.activeProfileId) throw new Error('No active profile')
    
    const nodes = loadProfileNodes(data.activeProfileId)
    const node = nodes.find(n => n.tag === tag)
    
    if (!node) throw new Error('Node not found')
    
    return exportNodeToLink(node)
  })
}

/**
 * Test latency for a node using sing-box Clash API
 * Works both when VPN is running (uses main sing-box) and when not (uses temp sing-box)
 * Reference: GUI.for.SingBox, Android KunBox
 */
async function testProxyLatency(tag: string, timeout: number = 10000): Promise<number> {
  // Check if main VPN is running by trying to connect to its API
  const isMainVpnRunning = await checkMainVpnRunning()
  
  if (isMainVpnRunning) {
    // VPN is running, use main sing-box Clash API
    return testWithClashApi(tag, timeout, 9090)
  } else {
    // VPN not running, need to use temporary sing-box
    const nodes = data.activeProfileId ? loadProfileNodes(data.activeProfileId) : []
    const node = nodes.find(n => n.tag === tag)
    if (!node) {
      log.debug(`Node not found: ${tag}`)
      return -1
    }
    
    // Start temp sing-box if not running
    const started = await startTempSingbox(nodes)
    if (!started) {
      log.debug('Failed to start temp sing-box for latency test')
      return -1
    }
    
    // Wait a bit for sing-box to be ready
    await new Promise(resolve => setTimeout(resolve, 500))
    
    // Test using temp sing-box
    return testWithClashApi(tag, timeout, tempSingboxPort)
  }
}

/**
 * Check if main VPN sing-box is running by trying to connect to its API
 */
async function checkMainVpnRunning(): Promise<boolean> {
  const http = await import('http')
  
  return new Promise((resolve) => {
    const req = http.request({
      hostname: '127.0.0.1',
      port: 9090,
      path: '/version',
      method: 'GET',
      timeout: 1000
    }, (res) => {
      resolve(res.statusCode === 200)
    })
    
    req.on('error', () => resolve(false))
    req.on('timeout', () => {
      req.destroy()
      resolve(false)
    })
    
    req.end()
  })
}

/**
 * Test latency using sing-box's Clash API
 * Uses /proxies/{name}/delay endpoint
 * Reference: GUI.for.SingBox
 */
async function testWithClashApi(proxyName: string, timeout: number = 10000, port: number = 9090): Promise<number> {
  const http = await import('http')
  
  const API_HOST = '127.0.0.1'
  const TEST_URL = 'https://www.gstatic.com/generate_204'
  
  return new Promise((resolve) => {
    const encodedName = encodeURIComponent(proxyName)
    // Build path with query params
    const path = `/proxies/${encodedName}/delay?url=${encodeURIComponent(TEST_URL)}&timeout=${timeout}`
    
    const options = {
      hostname: API_HOST,
      port: port,
      path: path,
      method: 'GET',
      timeout: timeout + 5000
    }
    
    const req = http.request(options, (res) => {
      let body = ''
      res.on('data', chunk => body += chunk)
      res.on('end', () => {
        try {
          const result = JSON.parse(body)
          if (typeof result.delay === 'number' && result.delay > 0) {
            resolve(result.delay)
          } else if (result.message) {
            log.debug(`Delay test error for ${proxyName}: ${result.message}`)
            resolve(-1)
          } else {
            resolve(-1)
          }
        } catch (e) {
          log.debug(`Clash API parse error for ${proxyName}: ${body}`)
          resolve(-1)
        }
      })
    })
    
    req.on('error', (e) => {
      log.debug(`Clash API request error for ${proxyName}: ${e.message}`)
      resolve(-1)
    })
    
    req.on('timeout', () => {
      req.destroy()
      log.debug(`Clash API timeout for ${proxyName}`)
      resolve(-1)
    })
    
    req.end()
  })
}

function exportNodeToLink(node: SingBoxOutbound): string {
  const tag = encodeURIComponent(node.tag || 'Node')
  
  switch (node.type?.toLowerCase()) {
    case 'shadowsocks': {
      const userInfo = Buffer.from(`${node.method}:${node.password}`).toString('base64')
      return `ss://${userInfo}@${node.server}:${node.server_port}#${tag}`
    }
    
    case 'vmess': {
      const vmessConfig = {
        v: '2',
        ps: node.tag,
        add: node.server,
        port: node.server_port,
        id: node.uuid,
        aid: 0,
        net: 'tcp',
        type: 'none',
        tls: ''
      }
      return `vmess://${Buffer.from(JSON.stringify(vmessConfig)).toString('base64')}`
    }
    
    case 'vless': {
      return `vless://${node.uuid}@${node.server}:${node.server_port}?flow=${node.flow || ''}&type=tcp#${tag}`
    }
    
    case 'trojan': {
      return `trojan://${node.password}@${node.server}:${node.server_port}#${tag}`
    }
    
    case 'hysteria2': {
      return `hysteria2://${node.password}@${node.server}:${node.server_port}#${tag}`
    }
    
    default:
      return JSON.stringify(node, null, 2)
  }
}

/**
 * Start a temporary sing-box instance for latency testing when VPN is not running
 */
async function startTempSingbox(nodes: SingBoxOutbound[]): Promise<boolean> {
  if (tempSingboxProcess) {
    // Check if still running
    try {
      const isRunning = await checkTempSingboxRunning()
      if (isRunning) return true
    } catch {
      // Not running, need to restart
    }
    stopTempSingbox()
  }

  mkdirSync(TEMP_TEST_DIR, { recursive: true })

  // Generate minimal config for testing - need log enabled to detect startup
  const config = {
    log: { 
      disabled: false,
      level: 'info',
      timestamp: true
    },
    experimental: {
      clash_api: {
        external_controller: `127.0.0.1:${tempSingboxPort}`,
        default_mode: 'rule'
      }
    },
    inbounds: [],
    outbounds: [
      ...nodes,
      { type: 'direct', tag: 'direct' }
    ],
    route: {
      final: 'direct',
      auto_detect_interface: true
    }
  }

  const configPath = join(TEMP_TEST_DIR, 'test_config.json')
  writeFileSync(configPath, JSON.stringify(config, null, 2))
  log.info(`[TempSingbox] Config written to ${configPath}`)

  // Find sing-box executable - use same logic as singbox.ts
  const devPath = join(__dirname, '../../resources/libs/sing-box.exe')
  const prodPath = join(process.resourcesPath || '', 'resources/libs/sing-box.exe')
  const exePath = existsSync(devPath) ? devPath : prodPath

  if (!existsSync(exePath)) {
    log.error(`[TempSingbox] sing-box executable not found at ${devPath} or ${prodPath}`)
    return false
  }

  log.info(`[TempSingbox] Starting with executable: ${exePath}`)

  return new Promise((resolve) => {
    tempSingboxProcess = spawn(exePath, ['run', '-c', configPath], {
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
      cwd: TEMP_TEST_DIR
    })

    let started = false
    const timeout = setTimeout(() => {
      if (!started) {
        // Even if we don't see the log, try to check if API is responding
        checkTempSingboxRunning().then(running => {
          if (running) {
            started = true
            log.info('[TempSingbox] Started (detected via API)')
            resolve(true)
          } else {
            log.error('[TempSingbox] Startup timeout')
            stopTempSingbox()
            resolve(false)
          }
        })
      }
    }, 3000)

    tempSingboxProcess.stdout?.on('data', (data) => {
      const msg = data.toString()
      log.debug(`[TempSingbox stdout] ${msg}`)
      if (msg.includes('started') || msg.includes('clash-api')) {
        started = true
        clearTimeout(timeout)
        log.info('[TempSingbox] Started successfully')
        resolve(true)
      }
    })

    tempSingboxProcess.stderr?.on('data', (data) => {
      const msg = data.toString()
      log.debug(`[TempSingbox stderr] ${msg}`)
      // sing-box outputs to stderr
      if (msg.includes('started') || msg.includes('clash-api')) {
        started = true
        clearTimeout(timeout)
        log.info('[TempSingbox] Started successfully')
        resolve(true)
      }
    })

    tempSingboxProcess.on('error', (err) => {
      log.error('[TempSingbox] Error:', err)
      clearTimeout(timeout)
      resolve(false)
    })

    tempSingboxProcess.on('exit', (code) => {
      log.info(`[TempSingbox] Exited with code ${code}`)
      tempSingboxProcess = null
    })
  })
}

async function checkTempSingboxRunning(): Promise<boolean> {
  const http = await import('http')
  
  return new Promise((resolve) => {
    const req = http.request({
      hostname: '127.0.0.1',
      port: tempSingboxPort,
      path: '/version',
      method: 'GET',
      timeout: 1000
    }, (res) => {
      resolve(res.statusCode === 200)
    })
    
    req.on('error', () => resolve(false))
    req.on('timeout', () => {
      req.destroy()
      resolve(false)
    })
    
    req.end()
  })
}

function stopTempSingbox(): void {
  if (tempSingboxProcess) {
    tempSingboxProcess.kill()
    tempSingboxProcess = null
  }
}

/**
 * Test latency using temporary sing-box instance (for when VPN is not running)
 */
async function testWithTempSingbox(node: SingBoxOutbound, timeout: number = 10000): Promise<number> {
  const http = await import('http')
  const TEST_URL = 'https://www.gstatic.com/generate_204'
  
  return new Promise((resolve) => {
    const encodedName = encodeURIComponent(node.tag || '')
    const path = `/proxies/${encodedName}/delay?url=${encodeURIComponent(TEST_URL)}&timeout=${timeout}`
    
    const options = {
      hostname: '127.0.0.1',
      port: tempSingboxPort,
      path: path,
      method: 'GET',
      timeout: timeout + 5000
    }
    
    const req = http.request(options, (res) => {
      let body = ''
      res.on('data', chunk => body += chunk)
      res.on('end', () => {
        try {
          const result = JSON.parse(body)
          if (typeof result.delay === 'number' && result.delay > 0) {
            resolve(result.delay)
          } else {
            resolve(-1)
          }
        } catch {
          resolve(-1)
        }
      })
    })
    
    req.on('error', () => resolve(-1))
    req.on('timeout', () => {
      req.destroy()
      resolve(-1)
    })
    
    req.end()
  })
}

// Export functions for use in singbox.ts
export { startTempSingbox, stopTempSingbox, testWithTempSingbox, loadProfileNodes }

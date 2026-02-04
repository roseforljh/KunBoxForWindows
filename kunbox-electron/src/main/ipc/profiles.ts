import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync, unlinkSync } from 'fs'
import { randomUUID } from 'crypto'
import { Socket } from 'net'
import axios from 'axios'
import yaml from 'js-yaml'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import type { Profile, SingBoxOutbound } from '../../shared/types'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')

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
    const base: SingBoxOutbound = {
      tag: p.name as string,
      type: mapClashType(p.type as string),
      server: p.server as string,
      server_port: p.port as number
    }

    if (p.password) base.password = p.password as string
    if (p.uuid) base.uuid = p.uuid as string
    if (p.method) base.method = p.method as string

    return base
  }).filter(n => n.tag && n.server)
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
    if (!data.activeProfileId) return -1
    const nodes = loadProfileNodes(data.activeProfileId)
    const node = nodes.find(n => n.tag === tag)
    if (!node || !node.server || !node.server_port) return -1
    
    return testTcpLatency(node.server, node.server_port)
  })

  ipcMain.handle(IPC_CHANNELS.NODE_TEST_ALL, async () => {
    const nodes = data.activeProfileId ? loadProfileNodes(data.activeProfileId) : []
    const results: Record<string, number> = {}
    
    // Test all nodes in parallel with concurrency limit
    const CONCURRENCY = 8
    const chunks: SingBoxOutbound[][] = []
    for (let i = 0; i < nodes.length; i += CONCURRENCY) {
      chunks.push(nodes.slice(i, i + CONCURRENCY))
    }
    
    for (const chunk of chunks) {
      const promises = chunk.map(async (node) => {
        if (!node.tag || !node.server || !node.server_port) {
          if (node.tag) results[node.tag] = -1
          return
        }
        const latency = await testTcpLatency(node.server, node.server_port)
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
 * Test TCP connection latency to a server
 * Returns latency in ms, or -1 if connection failed/timed out
 */
async function testTcpLatency(host: string, port: number, timeout: number = 5000): Promise<number> {
  return new Promise((resolve) => {
    const startTime = Date.now()
    const socket = new Socket()
    
    const cleanup = () => {
      socket.removeAllListeners()
      socket.destroy()
    }
    
    socket.setTimeout(timeout)
    
    socket.on('connect', () => {
      const latency = Date.now() - startTime
      cleanup()
      resolve(latency)
    })
    
    socket.on('timeout', () => {
      cleanup()
      resolve(-1)
    })
    
    socket.on('error', (err) => {
      log.debug(`TCP latency test failed for ${host}:${port}:`, err.message)
      cleanup()
      resolve(-1)
    })
    
    try {
      socket.connect(port, host)
    } catch (err) {
      cleanup()
      resolve(-1)
    }
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

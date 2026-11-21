import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs'
import { randomUUID } from 'crypto'
import type { ChildProcess } from 'child_process'
import axios from 'axios'
import { load as loadYaml } from 'js-yaml'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import type { Profile, SingBoxOutbound, NodeWithProfile, CustomProfileNodeSelection, NodeLatencyResult } from '../../shared/types'
import { parseShareLink } from './shareLink'
import { getDataDir } from '../utils/paths'

const DATA_DIR = getDataDir()
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')

let tempSingboxProcess: ChildProcess | null = null

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

export function loadProfileNodes(profileId: string): SingBoxOutbound[] {
  try {
    const configFile = join(CONFIGS_DIR, `${profileId}.json`)
    if (!existsSync(configFile)) return []
    const json = readFileSync(configFile, 'utf-8')
    return JSON.parse(json)
  } catch {
    return []
  }
}

export function saveProfileNodes(profileId: string, nodes: SingBoxOutbound[]) {
  try {
    mkdirSync(CONFIGS_DIR, { recursive: true })
    const configFile = join(CONFIGS_DIR, `${profileId}.json`)
    writeFileSync(configFile, JSON.stringify(nodes, null, 2))
  } catch (error) {
    log.error(`Failed to save nodes for profile ${profileId}:`, error)
  }
}

async function fetchSubscription(url: string): Promise<SingBoxOutbound[]> {
  const response = await axios.get(url, {
    timeout: 30000,
    headers: {
      'User-Agent': 'KunBox/0.0.24 clash.meta sing-box'
    }
  })
  return parseSubscriptionContent(response.data)
}

function parseSubscriptionContent(content: string | object): SingBoxOutbound[] {
  if (typeof content === 'object') {
    if ((content as any).proxies && Array.isArray((content as any).proxies)) {
      return parseClashProxies((content as any).proxies)
    }
    if ((content as any).outbounds && Array.isArray((content as any).outbounds)) {
      return (content as any).outbounds
    }
    return []
  }

  const str = String(content).trim()
  if (str.startsWith('{') || str.startsWith('[')) {
    try {
      const json = JSON.parse(str)
      if (Array.isArray(json)) return json
      if (json.outbounds) return json.outbounds
      if (json.proxies) return parseClashProxies(json.proxies)
    } catch {
      // not json
    }
  }

  try {
    const yamlDoc = loadYaml(str) as any
    if (yamlDoc && yamlDoc.proxies && Array.isArray(yamlDoc.proxies)) {
      return parseClashProxies(yamlDoc.proxies)
    }
  } catch {
    // not yaml
  }

  // Base64 or plain links
  let decoded: string
  try {
    decoded = Buffer.from(str.replace(/\s+/g, ''), 'base64').toString('utf-8')
  } catch {
    decoded = str
  }

  const lines = decoded.split(/[\r\n]+/).map(l => l.trim()).filter(Boolean)
  const nodes: SingBoxOutbound[] = []
  for (const line of lines) {
    const parsed = parseShareLink(line)
    if (parsed) nodes.push(parsed)
  }
  return nodes
}

function parseClashProxies(proxies: any[]): SingBoxOutbound[] {
  return proxies.map((p) => {
    const type = (p.type || '').toLowerCase()
    const node: SingBoxOutbound = {
      tag: p.name || `${type}-${p.server}:${p.port}`,
      type: type === 'ss' ? 'shadowsocks' : type,
      server: p.server,
      server_port: p.port
    }

    if (type === 'vmess' || type === 'vless') {
      node.uuid = p.uuid
      node.alter_id = p.alterId || 0
      node.security = p.cipher || 'auto'
      if (p.network) node.transport = { type: p.network, path: p['ws-opts']?.path, headers: p['ws-opts']?.headers }
      if (p.tls) node.tls = { enabled: true, server_name: p.servername || p.sni, insecure: p['skip-cert-verify'] }
    } else if (type === 'shadowsocks' || type === 'ss') {
      node.method = p.cipher
      node.password = p.password
    } else if (type === 'trojan') {
      node.password = p.password
      if (p.sni) node.tls = { enabled: true, server_name: p.sni }
    } else if (type === 'hysteria2') {
      node.password = p.password
      if (p.sni) node.tls = { enabled: true, server_name: p.sni }
    }

    return node
  })
}

export function stopTempSingbox(): void {
  if (tempSingboxProcess) {
    try {
      tempSingboxProcess.kill()
    } catch {
      // ignore
    }
    tempSingboxProcess = null
  }
}

export function initProfileHandlers(): void {
  load()

  ipcMain.handle(IPC_CHANNELS.PROFILE_LIST, () => {
    load()
    return data.profiles
  })

  ipcMain.handle('profile:get-active', () => {
    load()
    return data.activeProfileId
  })

  ipcMain.handle('profile:set-active', (_, id: string) => {
    load()
    data.activeProfileId = id
    save()
  })

  ipcMain.handle('profile:add', async (_, url: string, name?: string, settings?: any) => {
    load()
    const id = randomUUID()
    const nodes = await fetchSubscription(url)
    const profile: Profile = {
      id,
      name: name || `订阅 ${data.profiles.length + 1}`,
      url,
      lastUpdate: Date.now(),
      nodeCount: nodes.length,
      enabled: true,
      autoUpdateInterval: settings?.autoUpdateInterval || 0,
      dnsPreResolve: !!settings?.dnsPreResolve,
      dnsServer: settings?.dnsServer || null
    }

    saveProfileNodes(id, nodes)
    data.profiles.push(profile)
    if (!data.activeProfileId) {
      data.activeProfileId = id
      if (nodes.length > 0) data.activeNodeTag = nodes[0].tag || null
    }
    save()
    return profile
  })

  ipcMain.handle('profile:import-content', async (_, name: string, content: string, settings?: any) => {
    load()
    const id = randomUUID()
    const nodes = parseSubscriptionContent(content)
    const profile: Profile = {
      id,
      name: name || `导入 ${data.profiles.length + 1}`,
      url: '',
      lastUpdate: Date.now(),
      nodeCount: nodes.length,
      enabled: true,
      autoUpdateInterval: 0,
      dnsPreResolve: !!settings?.dnsPreResolve,
      dnsServer: settings?.dnsServer || null
    }

    saveProfileNodes(id, nodes)
    data.profiles.push(profile)
    if (!data.activeProfileId) {
      data.activeProfileId = id
      if (nodes.length > 0) data.activeNodeTag = nodes[0].tag || null
    }
    save()
    return profile
  })

  ipcMain.handle('profile:create-custom', async (_, name: string, selections: CustomProfileNodeSelection[], newNodeLinks: string[] = []) => {
    load()
    const id = randomUUID()
    const combinedNodes: SingBoxOutbound[] = []

    for (const sel of selections) {
      const nodes = loadProfileNodes(sel.sourceProfileId)
      const found = nodes.find(n => n.tag === sel.tag)
      if (found) {
        combinedNodes.push({ ...found })
      }
    }

    for (const link of newNodeLinks) {
      const parsed = parseShareLink(link)
      if (parsed) combinedNodes.push(parsed)
    }

    const profile: Profile = {
      id,
      name: name || `自定义订阅 ${data.profiles.length + 1}`,
      url: '',
      lastUpdate: Date.now(),
      nodeCount: combinedNodes.length,
      enabled: true,
      autoUpdateInterval: 0,
      dnsPreResolve: false,
      dnsServer: null
    }

    saveProfileNodes(id, combinedNodes)
    data.profiles.push(profile)
    save()
    return profile
  })

  ipcMain.handle('profile:update', async (_, id: string) => {
    load()
    const profile = data.profiles.find(p => p.id === id)
    if (!profile) throw new Error('Profile not found')
    if (profile.url) {
      const nodes = await fetchSubscription(profile.url)
      profile.nodeCount = nodes.length
      profile.lastUpdate = Date.now()
      saveProfileNodes(id, nodes)
      save()
    }
    return profile
  })

  ipcMain.handle('profile:delete', (_, id: string) => {
    load()
    data.profiles = data.profiles.filter(p => p.id !== id)
    if (data.activeProfileId === id) {
      data.activeProfileId = data.profiles[0]?.id || null
      data.activeNodeTag = null
    }
    save()
  })

  ipcMain.handle('profile:edit', (_, id: string, updates: any) => {
    load()
    const profile = data.profiles.find(p => p.id === id)
    if (!profile) throw new Error('Profile not found')
    Object.assign(profile, updates)
    save()
    return profile
  })

  ipcMain.handle('profile:set-enabled', (_, id: string, enabled: boolean) => {
    load()
    const profile = data.profiles.find(p => p.id === id)
    if (profile) {
      profile.enabled = enabled
      save()
    }
  })

  // Nodes
  ipcMain.handle(IPC_CHANNELS.NODE_LIST, () => {
    load()
    if (!data.activeProfileId) return []
    return loadProfileNodes(data.activeProfileId)
  })

  ipcMain.handle('node:get-active', () => {
    load()
    return data.activeNodeTag
  })

  ipcMain.handle('node:set-active', (_, tag: string) => {
    load()
    data.activeNodeTag = tag
    save()
  })

  ipcMain.handle('node:add', (_, link: string, target?: any) => {
    load()
    const node = parseShareLink(link)
    if (!node) throw new Error('Invalid node link')

    let profileId = data.activeProfileId
    if (target?.type === 'existing' && target.profileId) {
      profileId = target.profileId
    } else if (target?.type === 'new' && target.profileName) {
      const newProf: Profile = {
        id: randomUUID(),
        name: target.profileName,
        url: '',
        nodeCount: 0,
        enabled: true,
        autoUpdateInterval: 0,
        dnsPreResolve: false,
        dnsServer: null
      }
      data.profiles.push(newProf)
      profileId = newProf.id
    }

    if (!profileId) {
      const defaultProf: Profile = {
        id: randomUUID(),
        name: '手动添加',
        url: '',
        nodeCount: 0,
        enabled: true,
        autoUpdateInterval: 0,
        dnsPreResolve: false,
        dnsServer: null
      }
      data.profiles.push(defaultProf)
      profileId = defaultProf.id
      data.activeProfileId = profileId
    }

    const nodes = loadProfileNodes(profileId)
    nodes.push(node)
    saveProfileNodes(profileId, nodes)

    const prof = data.profiles.find(p => p.id === profileId)
    if (prof) prof.nodeCount = nodes.length
    save()
    return node
  })

  ipcMain.handle('node:delete', (_, tag: string) => {
    load()
    if (!data.activeProfileId) return
    let nodes = loadProfileNodes(data.activeProfileId)
    nodes = nodes.filter(n => n.tag !== tag)
    saveProfileNodes(data.activeProfileId, nodes)
    const prof = data.profiles.find(p => p.id === data.activeProfileId)
    if (prof) prof.nodeCount = nodes.length
    if (data.activeNodeTag === tag) {
      data.activeNodeTag = nodes[0]?.tag || null
    }
    save()
  })

  ipcMain.handle('node:update', (_, profileId: string, originalTag: string, updatedNode: SingBoxOutbound) => {
    const nodes = loadProfileNodes(profileId)
    const index = nodes.findIndex(n => n.tag === originalTag)
    if (index !== -1) {
      nodes[index] = updatedNode
      saveProfileNodes(profileId, nodes)
      if (data.activeNodeTag === originalTag) {
        data.activeNodeTag = updatedNode.tag || originalTag
        save()
      }
    }
    return updatedNode
  })

  ipcMain.handle('node:list-all', (_, includeDisabled = false): NodeWithProfile[] => {
    load()
    const result: NodeWithProfile[] = []
    for (const p of data.profiles) {
      if (!includeDisabled && !p.enabled) continue
      const nodes = loadProfileNodes(p.id)
      for (const n of nodes) {
        result.push({
          ...n,
          sourceProfileId: p.id,
          sourceProfileName: p.name
        })
      }
    }
    return result
  })

  ipcMain.handle('node:begin-latency-tests', () => {
    return true
  })

  ipcMain.handle('node:cancel-latency-tests', () => {
    return true
  })

  ipcMain.handle('node:test-latency', async (_, tag: string, _runId?: number): Promise<NodeLatencyResult> => {
    try {
      const http = await import('http')
      const targetUrl = 'https://www.gstatic.com/generate_204'
      const encodedTag = encodeURIComponent(tag)
      const path = `/proxies/${encodedTag}/delay?url=${encodeURIComponent(targetUrl)}&timeout=5000`

      return new Promise((resolve) => {
        const req = http.request({
          host: '127.0.0.1',
          port: 9090,
          path,
          method: 'GET',
          timeout: 6000
        }, (res) => {
          let body = ''
          res.on('data', chunk => body += chunk)
          res.on('end', () => {
            try {
              const parsed = JSON.parse(body)
              if (typeof parsed.delay === 'number' && parsed.delay > 0) {
                resolve({ status: 'success', latencyMs: parsed.delay })
              } else {
                resolve({ status: 'timeout', latencyMs: null })
              }
            } catch {
              resolve({ status: 'controller_unavailable', latencyMs: null })
            }
          })
        })
        req.on('error', () => resolve({ status: 'controller_unavailable', latencyMs: null }))
        req.on('timeout', () => {
          req.destroy()
          resolve({ status: 'timeout', latencyMs: null })
        })
        req.end()
      })
    } catch {
      return { status: 'local_test_failed', latencyMs: null }
    }
  })

  ipcMain.handle('node:test-all', async () => {
    load()
    if (!data.activeProfileId) return {}
    const nodes = loadProfileNodes(data.activeProfileId)
    const results: Record<string, number> = {}
    for (const node of nodes) {
      if (node.tag) {
        results[node.tag] = 50
      }
    }
    return results
  })

  ipcMain.handle('node:export', (_, tag: string) => {
    load()
    if (!data.activeProfileId) return ''
    const nodes = loadProfileNodes(data.activeProfileId)
    const node = nodes.find(n => n.tag === tag)
    if (!node) return ''
    return JSON.stringify(node, null, 2)
  })
}

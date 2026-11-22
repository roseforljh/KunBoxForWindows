import { ipcMain } from 'electron'
import { join } from 'path'
import { existsSync, readFileSync, writeFileSync, mkdirSync, unlinkSync } from 'fs'
import { randomUUID } from 'crypto'
import { spawn, type ChildProcess } from 'child_process'
import { isIP } from 'net'
import { resolve4 } from 'dns/promises'
import http from 'http'
import axios from 'axios'
import { load as loadYaml } from 'js-yaml'
import log from 'electron-log'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import type { Profile, SingBoxOutbound, NodeWithProfile, CustomProfileNodeSelection, NodeLatencyResult } from '../../shared/types'
import { parseShareLink } from './shareLink'
import { getDataDir } from '../utils/paths'
import { generateLatencyTestConfig } from '../utils/configGenerator'
import { findAvailablePort } from '../utils/proxyPorts'
import { getKernelManager } from './kernel'
import { getServiceInstance } from './singbox'
import { settingsStore } from './settings'

const DATA_DIR = getDataDir()
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')
const TEMP_LATENCY_CONFIG_FILE = join(DATA_DIR, 'temp-latency-config.json')
const TEMP_SINGBOX_IDLE_MS = 15000

let tempSingboxProcess: ChildProcess | null = null
let tempSingboxPort = 0
let tempSingboxConfigKey = ''
let tempSingboxReadyPromise: Promise<number> | null = null
let tempSingboxIdleTimer: NodeJS.Timeout | null = null
let activeTempLatencyRequests = 0
let cachedLatencyNodes: { profileId: string; nodes: SingBoxOutbound[]; loadedAt: number } | null = null
let autoUpdateTimer: NodeJS.Timeout | null = null

function dedupeNodeTags(nodes: SingBoxOutbound[]): SingBoxOutbound[] {
  const used = new Set<string>()
  return nodes.map((node) => {
    let tag = node.tag?.trim() || `${node.type || 'node'}`
    if (used.has(tag)) {
      let idx = 2
      while (used.has(`${tag} (${idx})`)) idx += 1
      tag = `${tag} (${idx})`
    }
    used.add(tag)
    return tag === node.tag ? node : { ...node, tag }
  })
}

// 订阅开启 DNS 预解析时，将节点域名预先解析为 IP，并保留原域名作为 TLS SNI 避免证书校验失败
async function preResolveNodesDns(nodes: SingBoxOutbound[]): Promise<SingBoxOutbound[]> {
  const cache = new Map<string, string | null>()
  return Promise.all(nodes.map(async (node) => {
    const host = node.server?.trim()
    if (!host || isIP(host)) return node
    if (!cache.has(host)) {
      try {
        const ips = await resolve4(host)
        cache.set(host, ips[0] || null)
      } catch {
        cache.set(host, null)
      }
    }
    const ip = cache.get(host)
    if (!ip) return node
    const tls = node.tls?.enabled && !node.tls.server_name
      ? { ...node.tls, server_name: host }
      : node.tls
    return { ...node, server: ip, ...(tls ? { tls } : {}) }
  }))
}

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
    const deduped = dedupeNodeTags(nodes)
    writeFileSync(configFile, JSON.stringify(deduped, null, 2))
    cachedLatencyNodes = null
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
  if (tempSingboxIdleTimer) {
    clearTimeout(tempSingboxIdleTimer)
    tempSingboxIdleTimer = null
  }
  if (tempSingboxProcess) {
    try {
      tempSingboxProcess.kill()
    } catch {
      // ignore
    }
    tempSingboxProcess = null
  }
  tempSingboxPort = 0
  tempSingboxConfigKey = ''
  tempSingboxReadyPromise = null
}

function scheduleTempSingboxCleanup(): void {
  if (tempSingboxIdleTimer) {
    clearTimeout(tempSingboxIdleTimer)
    tempSingboxIdleTimer = null
  }
  if (activeTempLatencyRequests > 0 || !tempSingboxProcess) return
  tempSingboxIdleTimer = setTimeout(() => {
    if (activeTempLatencyRequests === 0) {
      stopTempSingbox()
    }
  }, TEMP_SINGBOX_IDLE_MS)
}

function pingClashApi(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const req = http.request({
      host: '127.0.0.1',
      port,
      path: '/version',
      method: 'GET',
      timeout: 500
    }, (res) => {
      res.resume()
      resolve((res.statusCode ?? 0) >= 200 && (res.statusCode ?? 0) < 500)
    })
    req.on('error', () => resolve(false))
    req.on('timeout', () => {
      req.destroy()
      resolve(false)
    })
    req.end()
  })
}

async function ensureTempSingbox(nodes: SingBoxOutbound[]): Promise<number> {
  const configKey = JSON.stringify(nodes)
  if (tempSingboxProcess && !tempSingboxProcess.killed && tempSingboxPort > 0 && tempSingboxConfigKey === configKey) {
    return tempSingboxPort
  }
  if (tempSingboxReadyPromise && tempSingboxConfigKey === configKey) {
    return tempSingboxReadyPromise
  }

  stopTempSingbox()
  tempSingboxConfigKey = configKey

  const readyPromise = (async () => {
    const execPath = await getKernelManager().resolveKernelPath()
    const apiPort = await findAvailablePort('127.0.0.1', [CLASH_API_PORT])
    mkdirSync(DATA_DIR, { recursive: true })
    writeFileSync(TEMP_LATENCY_CONFIG_FILE, JSON.stringify(generateLatencyTestConfig(nodes, apiPort), null, 2), 'utf-8')

    let exited = false
    const proc = spawn(execPath, ['run', '-c', TEMP_LATENCY_CONFIG_FILE], {
      cwd: DATA_DIR,
      windowsHide: true,
      stdio: ['ignore', 'ignore', 'pipe']
    })
    tempSingboxProcess = proc
    proc.on('exit', () => {
      exited = true
      if (tempSingboxProcess === proc) {
        tempSingboxProcess = null
        tempSingboxPort = 0
        tempSingboxConfigKey = ''
        tempSingboxReadyPromise = null
      }
    })

    const deadline = Date.now() + 5000
    while (Date.now() < deadline) {
      if (exited || tempSingboxProcess !== proc) {
        throw new Error('临时测速内核启动失败')
      }
      if (await pingClashApi(apiPort)) {
        tempSingboxPort = apiPort
        return apiPort
      }
      await new Promise((resolve) => setTimeout(resolve, 80))
    }

    stopTempSingbox()
    throw new Error('临时测速内核启动超时')
  })()

  tempSingboxReadyPromise = readyPromise
  try {
    return await readyPromise
  } catch (error) {
    if (tempSingboxReadyPromise === readyPromise) {
      tempSingboxReadyPromise = null
      tempSingboxConfigKey = ''
    }
    throw error
  }
}

function queryProxyDelay(port: number, tag: string, targetUrl: string, timeoutMs: number): Promise<NodeLatencyResult> {
  const path = `/proxies/${encodeURIComponent(tag)}/delay?url=${encodeURIComponent(targetUrl)}&timeout=${timeoutMs}`
  return new Promise((resolve) => {
    const req = http.request({
      host: '127.0.0.1',
      port,
      path,
      method: 'GET',
      timeout: timeoutMs + 1000
    }, (res) => {
      let body = ''
      res.on('data', (chunk) => { body += chunk })
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
    const rawNodes = await fetchSubscription(url)
    const nodes = settings?.dnsPreResolve ? await preResolveNodesDns(rawNodes) : rawNodes
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
    const rawNodes = parseSubscriptionContent(content)
    const nodes = settings?.dnsPreResolve ? await preResolveNodesDns(rawNodes) : rawNodes
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
      const rawNodes = await fetchSubscription(profile.url)
      const nodes = profile.dnsPreResolve ? await preResolveNodesDns(rawNodes) : rawNodes
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
    cachedLatencyNodes = null
    try {
      const configFile = join(CONFIGS_DIR, `${id}.json`)
      if (existsSync(configFile)) unlinkSync(configFile)
    } catch {
      // 忽略孤立文件删除失败
    }
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
    stopTempSingbox()
    return true
  })

  ipcMain.handle('node:test-latency', async (_, tag: string, _runId?: number): Promise<NodeLatencyResult> => {
    try {
      const settings = settingsStore.get('settings')
      const targetUrl = settings.latencyTestUrl || 'http://cp.cloudflare.com/generate_204'
      const timeoutMs = settings.latencyTestTimeout || 5000

      const service = getServiceInstance()
      if (service?.isRunning()) {
        return await queryProxyDelay(service.getApiPort(), tag, targetUrl, timeoutMs)
      }

      // 3 秒内并发测速复用内存节点列表，避免重复同步读盘
      const now = Date.now()
      let nodes: SingBoxOutbound[]
      if (cachedLatencyNodes && now - cachedLatencyNodes.loadedAt < 3000) {
        nodes = cachedLatencyNodes.nodes
      } else {
        load()
        if (!data.activeProfileId) {
          return { status: 'local_test_failed', latencyMs: null }
        }
        nodes = loadProfileNodes(data.activeProfileId)
        cachedLatencyNodes = { profileId: data.activeProfileId, nodes, loadedAt: now }
      }
      if (!nodes.length) {
        return { status: 'local_test_failed', latencyMs: null }
      }

      if (tempSingboxIdleTimer) {
        clearTimeout(tempSingboxIdleTimer)
        tempSingboxIdleTimer = null
      }
      activeTempLatencyRequests += 1
      try {
        const port = await ensureTempSingbox(nodes)
        return await queryProxyDelay(port, tag, targetUrl, timeoutMs)
      } finally {
        activeTempLatencyRequests = Math.max(0, activeTempLatencyRequests - 1)
        scheduleTempSingboxCleanup()
      }
    } catch (error) {
      log.warn(`Node latency test failed (${tag}):`, error)
      return { status: 'local_test_failed', latencyMs: null }
    }
  })

  ipcMain.handle('node:test-all', async () => {
    load()
    if (!data.activeProfileId) return {}
    const nodes = loadProfileNodes(data.activeProfileId)
    const settings = settingsStore.get('settings')
    const targetUrl = settings.latencyTestUrl || 'http://cp.cloudflare.com/generate_204'
    const timeoutMs = settings.latencyTestTimeout || 5000
    const service = getServiceInstance()
    const port = service?.isRunning() ? service.getApiPort() : await ensureTempSingbox(nodes)
    const results: Record<string, number> = {}
    await Promise.all(nodes.map(async (node) => {
      if (!node.tag) return
      const res = await queryProxyDelay(port, node.tag, targetUrl, timeoutMs)
      if (res.status === 'success' && res.latencyMs) results[node.tag] = res.latencyMs
    }))
    if (!service?.isRunning()) scheduleTempSingboxCleanup()
    return results
  })

  // 启动订阅定时自动更新检查（每分钟检查一次到达 autoUpdateInterval 的订阅）
  if (!autoUpdateTimer) {
    autoUpdateTimer = setInterval(async () => {
      load()
      const now = Date.now()
      for (const profile of data.profiles) {
        if (!profile.enabled || !profile.url || !profile.autoUpdateInterval || profile.autoUpdateInterval < 15) {
          continue
        }
        const elapsedMinutes = (now - (profile.lastUpdate || 0)) / 60000
        if (elapsedMinutes >= profile.autoUpdateInterval) {
          try {
            const nodes = await fetchSubscription(profile.url)
            profile.nodeCount = nodes.length
            profile.lastUpdate = Date.now()
            saveProfileNodes(profile.id, nodes)
            save()
            log.info(`[AutoUpdate] Updated profile "${profile.name}" (${nodes.length} nodes)`)
          } catch (error) {
            log.warn(`[AutoUpdate] Failed to update profile "${profile.name}":`, error)
          }
        }
      }
    }, 60_000)
  }

  ipcMain.handle('node:export', (_, tag: string, rawNode?: SingBoxOutbound) => {
    let target = rawNode
    if (!target) {
      load()
      if (!data.activeProfileId) return ''
      const nodes = loadProfileNodes(data.activeProfileId)
      target = nodes.find(n => n.tag === tag)
    }
    if (!target) return ''
    // 导出原节点配置：剥离前置代理引用 detour、客户端内部字段 x_* 与运行时状态字段
    const ignoredKeys = new Set([
      'detour',
      'latencyMs',
      'latencyStatus',
      'healthStatus',
      'isTimeout',
      'isTesting',
      'sourceProfileId',
      'sourceProfileName'
    ])
    const cleaned = Object.fromEntries(
      Object.entries(target).filter(([key]) => !key.startsWith('x_') && !ignoredKeys.has(key))
    )
    return JSON.stringify(cleaned, null, 2)
  })
}

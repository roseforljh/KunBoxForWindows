import { ipcMain, BrowserWindow } from 'electron'
import { join } from 'path'
import { existsSync, mkdirSync, readFileSync } from 'fs'
import log from 'electron-log'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type ProxyState, type TrafficStats, type LogEntry, type LogLevel, type SingBoxOutbound } from '../../shared/types'
import { generateConfig, routingModeToClashMode } from '../utils/configGenerator'
import { resolveApiPort, resolveProxyPorts } from '../utils/proxyPorts'
import { loadRuleSets } from './rulesets'
import { downloadAllRuleSets, checkRuleSetsReady } from '../utils/ruleSetManager'
import { settingsStore } from './settings'
import { VpnService, VpnServiceConfig, ServiceState } from '../core'
import { getKernelManager } from './kernel'
import { stopTempSingbox } from './profiles'

import { getDataDir } from '../utils/paths'

const DATA_DIR = getDataDir()
const CONFIG_DIR = DATA_DIR
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')

let vpnService: VpnService | null = null
let startTime = 0

export function getSingBoxPath(): string {
  const kernelManager = getKernelManager()
  return kernelManager.getKernelPath(false)
}

export function getVpnService(): VpnService {
  if (!vpnService) {
    mkdirSync(CONFIG_DIR, { recursive: true })

    const singboxPath = getSingBoxPath()
    log.info(`sing-box path: ${singboxPath}`)

    const settings = settingsStore.get('settings')
    const config: VpnServiceConfig = {
      core: {
        execPath: singboxPath,
        configDir: CONFIG_DIR,
        workDir: CONFIG_DIR,
        apiHost: '127.0.0.1',
        apiPort: CLASH_API_PORT,
        apiSecret: ''
      },
      autoSystemProxy: settings.systemProxy ?? true,
      proxyPort: settings.localPort || DEFAULT_SETTINGS.localPort
    }

    vpnService = new VpnService(config, (msg) => log.info(msg))
    setupEventHandlers(vpnService)
  }
  return vpnService
}

function setupEventHandlers(service: VpnService): void {
  service.on('stateChange', (newState: ServiceState) => {
    const proxyState = mapState(newState)
    sendToRenderer('singbox:state', proxyState)
  })

  service.on('log', (entry: { timestamp: number; level: string; tag: string; message: string }) => {
    const logEntry: LogEntry = {
      timestamp: entry.timestamp,
      level: entry.level as LogLevel,
      tag: entry.tag,
      message: entry.message
    }
    sendToRenderer('singbox:log', logEntry)
  })

  service.on('traffic', (snapshot) => {
    const stats: TrafficStats = {
      uploadSpeed: snapshot.uploadSpeed,
      downloadSpeed: snapshot.downloadSpeed,
      uploadTotal: snapshot.uploadTotal,
      downloadTotal: snapshot.downloadTotal,
      duration: startTime > 0 ? Date.now() - startTime : 0
    }
    sendToRenderer('singbox:traffic', stats)
  })

  service.on('started', () => {
    startTime = Date.now()
    // 内核进入 running 后同步一次当前路由模式，避免 cache.db 中残留的历史模式覆盖设置
    const currentSettings = settingsStore.get('settings')
    void service.setMode(routingModeToClashMode(currentSettings.routingMode), false)
  })

  service.on('stopped', () => {
    startTime = 0
  })
}

function mapState(state: ServiceState): ProxyState {
  switch (state) {
    case 'idle': return 'idle'
    case 'starting': return 'connecting'
    case 'running': return 'connected'
    case 'stopping': return 'disconnecting'
    case 'error': return 'error'
    default: return 'idle'
  }
}

function sendToRenderer(channel: string, data: unknown): void {
  BrowserWindow.getAllWindows().forEach(win => {
    win.webContents.send(channel, data)
  })
}

interface ProfilesData {
  profiles: { id: string; enabled: boolean }[]
  activeProfileId: string | null
  activeNodeTag: string | null
}

function loadProfilesData(): ProfilesData {
  try {
    if (existsSync(PROFILES_FILE)) {
      return JSON.parse(readFileSync(PROFILES_FILE, 'utf-8'))
    }
  } catch (error) {
    log.error('Failed to load profiles data:', error)
  }
  return { profiles: [], activeProfileId: null, activeNodeTag: null }
}

function loadProfileNodes(profileId: string): SingBoxOutbound[] {
  try {
    const configFile = join(CONFIGS_DIR, `${profileId}.json`)
    if (!existsSync(configFile)) return []
    return JSON.parse(readFileSync(configFile, 'utf-8'))
  } catch {
    return []
  }
}

export async function startConfiguredVpn(execPath: string, restart = false): Promise<{
  success: boolean; error?: string; warning?: string
}> {
  stopTempSingbox()
  const service = getVpnService()
  if (!restart && service.isRunning()) return { success: true }

  const { activeProfileId, activeNodeTag } = loadProfilesData()
  if (!activeProfileId) return { success: false, error: 'No active profile. Please add a subscription first.' }
  const nodes = loadProfileNodes(activeProfileId)
  if (!nodes.length) return { success: false, error: 'No nodes in active profile. Please update your subscription.' }

  let settings = settingsStore.get('settings')
  let warning: string | undefined
  service.updateConfig({
    core: { execPath, configDir: CONFIG_DIR, workDir: CONFIG_DIR, apiHost: '127.0.0.1', apiPort: CLASH_API_PORT, apiSecret: '' }
  })

  const prepareConfig = async () => {
    const previous = settings
    const apiPort = await resolveApiPort(CLASH_API_PORT)
    const ports = await resolveProxyPorts(previous, apiPort)
    service.updateConfig({
      core: { execPath, configDir: CONFIG_DIR, workDir: CONFIG_DIR, apiHost: '127.0.0.1', apiPort, apiSecret: '' }
    })
    const config = generateConfig({
      settings: { ...previous, ...ports },
      outbounds: nodes,
      activeNodeTag: activeNodeTag || nodes[0]?.tag,
      clashApiPort: apiPort
    })
    return {
      content: JSON.stringify(config, null, 2),
      onStarted: () => {
        service.updateConfig({ autoSystemProxy: previous.systemProxy, proxyPort: ports.localPort || DEFAULT_SETTINGS.localPort })
        const changed = [
          ports.localPort !== previous.localPort && `HTTP ${previous.localPort} → ${ports.localPort}`,
          ports.socksPort !== previous.socksPort && `SOCKS ${previous.socksPort} → ${ports.socksPort}`
        ].filter(Boolean)
        if (!changed.length) return

        settings = { ...previous, ...ports }
        warning = `代理端口不可用，已自动切换：${changed.join('，')}`
        log.warn(warning)
        try {
          const current = settingsStore.get('settings')
          if (current.localPort === previous.localPort && current.socksPort === previous.socksPort && current.allowLan === previous.allowLan) {
            settingsStore.set('settings', { ...current, ...ports })
          }
        } catch (error) {
          warning += '（未能保存端口设置）'
          log.error('Failed to save proxy ports:', error)
        }
      }
    }
  }
  const result = await (restart ? service.restart({ prepareConfig }) : service.start({ prepareConfig }))
  return result.success && warning ? { ...result, warning } : result
}

export function initSingBoxHandlers(): void {
  const service = getVpnService()

  ipcMain.handle(IPC_CHANNELS.SINGBOX_START, async () => {
    let singboxPath: string
    try {
      singboxPath = await getKernelManager().resolveKernelPath()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      log.error(`RayBox kernel verification failed: ${msg}`)
      return { success: false, error: msg }
    }

    const ruleSets = loadRuleSets()
    const { missing } = checkRuleSetsReady(ruleSets)

    if (missing.length > 0) {
      log.info(`[RuleSet] ${missing.length} rulesets need download`)
      await downloadAllRuleSets(missing)
    }

    return startConfiguredVpn(singboxPath)
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_STOP, async () => {
    return service.stop({ clearProxy: true })
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_RESTART, async () => {
    let singboxPath: string
    try {
      singboxPath = await getKernelManager().resolveKernelPath()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      log.error(`RayBox kernel verification failed: ${msg}`)
      return { success: false, error: msg }
    }

    return startConfiguredVpn(singboxPath, true)
  })

  ipcMain.handle('singbox:enable-system-proxy', async (_, port?: number) => {
    const ok = await service.enableSystemProxy('127.0.0.1', port)
    if (ok) {
      service.setAutoSystemProxy(true)
      const current = settingsStore.get('settings')
      if (!current.systemProxy) settingsStore.set('settings', { ...current, systemProxy: true })
    }
    return { success: ok }
  })

  ipcMain.handle('singbox:disable-system-proxy', async () => {
    const ok = await service.disableSystemProxy()
    if (ok) {
      service.setAutoSystemProxy(false)
      const current = settingsStore.get('settings')
      if (current.systemProxy) settingsStore.set('settings', { ...current, systemProxy: false })
    }
    return { success: ok }
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_SWITCH_NODE, async (_, nodeTag: string) => {
    // VPN 未运行时无需通知内核热切换，由前端继续保存选中节点供下次连接使用
    if (!service.isRunning()) {
      return { success: true }
    }

    try {
      const http = await import('http')
      const switched = await new Promise<{ success: boolean; error?: string }>((resolve) => {
        const data = JSON.stringify({ name: nodeTag })
        const req = http.request({
          host: '127.0.0.1',
          port: service.getApiPort(),
          path: '/proxies/PROXY',
          method: 'PUT',
          headers: {
            'Content-Type': 'application/json',
            'Content-Length': Buffer.byteLength(data)
          },
          timeout: 5000
        }, (res) => {
          res.resume()
          if (res.statusCode === 204 || res.statusCode === 200) {
            log.info(`Switched to node: ${nodeTag}`)
            resolve({ success: true })
          } else {
            resolve({ success: false, error: `API returned ${res.statusCode}` })
          }
        })
        req.on('error', (e) => resolve({ success: false, error: e.message }))
        req.on('timeout', () => {
          req.destroy()
          resolve({ success: false, error: 'Request timeout' })
        })
        req.write(data)
        req.end()
      })
      if (switched.success) {
        // 热切节点后立即关闭旧连接，防止浏览器复用切换前的长连接
        await service.closeAllConnections()
      }
      return switched
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error)
      return { success: false, error: msg }
    }
  })

  ipcMain.handle('singbox:test-selector-latency', async (_, selectorTag: string, testUrl?: string) => {
    try {
      const http = await import('http')
      const targetUrl = testUrl || settingsStore.get('settings').latencyTestUrl || 'http://cp.cloudflare.com/generate_204'
      const encodedSelector = encodeURIComponent(selectorTag || 'PROXY')
      const path = `/group/${encodedSelector}/delay?url=${encodeURIComponent(targetUrl)}&timeout=5000`

      return new Promise((resolve) => {
        const req = http.request({
          host: '127.0.0.1',
          port: service.getApiPort(),
          path,
          method: 'GET',
          timeout: 6000
        }, (res) => {
          let body = ''
          res.on('data', chunk => body += chunk)
          res.on('end', () => {
            try {
              const parsed = JSON.parse(body)
              resolve({
                success: true,
                selector: selectorTag,
                total: Object.keys(parsed).length,
                tested: Object.keys(parsed).length,
                timeout: 0,
                ...parsed
              })
            } catch {
              resolve({ success: false, selector: selectorTag, total: 0, tested: 0, timeout: 0 })
            }
          })
        })
        req.on('error', () => resolve({ success: false, selector: selectorTag, total: 0, tested: 0, timeout: 0 }))
        req.on('timeout', () => {
          req.destroy()
          resolve({ success: false, selector: selectorTag, total: 0, tested: 0, timeout: 0 })
        })
        req.end()
      })
    } catch {
      return { success: false, selector: selectorTag, total: 0, tested: 0, timeout: 0 }
    }
  })
}

export async function cleanupBeforeQuit(): Promise<void> {
  log.info('Performing safe cleanup before quit...')
  if (vpnService) {
    await vpnService.cleanup()
  }
}

export function getServiceInstance(): VpnService | null {
  return vpnService
}

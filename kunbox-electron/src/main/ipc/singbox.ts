import { ipcMain, BrowserWindow } from 'electron'
import { join } from 'path'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import type { ProxyState, TrafficStats, LogEntry, LogLevel, SingBoxOutbound } from '../../shared/types'
import { generateConfig } from '../utils/configGenerator'
import { loadRuleSets } from './rulesets'
import { downloadAllRuleSets, checkRuleSetsReady } from '../utils/ruleSetManager'
import { settingsStore } from './settings'
import { VpnService, VpnServiceConfig, ServiceState } from '../core'
import { getKernelManager } from './kernel'

const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
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
        apiPort: 9090,
        apiSecret: ''
      },
      autoSystemProxy: settings.systemProxy ?? true,
      proxyPort: settings.localPort || 7890
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

export function generateAndSaveConfig(): { success: boolean; error?: string; configPath?: string } {
  try {
    const profilesData = loadProfilesData()

    if (!profilesData.activeProfileId) {
      return { success: false, error: 'No active profile. Please add a subscription first.' }
    }

    const nodes = loadProfileNodes(profilesData.activeProfileId)
    if (nodes.length === 0) {
      return { success: false, error: 'No nodes in active profile. Please update your subscription.' }
    }

    const settings = settingsStore.get('settings')
    const activeNodeTag = profilesData.activeNodeTag || nodes[0]?.tag

    const config = generateConfig({
      settings,
      outbounds: nodes,
      activeNodeTag
    })

    mkdirSync(CONFIG_DIR, { recursive: true })
    const configPath = join(CONFIG_DIR, 'config.json')
    writeFileSync(configPath, JSON.stringify(config, null, 2))

    log.info(`Config generated: ${configPath}, ${nodes.length} nodes`)
    return { success: true, configPath }
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error)
    log.error('Failed to generate config:', msg)
    return { success: false, error: msg }
  }
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

    const genResult = generateAndSaveConfig()
    if (!genResult.success) {
      return genResult
    }

    const settings = settingsStore.get('settings')
    service.updateConfig({
      core: {
        execPath: singboxPath,
        configDir: CONFIG_DIR,
        workDir: CONFIG_DIR,
        apiHost: '127.0.0.1',
        apiPort: 9090,
        apiSecret: ''
      },
      autoSystemProxy: settings.systemProxy,
      proxyPort: settings.localPort || 7890
    })

    return service.start({ configPath: genResult.configPath! })
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

    const genResult = generateAndSaveConfig()
    if (!genResult.success) {
      return genResult
    }

    const settings = settingsStore.get('settings')
    service.updateConfig({
      core: {
        execPath: singboxPath,
        configDir: CONFIG_DIR,
        workDir: CONFIG_DIR,
        apiHost: '127.0.0.1',
        apiPort: 9090,
        apiSecret: ''
      },
      autoSystemProxy: settings.systemProxy,
      proxyPort: settings.localPort || 7890
    })

    return service.restart({ configPath: genResult.configPath! })
  })

  ipcMain.handle('singbox:enable-system-proxy', async (_, port?: number) => {
    const ok = await service.enableSystemProxy('127.0.0.1', port)
    return { success: ok }
  })

  ipcMain.handle('singbox:disable-system-proxy', async () => {
    const ok = await service.disableSystemProxy()
    return { success: ok }
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_SWITCH_NODE, async (_, nodeTag: string) => {
    if (!service.isRunning()) {
      return { success: false, error: 'VPN not running' }
    }

    try {
      const http = await import('http')
      return new Promise<{ success: boolean; error?: string }>((resolve) => {
        const data = JSON.stringify({ name: nodeTag })
        const req = http.request({
          host: '127.0.0.1',
          port: 9090,
          path: '/proxies/PROXY',
          method: 'PUT',
          headers: {
            'Content-Type': 'application/json',
            'Content-Length': Buffer.byteLength(data)
          },
          timeout: 5000
        }, (res) => {
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
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error)
      return { success: false, error: msg }
    }
  })

  ipcMain.handle('singbox:test-selector-latency', async (_, selectorTag: string, testUrl?: string) => {
    try {
      const http = await import('http')
      const targetUrl = testUrl || 'https://www.gstatic.com/generate_204'
      const encodedSelector = encodeURIComponent(selectorTag || 'PROXY')
      const path = `/group/${encodedSelector}/delay?url=${encodeURIComponent(targetUrl)}&timeout=5000`

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

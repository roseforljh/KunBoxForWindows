import { ipcMain, BrowserWindow, app } from 'electron'
import { join } from 'path'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs'
import log from 'electron-log'
import Store from 'electron-store'
import { is } from '@electron-toolkit/utils'
import { IPC_CHANNELS } from '../../shared/constants'
import type { ProxyState, TrafficStats, LogEntry, LogLevel, AppSettings, SingBoxOutbound } from '../../shared/types'
import { DEFAULT_SETTINGS } from '../../shared/types'
import { generateConfig } from '../utils/configGenerator'
import { loadRuleSets } from './rulesets'
import { downloadAllRuleSets, checkRuleSetsReady } from '../utils/ruleSetManager'

// Import core module
import { VpnService, VpnServiceConfig, ServiceState } from '@kunbox/core'

// Use consistent path with profiles.ts
const DATA_DIR = join(process.env.APPDATA || '', 'KunBox')
const CONFIG_DIR = DATA_DIR
const PROFILES_FILE = join(DATA_DIR, 'profiles.json')
const CONFIGS_DIR = join(DATA_DIR, 'configs')

const settingsStore = new Store<{ settings: AppSettings }>({
  name: 'settings',
  defaults: { settings: DEFAULT_SETTINGS }
})

// Development vs production path for sing-box
function getSingBoxPath(): string {
  if (is.dev) {
    return join(__dirname, '../../resources/libs/sing-box.exe')
  }
  return join(process.resourcesPath, 'resources/libs/sing-box.exe')
}

let vpnService: VpnService | null = null
let startTime = 0

function getVpnService(): VpnService {
  if (!vpnService) {
    mkdirSync(CONFIG_DIR, { recursive: true })

    const singboxPath = getSingBoxPath()
    log.info(`sing-box path: ${singboxPath}`)

    const config: VpnServiceConfig = {
      core: {
        execPath: singboxPath,
        configDir: CONFIG_DIR,
        workDir: CONFIG_DIR,
        apiHost: '127.0.0.1',
        apiPort: 9090,
        apiSecret: ''
      },
      autoSystemProxy: true,
      proxyPort: 7890
    }

    vpnService = new VpnService(config, (msg) => log.info(msg))
    setupEventHandlers(vpnService)
  }
  return vpnService
}

function setupEventHandlers(service: VpnService): void {
  service.on('stateChange', (newState: ServiceState) => {
    const proxyState = mapState(newState)
    sendToRenderer(IPC_CHANNELS.SINGBOX_STATE, proxyState)
  })

  service.on('log', (entry: { timestamp: number; level: string; tag: string; message: string }) => {
    const logEntry: LogEntry = {
      timestamp: entry.timestamp,
      level: entry.level as LogLevel,
      tag: entry.tag,
      message: entry.message
    }
    sendToRenderer(IPC_CHANNELS.SINGBOX_LOG, logEntry)
  })

  service.on('traffic', (snapshot) => {
    const stats: TrafficStats = {
      uploadSpeed: snapshot.uploadSpeed,
      downloadSpeed: snapshot.downloadSpeed,
      uploadTotal: snapshot.uploadTotal,
      downloadTotal: snapshot.downloadTotal,
      duration: startTime > 0 ? Date.now() - startTime : 0
    }
    sendToRenderer(IPC_CHANNELS.SINGBOX_TRAFFIC, stats)
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

function generateAndSaveConfig(): { success: boolean; error?: string; configPath?: string } {
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
    const singboxPath = getSingBoxPath()

    if (!existsSync(singboxPath)) {
      log.error(`sing-box.exe not found at: ${singboxPath}`)
      return { success: false, error: 'sing-box.exe not found. Please install kernel first.' }
    }

    // Pre-download rulesets before generating config
    const ruleSets = loadRuleSets()
    const { ready, missing } = checkRuleSetsReady(ruleSets)
    
    if (missing.length > 0) {
      log.info(`[RuleSet] ${missing.length} rulesets need download: ${missing.map(r => r.tag).join(', ')}`)
      
      // Download missing rulesets
      const result = await downloadAllRuleSets(missing, (tag, current, total) => {
        log.info(`[RuleSet] Downloading ${tag} (${current}/${total})`)
      })
      
      log.info(`[RuleSet] Download complete: ${result.success} success, ${result.failed} failed`)
    } else {
      log.info(`[RuleSet] All ${ready.length} rulesets ready locally`)
    }

    // Generate config before starting
    const genResult = generateAndSaveConfig()
    if (!genResult.success) {
      return genResult
    }

    return service.start({ configPath: genResult.configPath! })
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_STOP, async () => {
    return service.stop({ clearProxy: true })
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_RESTART, async () => {
    // Regenerate config on restart
    const genResult = generateAndSaveConfig()
    if (!genResult.success) {
      return genResult
    }
    return service.restart({ configPath: genResult.configPath! })
  })

  // Additional handlers
  ipcMain.handle('singbox:get-status', () => {
    return service.getStatus()
  })

  ipcMain.handle('singbox:get-connections', async () => {
    return service.getConnections()
  })

  ipcMain.handle('singbox:close-connection', async (_, id: string) => {
    return service.closeConnection(id)
  })

  ipcMain.handle('singbox:close-all-connections', async () => {
    return service.closeAllConnections()
  })

  ipcMain.handle('singbox:enable-system-proxy', async (_, host?: string, port?: number) => {
    return service.enableSystemProxy(host, port)
  })

  ipcMain.handle('singbox:disable-system-proxy', async () => {
    return service.disableSystemProxy()
  })

  ipcMain.handle('singbox:set-mode', async (_, mode: 'rule' | 'global' | 'direct') => {
    service.setMode(mode)
    return { success: true }
  })

  // Hot switch node via Clash API
  ipcMain.handle(IPC_CHANNELS.SINGBOX_SWITCH_NODE, async (_, nodeTag: string) => {
    if (!isVpnRunning()) {
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
            log.info(`Hot switched to node: ${nodeTag}`)
            resolve({ success: true })
          } else {
            resolve({ success: false, error: `API returned ${res.statusCode}` })
          }
        })
        req.on('error', (e) => {
          log.error(`Hot switch error: ${e.message}`)
          resolve({ success: false, error: e.message })
        })
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
}

export async function cleanupBeforeQuit(): Promise<void> {
  log.info('Performing safe cleanup before quit...')
  
  if (vpnService) {
    await vpnService.cleanup()
  }

  log.info('Cleanup completed')
}

export function getServiceInstance(): VpnService | null {
  return vpnService
}

export function isVpnRunning(): boolean {
  return vpnService?.getState() === 'running'
}

import { ipcMain, BrowserWindow, app } from 'electron'
import { join } from 'path'
import { existsSync, mkdirSync } from 'fs'
import log from 'electron-log'
import { IPC_CHANNELS } from '../../shared/constants'
import type { ProxyState, TrafficStats, LogEntry, LogLevel } from '../../shared/types'

// Import core module
import { VpnService, VpnServiceConfig, ServiceState } from '@kunbox/core'

const CONFIG_DIR = join(app.getPath('userData'), 'KunBox')
const SINGBOX_DIR = join(process.resourcesPath, 'libs')
const SINGBOX_PATH = join(SINGBOX_DIR, 'sing-box.exe')

let vpnService: VpnService | null = null
let startTime = 0

function getVpnService(): VpnService {
  if (!vpnService) {
    mkdirSync(CONFIG_DIR, { recursive: true })

    const config: VpnServiceConfig = {
      core: {
        execPath: SINGBOX_PATH,
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

export function initSingBoxHandlers(): void {
  const service = getVpnService()

  ipcMain.handle(IPC_CHANNELS.SINGBOX_START, async () => {
    const configPath = join(CONFIG_DIR, 'config.json')

    if (!existsSync(SINGBOX_PATH)) {
      return { success: false, error: 'sing-box.exe not found' }
    }

    return service.start({ configPath })
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_STOP, async () => {
    return service.stop({ clearProxy: true })
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_RESTART, async () => {
    const configPath = join(CONFIG_DIR, 'config.json')
    return service.restart({ configPath })
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

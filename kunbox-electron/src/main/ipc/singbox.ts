import { ipcMain, BrowserWindow } from 'electron'
import { mkdirSync } from 'fs'
import log from 'electron-log'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type ProxyState, type TrafficStats, type LogEntry, type LogLevel } from '../../shared/types'
import { generateConfig, routingModeToClashMode } from '../utils/configGenerator'
import { resolveApiPort, resolveProxyPorts } from '../utils/proxyPorts'
import { loadRuleSets } from './rulesets'
import { downloadAllRuleSets, checkRuleSetsReady } from '../utils/ruleSetManager'
import { settingsStore } from './settings'
import { VpnService, VpnServiceConfig, ServiceState } from '../core'
import { getKernelManager } from './kernel'
import { getActiveProfileState, loadProfileNodes, setActiveNodeTag, stopTempSingbox } from './profiles'

import { getDataDir } from '../utils/paths'

const DATA_DIR = getDataDir()
const CONFIG_DIR = DATA_DIR

let vpnService: VpnService | null = null
let startTime = 0
let nodeSwitchSeq = 0
let nodeSwitchChain: Promise<void> = Promise.resolve()
let lastSyncedNodeTag: string | null = null

function enqueueNodeOp<T>(task: () => Promise<T>): Promise<T> {
  const run = nodeSwitchChain.then(task, task)
  nodeSwitchChain = run.then(() => undefined, () => undefined)
  return run
}

/**
 * 将当前持久化选中的激活节点同步至运行中的 RayBox PROXY 选择器。
 * 用于覆盖 cache.db 历史选中残留，以及补齐启动/重启过渡期内切换的新节点。
 */
async function syncActiveNodeToCore(service: VpnService, fallbackTag?: string): Promise<void> {
  const { activeNodeTag } = getActiveProfileState()
  const desiredTag = activeNodeTag || fallbackTag || null
  if (!desiredTag) return

  const actualNow = await service.getProxyNow('PROXY')
  if (actualNow === desiredTag) {
    lastSyncedNodeTag = desiredTag
    return
  }

  const switched = await service.selectProxy('PROXY', desiredTag)
  if (switched.success) {
    lastSyncedNodeTag = desiredTag
    log.info(`Synced PROXY selector from "${actualNow ?? 'unknown'}" to "${desiredTag}"`)
    await service.closeAllConnections()
    return
  }

  log.warn(`Failed to sync PROXY selector to "${desiredTag}": ${switched.error || 'unknown error'}`)
  if (fallbackTag && fallbackTag !== desiredTag && actualNow !== fallbackTag) {
    const fallbackRes = await service.selectProxy('PROXY', fallbackTag)
    if (fallbackRes.success) {
      lastSyncedNodeTag = fallbackTag
      log.warn(`Fallback synced PROXY selector to "${fallbackTag}"`)
      await service.closeAllConnections()
    }
  }
}

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
    // 兜底检查：若在 onStarted 同步完成至状态切为 running 的极短窗口内用户再次切换了节点，立即补同步
    const { activeNodeTag } = getActiveProfileState()
    if (activeNodeTag && activeNodeTag !== lastSyncedNodeTag) {
      void enqueueNodeOp(async () => {
        if (service.isRunning()) {
          await syncActiveNodeToCore(service)
        }
      })
    }
  })

  service.on('stopped', () => {
    startTime = 0
    lastSyncedNodeTag = null
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

export async function startConfiguredVpn(execPath: string, restart = false): Promise<{
  success: boolean; error?: string; warning?: string
}> {
  stopTempSingbox()
  const service = getVpnService()
  if (!restart && service.isRunning()) return { success: true }

  const initialState = getActiveProfileState()
  if (!initialState.activeProfileId) return { success: false, error: 'No active profile. Please add a subscription first.' }
  if (!loadProfileNodes(initialState.activeProfileId).length) {
    return { success: false, error: 'No nodes in active profile. Please update your subscription.' }
  }

  let warning: string | undefined
  service.updateConfig({
    core: { execPath, configDir: CONFIG_DIR, workDir: CONFIG_DIR, apiHost: '127.0.0.1', apiPort: CLASH_API_PORT, apiSecret: '' }
  })

  const prepareConfig = async () => {
    // 等待在途节点切换落盘，保证配置生成和自动重启始终读取最新激活订阅、节点与设置
    await nodeSwitchChain
    lastSyncedNodeTag = null

    const { activeProfileId, activeNodeTag } = getActiveProfileState()
    if (!activeProfileId) {
      throw new Error('No active profile. Please add a subscription first.')
    }
    const nodes = loadProfileNodes(activeProfileId)
    if (!nodes.length) {
      throw new Error('No nodes in active profile. Please update your subscription.')
    }

    const previous = settingsStore.get('settings')
    const apiPort = await resolveApiPort(CLASH_API_PORT)
    const ports = await resolveProxyPorts(previous, apiPort)
    service.updateConfig({
      core: { execPath, configDir: CONFIG_DIR, workDir: CONFIG_DIR, apiHost: '127.0.0.1', apiPort, apiSecret: '' }
    })
    const defaultTag = activeNodeTag || nodes[0]?.tag
    const config = generateConfig({
      settings: { ...previous, ...ports },
      outbounds: nodes,
      activeNodeTag: defaultTag,
      clashApiPort: apiPort
    })
    return {
      content: JSON.stringify(config, null, 2),
      onStarted: async () => {
        service.updateConfig({ autoSystemProxy: previous.systemProxy, proxyPort: ports.localPort || DEFAULT_SETTINGS.localPort })
        const changed = [
          ports.localPort !== previous.localPort && `HTTP ${previous.localPort} → ${ports.localPort}`,
          ports.socksPort !== previous.socksPort && `SOCKS ${previous.socksPort} → ${ports.socksPort}`
        ].filter(Boolean)
        if (changed.length) {
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

        // 在进入 running 和启用系统代理之前，强制对齐路由模式与 PROXY 选中节点，消除 cache.db 残留覆盖
        await enqueueNodeOp(async () => {
          const currentSettings = settingsStore.get('settings')
          await service.setMode(routingModeToClashMode(currentSettings.routingMode), false)
          await syncActiveNodeToCore(service, defaultTag)
        })
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
    const seq = ++nodeSwitchSeq
    return enqueueNodeOp(async () => {
      if (seq !== nodeSwitchSeq) {
        return { success: false, superseded: true }
      }

      // VPN 未进入 running（空闲、启动中或重启中）时先落盘选中节点，启动就绪阶段会自动同步到内核
      if (!service.isRunning()) {
        setActiveNodeTag(nodeTag)
        return { success: true }
      }

      try {
        const switched = await service.selectProxy('PROXY', nodeTag)
        if (!switched.success) {
          return { success: false, error: switched.error }
        }

        lastSyncedNodeTag = nodeTag
        if (seq !== nodeSwitchSeq) {
          return { success: false, superseded: true }
        }

        setActiveNodeTag(nodeTag)
        log.info(`Switched to node: ${nodeTag}`)
        // 热切节点后立即关闭旧连接，防止浏览器复用切换前的长连接
        await service.closeAllConnections()
        return { success: true }
      } catch (error) {
        const msg = error instanceof Error ? error.message : String(error)
        return { success: false, error: msg }
      }
    })
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

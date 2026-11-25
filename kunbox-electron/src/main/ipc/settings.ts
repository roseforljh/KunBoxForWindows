import { app, BrowserWindow, ipcMain } from 'electron'
import Store from 'electron-store'
import log from 'electron-log'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type AppSettings } from '../../shared/types'
import { getDataDir } from '../utils/paths'
import { findAvailablePort, proxyListenHost } from '../utils/proxyPorts'
import { isAutoStartLaunch, syncWindowsAutoStart } from '../utils/windowsAdmin'
import { routingModeToClashMode } from '../utils/configGenerator'
import { syncBypassLanRuleSet } from '../utils/ruleSetManager'
import { updateTrayStatus } from '../tray'
import { getServiceInstance } from './singbox'

const store = new Store<{ settings: AppSettings }>({
  name: 'settings',
  cwd: getDataDir(),
  defaults: {
    settings: DEFAULT_SETTINGS
  }
})

export async function applyAutoStartSetting(enabled: boolean): Promise<void> {
  if (process.platform !== 'win32' || !app.isPackaged) return
  try {
    const strategy = await syncWindowsAutoStart(
      enabled,
      process.execPath,
      (openAtLogin, args) => app.setLoginItemSettings({ openAtLogin, path: process.execPath, args })
    )
    log.info(`[AutoStart] synced: enabled=${enabled}, strategy=${strategy}`)
  } catch (error) {
    log.warn('[AutoStart] failed to sync startup setting:', error)
  }
}

export function initSettingsHandlers(): void {
  // 平滑迁移旧版 HTTPS 默认测速地址到无多余 TLS 握手的 HTTP 地址（用户自定义地址保持不变）
  const initial = store.get('settings')
  if (initial.latencyTestUrl === 'https://www.gstatic.com/generate_204') {
    store.set('settings', { ...initial, latencyTestUrl: DEFAULT_SETTINGS.latencyTestUrl })
  }

  store.onDidChange('settings', (settings) => {
    if (!settings) return
    const mode = settings.routingMode === 'global-proxy' ? 'global' : settings.routingMode === 'global-direct' ? 'direct' : 'rule'
    updateTrayStatus(Boolean(getServiceInstance()?.isRunning()), settings.systemProxy, settings.tunEnabled, mode)
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.send(IPC_CHANNELS.SETTINGS_CHANGED, settings)
    }
  })

  ipcMain.handle(IPC_CHANNELS.SETTINGS_GET, () => {
    return store.get('settings')
  })

  ipcMain.handle(IPC_CHANNELS.SETTINGS_SET, async (_, settings: Partial<AppSettings>) => {
    const current = store.get('settings')
    const next = { ...current, ...settings }
    store.set('settings', next)

    const service = getServiceInstance()

    // 系统代理开关直接热生效（无需重启内核）
    if (settings.systemProxy !== undefined && settings.systemProxy !== current.systemProxy && service) {
      service.setAutoSystemProxy(Boolean(next.systemProxy))
      if (service.isRunning()) {
        if (next.systemProxy) {
          await service.enableSystemProxy('127.0.0.1', next.localPort || DEFAULT_SETTINGS.localPort)
        } else {
          await service.disableSystemProxy()
        }
      }
    }

    // 路由模式通过 Clash API PATCH /configs 热切换（无需重启内核）
    if (settings.routingMode !== undefined && settings.routingMode !== current.routingMode && service) {
      await service.setMode(routingModeToClashMode(next.routingMode))
    }

    // 绕过局域网通过覆写本地规则集由 RayBox fswatch 自动热重载（无需重启内核）
    if (settings.bypassLan !== undefined && settings.bypassLan !== current.bypassLan) {
      syncBypassLanRuleSet(Boolean(next.bypassLan))
      if (service?.isRunning()) {
        await service.closeAllConnections()
      }
    }

    if (settings.startWithWindows !== undefined || settings.requireAdmin !== undefined) {
      await applyAutoStartSetting(Boolean(next.startWithWindows))
    }
  })

  ipcMain.handle('settings:is-autostart-launch', () => {
    return isAutoStartLaunch()
  })

  ipcMain.handle('settings:random-available-ports', async () => {
    const host = proxyListenHost(store.get('settings').allowLan)
    const port1 = await findAvailablePort(host, [CLASH_API_PORT])
    return [port1, await findAvailablePort(host, [CLASH_API_PORT, port1])]
  })
}

export { store as settingsStore }

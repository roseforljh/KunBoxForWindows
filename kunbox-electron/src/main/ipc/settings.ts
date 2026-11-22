import { app, BrowserWindow, ipcMain } from 'electron'
import Store from 'electron-store'
import log from 'electron-log'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type AppSettings } from '../../shared/types'
import { getDataDir } from '../utils/paths'
import { findAvailablePort, proxyListenHost } from '../utils/proxyPorts'
import { isAutoStartLaunch, syncWindowsAutoStart } from '../utils/windowsAdmin'
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

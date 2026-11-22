import { BrowserWindow, ipcMain } from 'electron'
import Store from 'electron-store'
import { CLASH_API_PORT, IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type AppSettings } from '../../shared/types'
import { getDataDir } from '../utils/paths'
import { findAvailablePort, proxyListenHost } from '../utils/proxyPorts'

const store = new Store<{ settings: AppSettings }>({
  name: 'settings',
  cwd: getDataDir(),
  defaults: {
    settings: DEFAULT_SETTINGS
  }
})

export function initSettingsHandlers(): void {
  store.onDidChange('settings', (settings) => {
    if (!settings) return
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.send(IPC_CHANNELS.SETTINGS_CHANGED, settings)
    }
  })

  ipcMain.handle(IPC_CHANNELS.SETTINGS_GET, () => {
    return store.get('settings')
  })

  ipcMain.handle(IPC_CHANNELS.SETTINGS_SET, (_, settings: Partial<AppSettings>) => {
    const current = store.get('settings')
    store.set('settings', { ...current, ...settings })
  })

  ipcMain.handle('settings:random-available-ports', async () => {
    const host = proxyListenHost(store.get('settings').allowLan)
    const port1 = await findAvailablePort(host, [CLASH_API_PORT])
    return [port1, await findAvailablePort(host, [CLASH_API_PORT, port1])]
  })
}

export { store as settingsStore }

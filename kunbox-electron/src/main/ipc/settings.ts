import { ipcMain } from 'electron'
import Store from 'electron-store'
import * as net from 'net'
import { IPC_CHANNELS } from '../../shared/constants'
import { DEFAULT_SETTINGS, type AppSettings } from '../../shared/types'
import { getDataDir } from '../utils/paths'

const store = new Store<{ settings: AppSettings }>({
  name: 'settings',
  cwd: getDataDir(),
  defaults: {
    settings: DEFAULT_SETTINGS
  }
})

function getAvailablePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.unref()
    server.on('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address() as net.AddressInfo
      const port = address.port
      server.close(() => resolve(port))
    })
  })
}

export function initSettingsHandlers(): void {
  ipcMain.handle(IPC_CHANNELS.SETTINGS_GET, () => {
    return store.get('settings')
  })

  ipcMain.handle(IPC_CHANNELS.SETTINGS_SET, (_, settings: Partial<AppSettings>) => {
    const current = store.get('settings')
    store.set('settings', { ...current, ...settings })
  })

  ipcMain.handle('settings:random-available-ports', async () => {
    const port1 = await getAvailablePort()
    const port2 = await getAvailablePort()
    return [port1, port2]
  })
}

export { store as settingsStore }

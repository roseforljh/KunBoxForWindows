import { ipcMain, BrowserWindow, app } from 'electron'
import { exec } from 'child_process'
import { promisify } from 'util'

const execAsync = promisify(exec)

export function initWindowHandlers(getMainWindow: () => BrowserWindow | null): void {
  ipcMain.handle('window:minimize', () => {
    const win = getMainWindow()
    win?.minimize()
    return true
  })

  ipcMain.handle('window:maximize', () => {
    const win = getMainWindow()
    if (win?.isMaximized()) {
      win.unmaximize()
    } else {
      win?.maximize()
    }
    return true
  })

  ipcMain.handle('window:close', () => {
    const win = getMainWindow()
    win?.hide()
    return true
  })

  ipcMain.handle('window:show', () => {
    const win = getMainWindow()
    win?.show()
    win?.focus()
    return true
  })

  ipcMain.handle('window:is-admin', async (): Promise<boolean> => {
    try {
      await execAsync('net session', { windowsHide: true })
      return true
    } catch {
      return false
    }
  })

  ipcMain.handle('window:restart-as-admin', async () => {
    const exePath = process.execPath
    const args = process.argv.slice(1).map(arg => `"${arg}"`).join(' ')
    const ps = `Start-Process -FilePath "${exePath}" -ArgumentList '${args}' -Verb RunAs`

    try {
      await execAsync(`powershell -Command "${ps}"`, { windowsHide: true })
      ;(app as any).isQuitting = true
      app.quit()
      return true
    } catch (err: any) {
      throw new Error(`管理员重启失败: ${err?.message || String(err)}`)
    }
  })
}

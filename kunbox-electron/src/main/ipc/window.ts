import { ipcMain, BrowserWindow, app } from 'electron'
import log from 'electron-log'
import { isRunningAsAdmin, launchAsAdmin } from '../utils/windowsAdmin'

export function launchCurrentAppAsAdmin(): Promise<void> {
  return launchAsAdmin(process.env.PORTABLE_EXECUTABLE_FILE || process.execPath,
    app.isPackaged ? undefined : app.getAppPath())
}

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

  ipcMain.handle('window:is-admin', isRunningAsAdmin)

  ipcMain.handle('window:restart-as-admin', async () => {
    try {
      await launchCurrentAppAsAdmin()
      app.quit()
      return true
    } catch (error) {
      log.error('管理员重启失败', error)
      throw new Error('未能以管理员身份重启，请确认 Windows 权限提示后重试', { cause: error })
    }
  })
}

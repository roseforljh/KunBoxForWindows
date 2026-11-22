import { app, BrowserWindow, dialog, shell, ipcMain } from 'electron'
import { join } from 'path'
import { existsSync } from 'fs'
import { electronApp, optimizer, is } from '@electron-toolkit/utils'
import log from 'electron-log'
import { getDataDir } from './utils/paths'
import { requestStartupElevation, waitForAdminRestart } from './utils/windowsAdmin'

// 将 Electron userData 统一指向 D:\KunBoxData（无 D 盘时回退 %APPDATA%\KunBox）
app.setPath('userData', getDataDir())
import { initSingBoxHandlers, cleanupBeforeQuit } from './ipc/singbox'
import { initProfileHandlers, stopTempSingbox } from './ipc/profiles'
import { initSettingsHandlers, settingsStore } from './ipc/settings'
import { initKernelHandlers } from './ipc/kernel'
import { initRuleSetsHandlers } from './ipc/rulesets'
import { initRulesHandlers } from './ipc/rules'
import { initWindowHandlers, launchCurrentAppAsAdmin } from './ipc/window'
import { initUpdaterHandlers } from './ipc/updater'
import { createTray, destroyTray, updateTrayStatus } from './tray'

let mainWindow: BrowserWindow | null = null

function getIconPath(): string {
  // Windows 优先使用多尺寸 icon.ico（含 16/24/32/48/64/256），高分屏下由系统原生匹配清晰尺寸
  const iconFile = process.platform === 'win32' ? 'icon.ico' : 'icon.png'
  const devPath = join(__dirname, '../../build', iconFile)
  if (is.dev && existsSync(devPath)) {
    return devPath
  }
  const prodPath = join(process.resourcesPath || '', iconFile)
  if (existsSync(prodPath)) {
    return prodPath
  }
  const fallbackProdPng = join(process.resourcesPath || '', 'icon.png')
  if (existsSync(fallbackProdPng)) {
    return fallbackProdPng
  }
  return devPath
}

function createWindow(): void {
  const iconPath = getIconPath()

  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    show: false,
    frame: false,
    titleBarStyle: 'hidden',
    backgroundColor: '#050505',
    icon: iconPath,
    webPreferences: {
      preload: join(__dirname, '../preload/index.mjs'),
      sandbox: false,
      contextIsolation: true,
      nodeIntegration: false
    }
  })

  mainWindow.on('ready-to-show', () => {
    mainWindow?.show()
  })

  mainWindow.on('close', (event) => {
    if (!(app as any).isQuitting) {
      event.preventDefault()
      mainWindow?.hide()
    }
  })

  mainWindow.webContents.setWindowOpenHandler((details) => {
    shell.openExternal(details.url)
    return { action: 'deny' }
  })

  if (is.dev && process.env['ELECTRON_RENDERER_URL']) {
    mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL'])
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }

  // Tray status update from renderer
  ipcMain.on('tray:status-update', (_, data: any) => {
    updateTrayStatus(data?.connected ?? false)
  })

  createTray(mainWindow, iconPath)
}

try {
  await waitForAdminRestart()
} catch (error) {
  log.error('等待旧 KunBox 进程退出失败', error)
  dialog.showErrorBox('KunBox', '旧进程未退出，请先从托盘退出 KunBox，再以管理员身份打开。')
  app.exit(1)
}

const gotTheLock = app.requestSingleInstanceLock()

if (!gotTheLock) {
  app.quit()
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore()
      if (!mainWindow.isVisible()) mainWindow.show()
      mainWindow.focus()
    }
  })

  app.whenReady().then(async () => {
    try {
      if (await requestStartupElevation(settingsStore.get('settings').requireAdmin, launchCurrentAppAsAdmin)) {
        app.quit()
        return
      }
    } catch (error) {
      log.warn('启动时未取得管理员权限，继续以普通权限运行', error)
    }

    electronApp.setAppUserModelId('com.kunbox')

    app.on('browser-window-created', (_, window) => {
      optimizer.watchWindowShortcuts(window)
    })

    initSingBoxHandlers()
    initProfileHandlers()
    initSettingsHandlers()
    initKernelHandlers()
    initRuleSetsHandlers()
    initRulesHandlers()
    initWindowHandlers(() => mainWindow)
    initUpdaterHandlers()

    createWindow()

    app.on('activate', function () {
      if (BrowserWindow.getAllWindows().length === 0) createWindow()
    })
  })

  app.on('window-all-closed', () => {
    // Keep running in tray
  })

  app.on('before-quit', async (event) => {
    if (!(app as any).cleanupDone) {
      event.preventDefault()
      ;(app as any).isQuitting = true

      log.info('App quitting, performing cleanup...')
      stopTempSingbox()
      await cleanupBeforeQuit()
      destroyTray()

      ;(app as any).cleanupDone = true
      app.quit()
    }
  })
}

log.initialize()
log.info('KunBox (Electron) started')

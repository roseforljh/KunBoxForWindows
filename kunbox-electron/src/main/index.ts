import { app, BrowserWindow, shell, ipcMain } from 'electron'
import { join } from 'path'
import { existsSync } from 'fs'
import { electronApp, optimizer, is } from '@electron-toolkit/utils'
import log from 'electron-log'
import { initSingBoxHandlers, cleanupBeforeQuit } from './ipc/singbox'
import { initProfileHandlers, stopTempSingbox } from './ipc/profiles'
import { initSettingsHandlers } from './ipc/settings'
import { initKernelHandlers } from './ipc/kernel'
import { initRuleSetsHandlers } from './ipc/rulesets'
import { initRulesHandlers } from './ipc/rules'
import { initWindowHandlers } from './ipc/window'
import { initUpdaterHandlers } from './ipc/updater'
import { createTray, destroyTray, updateTrayStatus } from './tray'

let mainWindow: BrowserWindow | null = null

function getIconPath(): string {
  const devPath = join(__dirname, '../../build/icon.png')
  if (is.dev && existsSync(devPath)) {
    return devPath
  }
  const prodPath = join(process.resourcesPath || '', 'icon.png')
  if (existsSync(prodPath)) {
    return prodPath
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

  app.whenReady().then(() => {
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

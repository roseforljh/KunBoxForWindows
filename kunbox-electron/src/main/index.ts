import { app, BrowserWindow, shell, ipcMain, nativeImage } from 'electron'
import { join } from 'path'
import { electronApp, optimizer, is } from '@electron-toolkit/utils'
import log from 'electron-log'
import { initSingBoxHandlers, cleanupBeforeQuit } from './ipc/singbox'
import { initProfileHandlers, stopTempSingbox } from './ipc/profiles'
import { initSettingsHandlers } from './ipc/settings'
import { initKernelHandlers } from './ipc/kernel'
import { initRuleSetsHandlers } from './ipc/rulesets'
import { createTray, destroyTray, updateTrayStatus } from './tray'

let mainWindow: BrowserWindow | null = null

function getIconPath(): string {
  if (is.dev) {
    return join(__dirname, '../../build/icon.png')
  }
  return join(process.resourcesPath, 'icon.png')
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

  // Minimize to tray instead of closing
  mainWindow.on('close', (event) => {
    if (!app.isQuitting) {
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

  ipcMain.on('window:minimize', () => mainWindow?.minimize())
  ipcMain.on('window:maximize', () => {
    if (mainWindow?.isMaximized()) {
      mainWindow.unmaximize()
    } else {
      mainWindow?.maximize()
    }
  })
  ipcMain.on('window:close', () => mainWindow?.hide())

  // Tray status update from renderer
  ipcMain.on('tray:update-status', (_, connected: boolean) => {
    updateTrayStatus(connected)
  })

  // Create system tray
  createTray(mainWindow, getIconPath())
}

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

  createWindow()

  app.on('activate', function () {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  // Don't quit on window close, keep running in tray
})

app.on('before-quit', async (event) => {
  if (!(app as any).cleanupDone) {
    event.preventDefault()
    ;(app as any).isQuitting = true
    
    log.info('App quitting, performing cleanup...')
    
    // Stop temp sing-box if running
    stopTempSingbox()
    
    // Perform safe cleanup
    await cleanupBeforeQuit()
    
    // Destroy tray
    destroyTray()
    
    ;(app as any).cleanupDone = true
    
    // Now actually quit
    app.quit()
  }
})

log.initialize()
log.info('KunBox started')

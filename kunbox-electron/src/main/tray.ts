import { Tray, Menu, nativeImage, app, BrowserWindow } from 'electron'
import { join } from 'path'
import log from 'electron-log'

let tray: Tray | null = null
let isConnected = false

export function createTray(mainWindow: BrowserWindow): Tray {
  const iconPath = join(process.resourcesPath, 'icon.png')
  
  let icon: Electron.NativeImage
  try {
    icon = nativeImage.createFromPath(iconPath)
    if (icon.isEmpty()) {
      icon = nativeImage.createEmpty()
    }
  } catch {
    icon = nativeImage.createEmpty()
  }

  tray = new Tray(icon)
  tray.setToolTip('KunBox')

  const contextMenu = buildContextMenu(mainWindow)
  tray.setContextMenu(contextMenu)

  tray.on('click', () => {
    if (mainWindow.isVisible()) {
      mainWindow.focus()
    } else {
      mainWindow.show()
    }
  })

  tray.on('double-click', () => {
    log.info('Tray double-click: toggle connection (placeholder)')
  })

  return tray
}

function buildContextMenu(mainWindow: BrowserWindow): Electron.Menu {
  return Menu.buildFromTemplate([
    {
      label: '显示窗口',
      click: () => {
        mainWindow.show()
        mainWindow.focus()
      }
    },
    { type: 'separator' },
    {
      label: isConnected ? '断开连接' : '连接',
      click: () => {
        log.info(`Tray: ${isConnected ? 'Disconnect' : 'Connect'} clicked (placeholder)`)
      }
    },
    { type: 'separator' },
    {
      label: '重启核心',
      click: () => {
        log.info('Tray: Restart core clicked (placeholder)')
      }
    },
    { type: 'separator' },
    {
      label: '退出',
      click: () => {
        app.quit()
      }
    }
  ])
}

export function updateTrayMenu(mainWindow: BrowserWindow, connected: boolean): void {
  isConnected = connected
  if (tray) {
    const contextMenu = buildContextMenu(mainWindow)
    tray.setContextMenu(contextMenu)
    tray.setToolTip(`KunBox - ${connected ? '已连接' : '未连接'}`)
  }
}

export function destroyTray(): void {
  if (tray) {
    tray.destroy()
    tray = null
  }
}

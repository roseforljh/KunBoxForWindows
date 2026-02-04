import { Tray, Menu, nativeImage, app, BrowserWindow, shell } from 'electron'
import log from 'electron-log'
import { getServiceInstance } from './ipc/singbox'

let tray: Tray | null = null
let isConnected = false
let currentWindow: BrowserWindow | null = null
let systemProxyEnabled = true
let tunModeEnabled = false
let currentMode: 'rule' | 'global' | 'direct' = 'rule'

export function createTray(mainWindow: BrowserWindow, iconPath: string): Tray {
  currentWindow = mainWindow
  
  let icon: Electron.NativeImage
  try {
    icon = nativeImage.createFromPath(iconPath)
    if (icon.isEmpty()) {
      log.warn('Tray icon is empty, using default')
      icon = nativeImage.createEmpty()
    }
    // Resize for tray (16x16 on Windows)
    icon = icon.resize({ width: 16, height: 16 })
  } catch (err) {
    log.error('Failed to load tray icon:', err)
    icon = nativeImage.createEmpty()
  }

  tray = new Tray(icon)
  tray.setToolTip('KunBox - 未连接')

  updateContextMenu()

  tray.on('click', () => {
    if (mainWindow.isVisible()) {
      mainWindow.focus()
    } else {
      mainWindow.show()
      mainWindow.focus()
    }
  })

  tray.on('double-click', () => {
    mainWindow.show()
    mainWindow.focus()
  })

  log.info('System tray created')
  return tray
}

function updateContextMenu(): void {
  if (!tray || !currentWindow) return

  const template: Electron.MenuItemConstructorOptions[] = [
    {
      label: `KunBox ${isConnected ? '(已连接)' : '(未连接)'}`,
      enabled: false,
      icon: undefined
    },
    { type: 'separator' },
    {
      label: isConnected ? '断开连接' : '启动连接',
      click: () => {
        currentWindow?.webContents.send('tray:toggle-connection')
      }
    },
    {
      label: '重启内核',
      enabled: isConnected,
      click: () => {
        currentWindow?.webContents.send('tray:restart-core')
      }
    },
    { type: 'separator' },
    {
      label: '代理模式',
      submenu: [
        {
          label: '规则模式',
          type: 'radio',
          checked: currentMode === 'rule',
          click: () => {
            currentMode = 'rule'
            const service = getServiceInstance()
            service?.setMode('rule')
            currentWindow?.webContents.send('tray:set-mode', 'rule')
            updateContextMenu()
          }
        },
        {
          label: '全局代理',
          type: 'radio',
          checked: currentMode === 'global',
          click: () => {
            currentMode = 'global'
            const service = getServiceInstance()
            service?.setMode('global')
            currentWindow?.webContents.send('tray:set-mode', 'global')
            updateContextMenu()
          }
        },
        {
          label: '直连模式',
          type: 'radio',
          checked: currentMode === 'direct',
          click: () => {
            currentMode = 'direct'
            const service = getServiceInstance()
            service?.setMode('direct')
            currentWindow?.webContents.send('tray:set-mode', 'direct')
            updateContextMenu()
          }
        }
      ]
    },
    { type: 'separator' },
    {
      label: '系统代理',
      type: 'checkbox',
      checked: systemProxyEnabled,
      click: async (menuItem) => {
        systemProxyEnabled = menuItem.checked
        const service = getServiceInstance()
        if (service) {
          if (menuItem.checked) {
            await service.enableSystemProxy()
          } else {
            await service.disableSystemProxy()
          }
        }
        currentWindow?.webContents.send('tray:toggle-system-proxy', menuItem.checked)
      }
    },
    {
      label: 'TUN 模式',
      type: 'checkbox',
      checked: tunModeEnabled,
      click: (menuItem) => {
        tunModeEnabled = menuItem.checked
        currentWindow?.webContents.send('tray:toggle-tun', menuItem.checked)
      }
    },
    { type: 'separator' },
    {
      label: '复制代理命令',
      submenu: [
        {
          label: 'PowerShell',
          click: () => {
            const cmd = `$env:HTTP_PROXY="http://127.0.0.1:7890"; $env:HTTPS_PROXY="http://127.0.0.1:7890"`
            require('electron').clipboard.writeText(cmd)
          }
        },
        {
          label: 'CMD',
          click: () => {
            const cmd = `set HTTP_PROXY=http://127.0.0.1:7890 && set HTTPS_PROXY=http://127.0.0.1:7890`
            require('electron').clipboard.writeText(cmd)
          }
        },
        {
          label: 'Bash',
          click: () => {
            const cmd = `export http_proxy=http://127.0.0.1:7890; export https_proxy=http://127.0.0.1:7890`
            require('electron').clipboard.writeText(cmd)
          }
        }
      ]
    },
    { type: 'separator' },
    {
      label: '打开主界面',
      click: () => {
        currentWindow?.show()
        currentWindow?.focus()
      }
    },
    {
      label: '打开日志目录',
      click: () => {
        shell.openPath(app.getPath('logs'))
      }
    },
    { type: 'separator' },
    {
      label: '退出 KunBox',
      click: () => {
        (app as any).isQuitting = true
        app.quit()
      }
    }
  ]

  const contextMenu = Menu.buildFromTemplate(template)
  tray.setContextMenu(contextMenu)
}

export function updateTrayStatus(connected: boolean, sysProxy?: boolean, tun?: boolean, mode?: 'rule' | 'global' | 'direct'): void {
  isConnected = connected
  if (sysProxy !== undefined) systemProxyEnabled = sysProxy
  if (tun !== undefined) tunModeEnabled = tun
  if (mode !== undefined) currentMode = mode
  
  if (tray) {
    tray.setToolTip(`KunBox - ${connected ? '已连接' : '未连接'}`)
    updateContextMenu()
  }
}

export function destroyTray(): void {
  if (tray) {
    tray.destroy()
    tray = null
  }
  log.info('System tray destroyed')
}

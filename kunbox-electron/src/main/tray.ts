import { Tray, Menu, nativeImage, app, BrowserWindow, shell } from 'electron'
import log from 'electron-log'
import { getServiceInstance } from './ipc/singbox'
import { settingsStore } from './ipc/settings'

let tray: Tray | null = null
let isConnected = false
let currentWindow: BrowserWindow | null = null
let systemProxyEnabled = false
let tunModeEnabled = false
let currentMode: 'rule' | 'global' | 'direct' = 'rule'

function routingModeToTrayMode(routingMode?: string): 'rule' | 'global' | 'direct' {
  if (routingMode === 'global-proxy') return 'global'
  if (routingMode === 'global-direct') return 'direct'
  return 'rule'
}

function trayModeToRoutingMode(mode: 'rule' | 'global' | 'direct'): 'rule' | 'global-proxy' | 'global-direct' {
  if (mode === 'global') return 'global-proxy'
  if (mode === 'direct') return 'global-direct'
  return 'rule'
}

export function createTray(mainWindow: BrowserWindow, iconPath: string): Tray {
  currentWindow = mainWindow
  const initialSettings = settingsStore.get('settings')
  systemProxyEnabled = Boolean(initialSettings.systemProxy)
  tunModeEnabled = Boolean(initialSettings.tunEnabled)
  currentMode = routingModeToTrayMode(initialSettings.routingMode)

  // Windows 下直接传入多尺寸 .ico 路径由系统按 DPI 选图，禁止压成 16x16 导致高分屏发糊
  if (process.platform === 'win32' && iconPath.toLowerCase().endsWith('.ico')) {
    tray = new Tray(iconPath)
  } else {
    let icon: Electron.NativeImage
    try {
      icon = nativeImage.createFromPath(iconPath)
      if (icon.isEmpty()) {
        icon = nativeImage.createEmpty()
      }
    } catch (err) {
      log.error('Failed to load tray icon:', err)
      icon = nativeImage.createEmpty()
    }
    tray = new Tray(icon)
  }
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
      enabled: false
    },
    { type: 'separator' },
    {
      label: isConnected ? '断开连接' : '启动连接',
      click: () => {
        if (isConnected) {
          currentWindow?.webContents.send('tray-vpn-stop')
        } else {
          currentWindow?.webContents.send('tray-vpn-start')
        }
      }
    },
    {
      label: '重启内核',
      enabled: isConnected,
      click: () => {
        currentWindow?.webContents.send('tray-vpn-restart')
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
            const cur = settingsStore.get('settings')
            settingsStore.set('settings', { ...cur, routingMode: trayModeToRoutingMode('rule') })
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
            const cur = settingsStore.get('settings')
            settingsStore.set('settings', { ...cur, routingMode: trayModeToRoutingMode('global') })
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
            const cur = settingsStore.get('settings')
            settingsStore.set('settings', { ...cur, routingMode: trayModeToRoutingMode('direct') })
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
        if (menuItem.checked) {
          currentWindow?.webContents.send('tray-proxy-enable')
        } else {
          currentWindow?.webContents.send('tray-proxy-disable')
        }
      }
    },
    {
      label: 'TUN 模式',
      type: 'checkbox',
      checked: tunModeEnabled,
      click: (menuItem) => {
        tunModeEnabled = menuItem.checked
        if (menuItem.checked) {
          currentWindow?.webContents.send('tray-tun-enable')
        } else {
          currentWindow?.webContents.send('tray-tun-disable')
        }
      }
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
        currentWindow?.webContents.send('tray-quit')
        ;(app as any).isQuitting = true
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

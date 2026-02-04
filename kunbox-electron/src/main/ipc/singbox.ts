import { ipcMain, BrowserWindow } from 'electron'
import { spawn, ChildProcess, exec } from 'child_process'
import { join } from 'path'
import { existsSync, mkdirSync, writeFileSync } from 'fs'
import axios from 'axios'
import log from 'electron-log'
import { IPC_CHANNELS, CLASH_API_URL } from '../../shared/constants'
import type { ProxyState, TrafficStats, LogEntry, LogLevel } from '../../shared/types'

const SINGBOX_DIR = join(process.resourcesPath, 'libs')
const SINGBOX_PATH = join(SINGBOX_DIR, 'sing-box.exe')
const CONFIG_DIR = join(process.env.APPDATA || '', 'KunBox')
const CONFIG_PATH = join(CONFIG_DIR, 'config.json')

let singboxProcess: ChildProcess | null = null
let trafficTimer: NodeJS.Timeout | null = null
let state: ProxyState = 'idle'
let startTime: number = 0
let lastUpload = 0
let lastDownload = 0

function sendToRenderer(channel: string, data: unknown) {
  BrowserWindow.getAllWindows().forEach(win => {
    win.webContents.send(channel, data)
  })
}

function setState(newState: ProxyState) {
  if (state === newState) return
  state = newState
  sendToRenderer(IPC_CHANNELS.SINGBOX_STATE, state)
}

function emitLog(level: LogLevel, tag: string, message: string) {
  const entry: LogEntry = {
    timestamp: Date.now(),
    level,
    tag,
    message
  }
  sendToRenderer(IPC_CHANNELS.SINGBOX_LOG, entry)
}

async function startTrafficMonitor() {
  trafficTimer = setInterval(async () => {
    if (state !== 'connected') return

    try {
      const response = await axios.get(`${CLASH_API_URL}/traffic`, { timeout: 2000 })
      const { up, down } = response.data

      const uploadSpeed = lastUpload > 0 ? Math.max(0, up - lastUpload) : 0
      const downloadSpeed = lastDownload > 0 ? Math.max(0, down - lastDownload) : 0

      const stats: TrafficStats = {
        uploadSpeed,
        downloadSpeed,
        uploadTotal: up,
        downloadTotal: down,
        duration: Date.now() - startTime
      }

      sendToRenderer(IPC_CHANNELS.SINGBOX_TRAFFIC, stats)

      lastUpload = up
      lastDownload = down
    } catch {
      // ignore
    }
  }, 1000)
}

function stopTrafficMonitor() {
  if (trafficTimer) {
    clearInterval(trafficTimer)
    trafficTimer = null
  }
}

async function killExistingProcesses() {
  const { exec } = await import('child_process')
  return new Promise<void>((resolve) => {
    exec('taskkill /F /IM sing-box.exe', () => {
      setTimeout(resolve, 500)
    })
  })
}

export function initSingBoxHandlers() {
  mkdirSync(CONFIG_DIR, { recursive: true })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_START, async () => {
    if (singboxProcess && !singboxProcess.killed) {
      return { success: true }
    }

    if (!existsSync(SINGBOX_PATH)) {
      emitLog('error', 'Core', 'sing-box.exe not found')
      setState('error')
      return { success: false, error: 'sing-box.exe not found' }
    }

    try {
      await killExistingProcesses()
      setState('connecting')
      emitLog('info', 'Core', 'Starting sing-box...')

      singboxProcess = spawn(SINGBOX_PATH, ['run', '-c', CONFIG_PATH], {
        cwd: SINGBOX_DIR,
        windowsHide: true
      })

      singboxProcess.stdout?.on('data', (data) => {
        const msg = data.toString().trim()
        log.info('[sing-box]', msg)
        emitLog('info', 'sing-box', msg)
      })

      singboxProcess.stderr?.on('data', (data) => {
        const msg = data.toString().trim()
        log.warn('[sing-box]', msg)
        const level: LogLevel = msg.includes('ERROR') ? 'error' : msg.includes('WARN') ? 'warn' : 'info'
        emitLog(level, 'sing-box', msg)
      })

      singboxProcess.on('exit', (code) => {
        stopTrafficMonitor()
        singboxProcess = null
        if (state === 'connected') {
          emitLog('warn', 'Core', `sing-box exited unexpectedly (code: ${code})`)
        }
        setState('idle')
      })

      await new Promise(resolve => setTimeout(resolve, 1000))

      if (singboxProcess && !singboxProcess.killed) {
        startTime = Date.now()
        lastUpload = 0
        lastDownload = 0
        startTrafficMonitor()
        setState('connected')
        emitLog('info', 'Core', 'sing-box started successfully')
        return { success: true }
      }

      setState('error')
      return { success: false, error: 'Failed to start' }
    } catch (error) {
      log.error('Failed to start sing-box:', error)
      emitLog('error', 'Core', `Failed to start: ${error}`)
      setState('error')
      return { success: false, error: String(error) }
    }
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_STOP, async () => {
    if (!singboxProcess || singboxProcess.killed) {
      setState('idle')
      return { success: true }
    }

    try {
      setState('disconnecting')
      emitLog('info', 'Core', 'Stopping sing-box...')
      stopTrafficMonitor()

      singboxProcess.kill()
      await new Promise(resolve => setTimeout(resolve, 500))

      singboxProcess = null
      setState('idle')
      emitLog('info', 'Core', 'sing-box stopped')
      return { success: true }
    } catch (error) {
      log.error('Failed to stop sing-box:', error)
      return { success: false, error: String(error) }
    }
  })

  ipcMain.handle(IPC_CHANNELS.SINGBOX_RESTART, async () => {
    await ipcMain.emit(IPC_CHANNELS.SINGBOX_STOP)
    await new Promise(resolve => setTimeout(resolve, 500))
    return ipcMain.emit(IPC_CHANNELS.SINGBOX_START)
  })
}

// Clear Windows system proxy settings
function clearSystemProxy(): Promise<void> {
  return new Promise((resolve) => {
    log.info('Clearing system proxy settings...')
    
    // Disable system proxy via registry
    const commands = [
      'reg add "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings" /v ProxyEnable /t REG_DWORD /d 0 /f',
      'reg delete "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings" /v ProxyServer /f'
    ]
    
    let completed = 0
    commands.forEach(cmd => {
      exec(cmd, { windowsHide: true }, (error) => {
        if (error) {
          log.warn(`Failed to clear proxy setting: ${error.message}`)
        }
        completed++
        if (completed >= commands.length) {
          log.info('System proxy settings cleared')
          resolve()
        }
      })
    })
    
    // Timeout fallback
    setTimeout(resolve, 2000)
  })
}

// Safe cleanup before app quit
export async function cleanupBeforeQuit(): Promise<void> {
  log.info('Performing safe cleanup before quit...')
  
  // Stop traffic monitor
  stopTrafficMonitor()
  
  // Stop sing-box process
  if (singboxProcess && !singboxProcess.killed) {
    log.info('Stopping sing-box process...')
    singboxProcess.kill()
    await new Promise(resolve => setTimeout(resolve, 500))
  }
  
  // Kill any remaining sing-box processes
  await killExistingProcesses()
  
  // Clear system proxy
  await clearSystemProxy()
  
  log.info('Cleanup completed')
}

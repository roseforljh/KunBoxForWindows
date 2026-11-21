import { spawn, ChildProcess, exec } from 'child_process'
import { join } from 'path'
import { existsSync, mkdirSync, writeFileSync, unlinkSync } from 'fs'
import { EventEmitter } from 'events'
import type {
  ServiceState,
  ServiceStatus,
  StartOptions,
  StopOptions,
  CoreConfig,
  LogEntry,
  ProxyMode
} from './types'

export class SingBoxManager extends EventEmitter {
  private process: ChildProcess | null = null
  private state: ServiceState = 'idle'
  private pid: number | null = null
  private startTime: number | null = null
  private lastError: string | null = null
  private configPath: string | null = null
  private mode: ProxyMode = 'rule'
  private config: CoreConfig
  private restartAttempts = 0
  private maxRestartAttempts = 3
  private restartDelay = 1000
  private logger: (msg: string) => void

  constructor(config: CoreConfig, logger?: (msg: string) => void) {
    super()
    this.config = config
    this.logger = logger || console.log
  }

  getStatus(): ServiceStatus {
    return {
      state: this.state,
      pid: this.pid,
      startTime: this.startTime,
      lastError: this.lastError,
      configPath: this.configPath,
      mode: this.mode
    }
  }

  getState(): ServiceState {
    return this.state
  }

  isRunning(): boolean {
    return this.state === 'running' && this.process !== null && !this.process.killed
  }

  setConfig(config: Partial<CoreConfig>): void {
    this.config = { ...this.config, ...config }
  }

  setMode(mode: ProxyMode): void {
    this.mode = mode
    this.emit('modeChange', mode)
  }

  private setState(newState: ServiceState, error?: string): void {
    if (this.state === newState) return
    const oldState = this.state
    this.state = newState
    if (error) this.lastError = error
    this.log('info', 'Core', `State: ${oldState} -> ${newState}${error ? ` (${error})` : ''}`)
    this.emit('stateChange', newState, oldState)
  }

  private log(level: LogEntry['level'], tag: string, message: string): void {
    const entry: LogEntry = { timestamp: Date.now(), level, tag, message }
    this.emit('log', entry)
    this.logger(`[${tag}] ${message}`)
  }

  async start(options: StartOptions = {}): Promise<{ success: boolean; error?: string }> {
    if (this.isRunning()) {
      return { success: true }
    }

    if (this.state === 'starting') {
      return { success: false, error: 'Already starting' }
    }

    try {
      this.setState('starting')
      this.log('info', 'Core', 'Preparing to start sing-box...')

      if (!existsSync(this.config.execPath)) {
        const error = `sing-box not found: ${this.config.execPath}`
        this.setState('error', error)
        return { success: false, error }
      }

      let configPath = options.configPath
      if (options.configContent) {
        mkdirSync(this.config.configDir, { recursive: true })
        configPath = join(this.config.configDir, 'config.json')
        writeFileSync(configPath, options.configContent)
      }

      if (!configPath || !existsSync(configPath)) {
        const error = `Config not found: ${configPath}`
        this.setState('error', error)
        return { success: false, error }
      }

      this.configPath = configPath

      if (options.cleanCache) {
        const cacheDb = join(this.config.workDir, 'cache.db')
        if (existsSync(cacheDb)) {
          try {
            unlinkSync(cacheDb)
            this.log('info', 'Core', 'Cache cleaned')
          } catch {
            // ignore
          }
        }
      }

      this.log('info', 'Core', `Starting sing-box process with ${this.config.execPath}...`)

      this.process = spawn(this.config.execPath, ['run', '-c', configPath], {
        cwd: this.config.workDir,
        windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe']
      })

      this.pid = this.process.pid || null

      this.process.stdout?.on('data', (data) => {
        const msg = data.toString().trim()
        if (msg) this.log('info', 'sing-box', msg)
      })

      this.process.stderr?.on('data', (data) => {
        const msg = data.toString().trim()
        if (msg) {
          const level = msg.includes('ERROR') || msg.includes('FATAL') ? 'error' :
                       msg.includes('WARN') ? 'warn' : 'info'
          this.log(level, 'sing-box', msg)
          if (this.state === 'starting' && (msg.includes('FATAL') || msg.includes('panic'))) {
            this.setState('error', msg)
          }
        }
      })

      this.process.on('exit', (code, signal) => {
        this.handleProcessExit(code, signal)
      })

      this.process.on('error', (err) => {
        this.log('error', 'Core', `Process error: ${err.message}`)
        this.setState('error', err.message)
      })

      await this.waitForStartup()

      if (this.process && !this.process.killed) {
        this.startTime = Date.now()
        this.restartAttempts = 0
        this.setState('running')
        this.log('info', 'Core', `sing-box started (PID: ${this.pid})`)
        this.emit('started')
        return { success: true }
      }

      return { success: false, error: this.lastError || 'Failed to start' }

    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      this.log('error', 'Core', `Start failed: ${error}`)
      this.setState('error', error)
      return { success: false, error }
    }
  }

  async stop(options: StopOptions = {}): Promise<{ success: boolean; error?: string }> {
    if (!this.process || this.process.killed) {
      this.setState('idle')
      return { success: true }
    }

    if (this.state === 'stopping') {
      return { success: false, error: 'Already stopping' }
    }

    try {
      this.setState('stopping')
      this.log('info', 'Core', 'Stopping sing-box...')

      const timeout = options.timeout || 5000
      await this.killProcess(options.force, timeout)

      this.process = null
      this.pid = null
      this.startTime = null
      this.setState('idle')
      this.log('info', 'Core', 'sing-box stopped')
      this.emit('stopped')
      return { success: true }

    } catch {
      this.destroyOwnProcess()
      this.process = null
      this.pid = null
      this.setState('idle')
      return { success: true }
    }
  }

  async restart(options: StartOptions = {}): Promise<{ success: boolean; error?: string }> {
    this.log('info', 'Core', 'Restarting sing-box...')
    await this.stop({ clearProxy: false })
    await new Promise(resolve => setTimeout(resolve, 500))
    return this.start(options)
  }

  private async waitForStartup(timeout = 3000): Promise<void> {
    const start = Date.now()
    const http = await import('http')
    
    while (Date.now() - start < timeout) {
      if (!this.process || this.process.killed) break
      
      const ready = await new Promise<boolean>((resolve) => {
        const req = http.request({
          host: this.config.apiHost,
          port: this.config.apiPort,
          path: '/',
          method: 'GET',
          timeout: 500
        }, (res) => {
          resolve(res.statusCode === 200)
        })
        req.on('error', () => resolve(false))
        req.on('timeout', () => { req.destroy(); resolve(false) })
        req.end()
      })
      
      if (ready) return
      await new Promise(resolve => setTimeout(resolve, 200))
    }
  }

  private async killProcess(force = false, timeout = 5000): Promise<void> {
    if (!this.process) return

    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.destroyOwnProcess()
        resolve()
      }, timeout)

      this.process!.once('exit', () => {
        clearTimeout(timer)
        resolve()
      })

      if (force) {
        this.process!.kill('SIGKILL')
      } else {
        this.process!.kill('SIGTERM')
      }
    })
  }

  private destroyOwnProcess(): void {
    const pid = this.pid || this.process?.pid
    if (process.platform === 'win32' && pid) {
      exec(`taskkill /F /T /PID ${pid}`, { windowsHide: true }, () => {})
    }
    if (this.process && !this.process.killed) {
      try {
        this.process.kill('SIGKILL')
      } catch {
        // ignore
      }
    }
  }

  private handleProcessExit(code: number | null, signal: string | null): void {
    const wasRunning = this.state === 'running'
    this.process = null
    this.pid = null

    if (this.state === 'stopping') {
      this.setState('idle')
      return
    }

    if (wasRunning) {
      this.log('warn', 'Core', `sing-box exited unexpectedly (code: ${code}, signal: ${signal})`)
      this.emit('unexpectedExit', code, signal)

      if (this.restartAttempts < this.maxRestartAttempts && this.configPath) {
        this.restartAttempts++
        this.log('info', 'Core', `Auto-restart ${this.restartAttempts}/${this.maxRestartAttempts}`)
        setTimeout(() => {
          this.start({ configPath: this.configPath! })
        }, this.restartDelay * this.restartAttempts)
      } else {
        this.setState('error', 'Process crashed too many times')
      }
    } else {
      this.setState('idle')
    }
  }

  getApiUrl(): string {
    return `http://${this.config.apiHost}:${this.config.apiPort}`
  }

  destroy(): void {
    this.removeAllListeners()
    if (this.process && !this.process.killed) {
      this.process.kill('SIGKILL')
    }
    this.process = null
  }
}

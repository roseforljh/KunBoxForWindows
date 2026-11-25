import { spawn, ChildProcess, exec } from 'child_process'
import http from 'http'
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

// 剥离终端 ANSI 颜色转义序列，避免界面 Toast 与日志出现方块乱码
function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '').trim()
}

// 将常见 sing-box 内核底层报错转换为易懂中文提示
function formatCoreError(raw: string): string {
  const clean = stripAnsi(raw)
    .replace(/^(FATAL|ERROR|WARN|INFO)\[\d+\]\s*/i, '')
    .replace(/^start service:\s*/i, '')
    .replace(/^create service:\s*/i, '')
    .trim()

  const depMatch = clean.match(/dependency\[([^\]]+)\]\s+not found for outbound\[([^\]]+)\]/i)
  if (depMatch) {
    const depName = depMatch[1].includes('::') ? depMatch[1].split('::').slice(1).join('::') : depMatch[1]
    return `节点「${depMatch[2]}」的前置代理「${depName}」不存在或已失效，请在编辑节点中重新选择`
  }

  if (/address already in use|only one usage of each socket address/i.test(clean)) {
    return '代理或控制端口被占用，请关闭冲突程序或在设置中更换端口'
  }

  if (/access is denied|permission denied|operation not permitted|wintun|create tun/i.test(clean)) {
    return '创建 TUN 网卡权限不足，请以管理员身份运行 KunBox 或关闭 TUN 模式'
  }

  if (/legacy DNS fakeip|Legacy Address Filter/i.test(clean)) {
    return 'DNS 配置与当前内核版本不兼容，请检查 DNS 或规则集设置'
  }

  if (/no such file or directory|cannot find the file|system cannot find the path/i.test(clean)) {
    return '缺少规则集或配置文件，请先更新规则集或重新导入订阅'
  }

  if (/decode config|invalid/i.test(clean)) {
    return `配置文件格式错误：${clean.replace(/^decode config[^:]*:\s*/i, '')}`
  }

  return clean || '内核启动失败，请查看日志了解详情'
}

export class SingBoxManager extends EventEmitter {
  private process: ChildProcess | null = null
  private state: ServiceState = 'idle'
  private pid: number | null = null
  private startTime: number | null = null
  private lastError: string | null = null
  private configPath: string | null = null
  private mode: ProxyMode = 'rule'
  private config: CoreConfig
  private startSequence = 0
  private restartOptions: StartOptions | null = null
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

  private isApiAvailable(): boolean {
    return (
      (this.state === 'running' || this.state === 'starting') &&
      this.process !== null &&
      !this.process.killed
    )
  }

  setMode(mode: ProxyMode): Promise<boolean> {
    this.mode = mode
    this.emit('modeChange', mode)
    // 运行中或启动就绪钩子阶段实时通知内核 Clash API 切换模式
    if (!this.isApiAvailable()) {
      return Promise.resolve(true)
    }
    return new Promise((resolve) => {
      const payload = JSON.stringify({ mode })
      const req = http.request({
        host: this.config.apiHost,
        port: this.config.apiPort,
        path: '/configs',
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(payload)
        },
        timeout: 3000
      }, (res) => {
        res.resume()
        resolve(res.statusCode === 204 || res.statusCode === 200)
      })
      req.on('error', () => resolve(false))
      req.on('timeout', () => {
        req.destroy()
        resolve(false)
      })
      req.write(payload)
      req.end()
    })
  }

  flushDnsCache(): Promise<boolean> {
    if (!this.isRunning()) {
      return Promise.resolve(true)
    }
    return new Promise((resolve) => {
      const req = http.request({
        host: this.config.apiHost,
        port: this.config.apiPort,
        path: '/cache/dns/flush',
        method: 'POST',
        timeout: 3000
      }, (res) => {
        res.resume()
        resolve(res.statusCode === 204 || res.statusCode === 200)
      })
      req.on('error', () => resolve(false))
      req.on('timeout', () => {
        req.destroy()
        resolve(false)
      })
      req.end()
    })
  }

  selectProxy(groupTag: string, nodeTag: string): Promise<{ success: boolean; statusCode?: number; error?: string }> {
    if (!this.isApiAvailable()) {
      return Promise.resolve({ success: false, error: '内核未运行' })
    }
    return new Promise((resolve) => {
      const data = JSON.stringify({ name: nodeTag })
      const req = http.request({
        host: this.config.apiHost,
        port: this.config.apiPort,
        path: `/proxies/${encodeURIComponent(groupTag)}`,
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(data)
        },
        timeout: 5000
      }, (res) => {
        res.resume()
        if (res.statusCode === 204 || res.statusCode === 200) {
          resolve({ success: true, statusCode: res.statusCode })
        } else {
          resolve({ success: false, statusCode: res.statusCode, error: `API returned ${res.statusCode}` })
        }
      })
      req.on('error', (e) => resolve({ success: false, error: e.message }))
      req.on('timeout', () => {
        req.destroy()
        resolve({ success: false, error: 'Request timeout' })
      })
      req.write(data)
      req.end()
    })
  }

  getProxyNow(groupTag: string): Promise<string | null> {
    if (!this.isApiAvailable()) {
      return Promise.resolve(null)
    }
    return new Promise((resolve) => {
      const req = http.request({
        host: this.config.apiHost,
        port: this.config.apiPort,
        path: `/proxies/${encodeURIComponent(groupTag)}`,
        method: 'GET',
        timeout: 3000
      }, (res) => {
        let body = ''
        res.on('data', (chunk) => { body += chunk })
        res.on('end', () => {
          if (res.statusCode !== 200) {
            resolve(null)
            return
          }
          try {
            const parsed = JSON.parse(body)
            resolve(typeof parsed?.now === 'string' ? parsed.now : null)
          } catch {
            resolve(null)
          }
        })
      })
      req.on('error', () => resolve(null))
      req.on('timeout', () => {
        req.destroy()
        resolve(null)
      })
      req.end()
    })
  }

  getApiPort(): number {
    return this.config.apiPort
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

    if (this.state === 'starting' || this.state === 'stopping' || this.process) {
      return { success: false, error: '内核正在启动或退出，请稍后再试' }
    }

    const sequence = ++this.startSequence
    try {
      this.lastError = null
      this.setState('starting')
      this.log('info', 'Core', 'Preparing to start sing-box...')

      if (!existsSync(this.config.execPath)) {
        const error = `sing-box not found: ${this.config.execPath}`
        this.setState('error', error)
        return { success: false, error }
      }

      const prepared = await options.prepareConfig?.()
      if (sequence !== this.startSequence) return { success: false, error: '内核启动已取消' }

      let configPath = options.configPath
      const configContent = prepared?.content ?? options.configContent
      if (configContent) {
        mkdirSync(this.config.configDir, { recursive: true })
        configPath = join(this.config.configDir, 'config.json')
        writeFileSync(configPath, configContent)
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
        const msg = stripAnsi(data.toString())
        if (msg) this.log('info', 'sing-box', msg)
      })

      this.process.stderr?.on('data', (data) => {
        const msg = stripAnsi(data.toString())
        if (msg) {
          const level = msg.includes('ERROR') || msg.includes('FATAL') ? 'error' :
                       msg.includes('WARN') ? 'warn' : 'info'
          this.log(level, 'sing-box', msg)
          if (this.state === 'starting' && (msg.includes('FATAL') || msg.includes('panic'))) {
            this.setState('error', formatCoreError(msg))
          }
        }
      })

      this.process.on('exit', (code, signal) => {
        this.handleProcessExit(code, signal)
      })

      this.process.on('error', (err) => {
        if (!this.pid) this.process = null
        this.log('error', 'Core', `Process error: ${err.message}`)
        this.setState('error', err.message)
      })

      await this.waitForStartup()

      if (sequence !== this.startSequence) return { success: false, error: '内核启动已取消' }
      if (this.getState() === 'starting' && this.process && !this.process.killed) {
        await prepared?.onStarted()
        if (sequence !== this.startSequence || this.getState() !== 'starting' || !this.process || this.process.killed) {
          return { success: false, error: '内核启动已取消' }
        }
        this.startTime = Date.now()
        this.restartAttempts = 0
        this.restartOptions = { configPath, prepareConfig: options.prepareConfig }
        this.setState('running')
        this.log('info', 'Core', `sing-box started (PID: ${this.pid})`)
        this.emit('started')
        return { success: true }
      }

      return { success: false, error: this.lastError || 'Failed to start' }

    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      this.log('error', 'Core', `Start failed: ${error}`)
      if (sequence === this.startSequence) this.setState('error', error)
      return { success: false, error }
    }
  }

  async stop(options: StopOptions = {}): Promise<{ success: boolean; error?: string }> {
    ++this.startSequence
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

      const timeout = options.timeout || 1500
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
    const stopping = this.stop({ clearProxy: false })
    const sequence = this.startSequence
    const result = await stopping
    if (!result.success) return result
    await new Promise(resolve => setTimeout(resolve, 30))
    if (sequence !== this.startSequence) return { success: false, error: '内核重启已取消' }
    return this.start(options)
  }

  private async waitForStartup(timeout = 3000): Promise<void> {
    const start = Date.now()
    const http = await import('http')
    // 避开刚 spawn 瞬间的必然 ECONNREFUSED 空转，随后以 40ms 高频探测就绪
    await new Promise(resolve => setTimeout(resolve, 40))

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
          res.resume()
          resolve(res.statusCode === 200)
        })
        req.on('error', () => resolve(false))
        req.on('timeout', () => { req.destroy(); resolve(false) })
        req.end()
      })

      if (ready) return
      await new Promise(resolve => setTimeout(resolve, 40))
    }
  }

  private async killProcess(force = false, timeout = 1500): Promise<void> {
    if (!this.process) return

    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        void this.destroyOwnProcess().finally(resolve)
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

  private destroyOwnProcess(): Promise<void> {
    const pid = this.pid || this.process?.pid
    if (this.process && !this.process.killed) {
      try {
        this.process.kill('SIGKILL')
      } catch {
        // ignore
      }
    }
    if (process.platform === 'win32' && pid) {
      return new Promise((resolve) => {
        exec(`taskkill /F /T /PID ${pid}`, { windowsHide: true, timeout: 3000 }, () => resolve())
      })
    }
    return Promise.resolve()
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

      if (this.restartAttempts < this.maxRestartAttempts && this.restartOptions) {
        const options = this.restartOptions
        const sequence = this.startSequence
        this.restartAttempts++
        this.log('info', 'Core', `Auto-restart ${this.restartAttempts}/${this.maxRestartAttempts}`)
        setTimeout(() => {
          if (sequence === this.startSequence) void this.start(options)
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
    ++this.startSequence
    this.removeAllListeners()
    if (this.process && !this.process.killed) {
      this.process.kill('SIGKILL')
    }
    this.process = null
  }
}

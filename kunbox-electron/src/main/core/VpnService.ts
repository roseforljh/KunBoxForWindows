import { EventEmitter } from 'events'
import { SingBoxManager } from './SingBoxManager'
import { SystemProxy } from './SystemProxy'
import { TrafficMonitor } from './TrafficMonitor'
import type {
  ServiceState,
  ServiceStatus,
  StartOptions,
  StopOptions,
  CoreConfig,
  TrafficSnapshot,
  LogEntry,
  ProxyMode
} from './types'

export interface VpnServiceConfig {
  core: CoreConfig
  autoSystemProxy: boolean
  proxyPort: number
}

export class VpnService extends EventEmitter {
  private singbox: SingBoxManager
  private systemProxy: SystemProxy
  private trafficMonitor: TrafficMonitor
  private config: VpnServiceConfig
  private autoSystemProxy: boolean
  private logger: (msg: string) => void

  constructor(config: VpnServiceConfig, logger?: (msg: string) => void) {
    super()
    this.config = config
    this.autoSystemProxy = config.autoSystemProxy
    this.logger = logger || console.log

    this.singbox = new SingBoxManager(config.core, this.logger)
    this.systemProxy = new SystemProxy(this.logger)
    this.trafficMonitor = new TrafficMonitor(this.singbox.getApiUrl(), this.logger)

    this.setupEventHandlers()
  }

  private setupEventHandlers(): void {
    this.singbox.on('stateChange', (newState: ServiceState, oldState: ServiceState) => {
      this.emit('stateChange', newState, oldState)

      if (newState === 'running') {
        if (this.autoSystemProxy) {
          this.systemProxy.enable('127.0.0.1', this.config.proxyPort)
        }
        this.trafficMonitor.start()
      } else if (newState === 'idle' || newState === 'error') {
        if (this.autoSystemProxy) {
          this.systemProxy.disable()
        }
        this.trafficMonitor.stop()
      }
    })

    this.singbox.on('log', (entry: LogEntry) => {
      this.emit('log', entry)
    })

    this.singbox.on('started', () => {
      this.emit('started')
    })

    this.singbox.on('stopped', () => {
      this.emit('stopped')
    })

    this.singbox.on('error', (error: string) => {
      this.emit('error', error)
    })

    this.singbox.on('unexpectedExit', (code: number | null, signal: string | null) => {
      this.emit('unexpectedExit', code, signal)
    })

    this.trafficMonitor.on('traffic', (snapshot: TrafficSnapshot) => {
      this.emit('traffic', snapshot)
    })
  }

  async start(options: StartOptions = {}): Promise<{ success: boolean; error?: string }> {
    return this.singbox.start(options)
  }

  async stop(options: StopOptions = {}): Promise<{ success: boolean; error?: string }> {
    this.trafficMonitor.stop()
    const result = await this.singbox.stop(options)
    if (options.clearProxy !== false && this.autoSystemProxy) {
      await this.systemProxy.disable()
    }
    return result
  }

  async restart(options: StartOptions = {}): Promise<{ success: boolean; error?: string }> {
    return this.singbox.restart(options)
  }

  getStatus(): ServiceStatus {
    return this.singbox.getStatus()
  }

  getState(): ServiceState {
    return this.singbox.getState()
  }

  isRunning(): boolean {
    return this.singbox.isRunning()
  }

  setMode(mode: ProxyMode): void {
    this.singbox.setMode(mode)
  }

  async enableSystemProxy(host?: string, port?: number): Promise<boolean> {
    return this.systemProxy.enable(host || '127.0.0.1', port || this.config.proxyPort)
  }

  async disableSystemProxy(): Promise<boolean> {
    return this.systemProxy.disable()
  }

  isSystemProxyEnabled(): boolean {
    return this.systemProxy.isEnabled()
  }

  setAutoSystemProxy(enabled: boolean): void {
    this.autoSystemProxy = enabled
    if (!enabled && this.systemProxy.isEnabled()) {
      this.systemProxy.disable()
    }
  }

  getTrafficSnapshot(): TrafficSnapshot {
    return this.trafficMonitor.getSnapshot()
  }

  async getConnections() {
    return this.trafficMonitor.getConnections()
  }

  async closeConnection(id: string): Promise<boolean> {
    return this.trafficMonitor.closeConnection(id)
  }

  async closeAllConnections(): Promise<boolean> {
    return this.trafficMonitor.closeAllConnections()
  }

  updateConfig(config: Partial<VpnServiceConfig>): void {
    if (config.core) {
      this.singbox.setConfig(config.core)
      this.trafficMonitor.setApiUrl(this.singbox.getApiUrl())
    }
    if (config.autoSystemProxy !== undefined) {
      this.autoSystemProxy = config.autoSystemProxy
    }
    if (config.proxyPort !== undefined) {
      this.config.proxyPort = config.proxyPort
    }
  }

  getApiUrl(): string {
    return this.singbox.getApiUrl()
  }

  async cleanup(): Promise<void> {
    this.log('Performing cleanup...')
    this.trafficMonitor.stop()
    await this.singbox.stop({ force: true, timeout: 3000 })
    await this.systemProxy.disable()
    this.log('Cleanup completed')
  }

  destroy(): void {
    this.trafficMonitor.stop()
    this.singbox.destroy()
    this.removeAllListeners()
  }

  private log(msg: string): void {
    this.logger(`[VpnService] ${msg}`)
  }
}

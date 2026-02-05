import { EventEmitter } from 'events'
import type { TrafficSnapshot, ConnectionInfo } from './types'

export class TrafficMonitor extends EventEmitter {
  private apiUrl: string
  private interval: NodeJS.Timeout | null = null
  private pollInterval = 1000
  private lastUpload = 0
  private lastDownload = 0
  private uploadTotal = 0
  private downloadTotal = 0
  private running = false
  private logger: (msg: string) => void

  constructor(apiUrl: string, logger?: (msg: string) => void) {
    super()
    this.apiUrl = apiUrl
    this.logger = logger || console.log
  }

  setApiUrl(url: string): void {
    this.apiUrl = url
  }

  start(): void {
    if (this.running) return
    this.running = true
    this.lastUpload = 0
    this.lastDownload = 0
    this.uploadTotal = 0
    this.downloadTotal = 0

    // Poll immediately on start
    this.poll()
    
    this.interval = setInterval(() => this.poll(), this.pollInterval)
    this.log('Traffic monitor started')
  }

  stop(): void {
    if (this.interval) {
      clearInterval(this.interval)
      this.interval = null
    }
    this.running = false
    this.log('Traffic monitor stopped')
  }

  pause(): void {
    if (this.interval) {
      clearInterval(this.interval)
      this.interval = null
    }
  }

  resume(): void {
    if (this.running && !this.interval) {
      this.interval = setInterval(() => this.poll(), this.pollInterval)
    }
  }

  isRunning(): boolean {
    return this.running
  }

  getSnapshot(): TrafficSnapshot {
    return {
      uploadSpeed: 0,
      downloadSpeed: 0,
      uploadTotal: this.uploadTotal,
      downloadTotal: this.downloadTotal,
      connections: 0
    }
  }

  private async poll(): Promise<void> {
    try {
      const http = await import('http')
      
      // Get connection count and calculate traffic from connections
      let connections = 0
      let totalUp = 0
      let totalDown = 0
      let gotTraffic = false
      
      try {
        const connData = await this.httpGet(http, '/connections')
        if (connData) {
          const parsed = JSON.parse(connData)
          connections = parsed.connections?.length || 0
          
          // Get total traffic directly from the response
          if (typeof parsed.uploadTotal === 'number') {
            totalUp = parsed.uploadTotal
          }
          if (typeof parsed.downloadTotal === 'number') {
            totalDown = parsed.downloadTotal
          }
          
          gotTraffic = true
        }
      } catch {
        // Ignore parse errors
      }
      
      if (gotTraffic) {
        const uploadSpeed = this.lastUpload > 0 ? Math.max(0, totalUp - this.lastUpload) : 0
        const downloadSpeed = this.lastDownload > 0 ? Math.max(0, totalDown - this.lastDownload) : 0
        
        this.lastUpload = totalUp
        this.lastDownload = totalDown
        this.uploadTotal = totalUp
        this.downloadTotal = totalDown

        const snapshot: TrafficSnapshot = {
          uploadSpeed,
          downloadSpeed,
          uploadTotal: totalUp,
          downloadTotal: totalDown,
          connections
        }

        this.emit('traffic', snapshot)
      }
    } catch {
      // Ignore poll exceptions
    }
  }

  private httpGet(http: typeof import('http'), path: string): Promise<string | null> {
    return new Promise((resolve) => {
      const url = new URL(this.apiUrl)
      const req = http.request({
        host: url.hostname,
        port: parseInt(url.port) || 9090,
        path,
        method: 'GET',
        timeout: 2000
      }, (res) => {
        let data = ''
        res.on('data', chunk => data += chunk)
        res.on('end', () => {
          resolve(data)
        })
      })
      req.on('error', () => {
        resolve(null)
      })
      req.on('timeout', () => {
        this.log('Request timeout')
        req.destroy()
        resolve(null)
      })
      req.end()
    })
  }

  // Get active connections
  async getConnections(): Promise<ConnectionInfo[]> {
    try {
      const http = await import('http')
      const data = await this.httpGet(http, '/connections')
      if (!data) return []
      
      const parsed = JSON.parse(data)
      return (parsed.connections || []).map((c: any) => ({
        id: c.id,
        network: c.metadata?.network || '',
        type: c.metadata?.type || '',
        source: `${c.metadata?.sourceIP || ''}:${c.metadata?.sourcePort || ''}`,
        destination: `${c.metadata?.destinationIP || ''}:${c.metadata?.destinationPort || ''}`,
        host: c.metadata?.host || '',
        rule: c.rule || '',
        rulePayload: c.rulePayload || '',
        chains: c.chains || [],
        upload: c.upload || 0,
        download: c.download || 0,
        start: new Date(c.start).getTime()
      }))
    } catch {
      return []
    }
  }

  // Close a specific connection
  async closeConnection(id: string): Promise<boolean> {
    try {
      const http = await import('http')
      return new Promise((resolve) => {
        const url = new URL(this.apiUrl)
        const req = http.request({
          host: url.hostname,
          port: parseInt(url.port) || 9090,
          path: `/connections/${id}`,
          method: 'DELETE',
          timeout: 2000
        }, (res) => {
          resolve(res.statusCode === 204)
        })
        req.on('error', () => resolve(false))
        req.end()
      })
    } catch {
      return false
    }
  }

  // Close all connections
  async closeAllConnections(): Promise<boolean> {
    try {
      const http = await import('http')
      return new Promise((resolve) => {
        const url = new URL(this.apiUrl)
        const req = http.request({
          host: url.hostname,
          port: parseInt(url.port) || 9090,
          path: '/connections',
          method: 'DELETE',
          timeout: 2000
        }, (res) => {
          resolve(res.statusCode === 204)
        })
        req.on('error', () => resolve(false))
        req.end()
      })
    } catch {
      return false
    }
  }

  private log(msg: string): void {
    this.logger(`[TrafficMonitor] ${msg}`)
  }
}

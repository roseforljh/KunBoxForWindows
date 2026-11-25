import { exec } from 'child_process'
import { promisify } from 'util'
import type { SystemProxyConfig } from './types'
import { DEFAULT_BYPASS_LIST } from './types'

const execAsync = promisify(exec)

const REGISTRY_PATH = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'

export class SystemProxy {
  private enabled = false
  private server = '127.0.0.1'
  private port = 7890
  private bypass: string[] = DEFAULT_BYPASS_LIST
  private logger: (msg: string) => void

  constructor(logger?: (msg: string) => void) {
    this.logger = logger || console.log
  }

  // Enable system proxy
  async enable(host = '127.0.0.1', port = 7890): Promise<boolean> {
    this.server = host
    this.port = port

    try {
      const proxyServer = `${host}:${port}`
      const bypassList = this.bypass.join(';')

      // 合并为单次 cmd 调用写入 3 个注册表值，减少子进程创建开销
      const cmd = [
        `reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d "1" /f`,
        `reg add "${REGISTRY_PATH}" /v ProxyServer /t REG_SZ /d "${proxyServer}" /f`,
        `reg add "${REGISTRY_PATH}" /v ProxyOverride /t REG_SZ /d "${bypassList}" /f`
      ].join(' && ')
      await execAsync(cmd, { windowsHide: true })

      // Notify system of proxy change
      await this.notifyProxyChange()

      this.enabled = true
      this.log(`System proxy enabled: ${proxyServer}`)
      return true
    } catch (err) {
      this.log(`Failed to enable system proxy: ${err}`)
      return false
    }
  }

  // Disable system proxy
  async disable(): Promise<boolean> {
    try {
      await execAsync(
        `reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d "0" /f && (reg delete "${REGISTRY_PATH}" /v ProxyServer /f 2>nul || exit /b 0)`,
        { windowsHide: true }
      )

      await this.notifyProxyChange()

      this.enabled = false
      this.log('System proxy disabled')
      return true
    } catch (err) {
      this.log(`Failed to disable system proxy: ${err}`)
      return false
    }
  }

  // Get current proxy status
  async getStatus(): Promise<SystemProxyConfig> {
    try {
      const enableResult = await this.queryReg('ProxyEnable')
      const serverResult = await this.queryReg('ProxyServer')
      const bypassResult = await this.queryReg('ProxyOverride')

      const enabled = enableResult.includes('0x1')
      const serverMatch = serverResult.match(/ProxyServer\s+REG_SZ\s+(.+)/i)
      const bypassMatch = bypassResult.match(/ProxyOverride\s+REG_SZ\s+(.+)/i)

      let server = '127.0.0.1'
      let port = 7890

      if (serverMatch) {
        const parts = serverMatch[1].trim().split(':')
        server = parts[0]
        port = parseInt(parts[1]) || 7890
      }

      const bypass = bypassMatch
        ? bypassMatch[1].trim().split(';').filter(Boolean)
        : DEFAULT_BYPASS_LIST

      return { enabled, server, port, bypass }
    } catch {
      return { enabled: false, server: '127.0.0.1', port: 7890, bypass: DEFAULT_BYPASS_LIST }
    }
  }

  // Set bypass list
  setBypass(bypass: string[]): void {
    this.bypass = bypass
  }

  isEnabled(): boolean {
    return this.enabled
  }

  getPort(): number {
    return this.port
  }

  private async queryReg(name: string): Promise<string> {
    try {
      const { stdout } = await execAsync(
        `reg query "${REGISTRY_PATH}" /v ${name}`,
        { windowsHide: true }
      )
      return stdout
    } catch {
      return ''
    }
  }

  private async notifyProxyChange(): Promise<void> {
    const ps = `
      Add-Type -TypeDefinition @"
        using System;
        using System.Runtime.InteropServices;
        public class WinInet {
          [DllImport("wininet.dll", SetLastError = true)]
          public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
          public const int INTERNET_OPTION_SETTINGS_CHANGED = 39;
          public const int INTERNET_OPTION_REFRESH = 37;
        }
"@
      [WinInet]::InternetSetOption([IntPtr]::Zero, [WinInet]::INTERNET_OPTION_SETTINGS_CHANGED, [IntPtr]::Zero, 0)
      [WinInet]::InternetSetOption([IntPtr]::Zero, [WinInet]::INTERNET_OPTION_REFRESH, [IntPtr]::Zero, 0)
    `.replace(/\n/g, '; ')

    try {
      await execAsync(`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "${ps}"`, { windowsHide: true })
    } catch {
      this.log('Could not notify system of proxy change')
    }
  }

  private log(msg: string): void {
    this.logger(`[SystemProxy] ${msg}`)
  }
}

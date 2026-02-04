import { exec } from 'child_process'
import { promisify } from 'util'
import { existsSync, mkdirSync, renameSync, unlinkSync, readdirSync, statSync } from 'fs'
import { join, dirname } from 'path'
import { EventEmitter } from 'events'
import * as https from 'https'
import * as http from 'http'
import * as fs from 'fs'

const execAsync = promisify(exec)

const GITHUB_API_STABLE = 'https://api.github.com/repos/SagerNet/sing-box/releases/latest'
const GITHUB_API_BETA = 'https://api.github.com/repos/SagerNet/sing-box/releases?per_page=5'
const GITHUB_RELEASES_PAGE = 'https://github.com/SagerNet/sing-box/releases'

export interface KernelVersion {
  version: string
  versionDetail: string
  isAlpha: boolean
}

export interface RemoteRelease {
  version: string
  tagName: string
  publishedAt: string
  isPrerelease: boolean
  downloadUrl: string
  assetName: string
}

export interface DownloadProgress {
  downloaded: number
  total: number
  percent: number
}

export class KernelManager extends EventEmitter {
  private kernelDir: string
  private cacheDir: string
  private logger: (msg: string) => void

  constructor(kernelDir: string, cacheDir: string, logger?: (msg: string) => void) {
    super()
    this.kernelDir = kernelDir
    this.cacheDir = cacheDir
    this.logger = logger || console.log

    mkdirSync(this.kernelDir, { recursive: true })
    mkdirSync(this.cacheDir, { recursive: true })
  }

  private getKernelPath(isAlpha = false): string {
    const name = isAlpha ? 'sing-box-alpha.exe' : 'sing-box.exe'
    return join(this.kernelDir, name)
  }

  private getBackupPath(isAlpha = false): string {
    return this.getKernelPath(isAlpha) + '.bak'
  }

  // Get local kernel version
  async getLocalVersion(isAlpha = false): Promise<KernelVersion | null> {
    const kernelPath = this.getKernelPath(isAlpha)
    
    if (!existsSync(kernelPath)) {
      return null
    }

    try {
      const { stdout } = await execAsync(`"${kernelPath}" version`, { windowsHide: true })
      const versionMatch = stdout.match(/version\s+(\S+)/)
      const version = versionMatch ? versionMatch[1] : 'unknown'
      
      return {
        version,
        versionDetail: stdout.trim(),
        isAlpha
      }
    } catch (err) {
      this.log(`Failed to get local version: ${err}`)
      return null
    }
  }

  // Get remote releases from GitHub
  async getRemoteReleases(includePrerelease = true): Promise<RemoteRelease[]> {
    const releases: RemoteRelease[] = []

    try {
      // Get stable release
      const stableData = await this.fetchJson(GITHUB_API_STABLE)
      if (stableData && stableData.tag_name) {
        const asset = this.findWindowsAsset(stableData.assets, stableData.tag_name)
        if (asset) {
          releases.push({
            version: stableData.tag_name.replace('v', ''),
            tagName: stableData.tag_name,
            publishedAt: stableData.published_at,
            isPrerelease: false,
            downloadUrl: asset.browser_download_url,
            assetName: asset.name
          })
        }
      }

      // Get beta releases
      if (includePrerelease) {
        const betaData = await this.fetchJson(GITHUB_API_BETA)
        if (Array.isArray(betaData)) {
          for (const release of betaData) {
            if (release.prerelease) {
              const asset = this.findWindowsAsset(release.assets, release.tag_name)
              if (asset) {
                releases.push({
                  version: release.tag_name.replace('v', ''),
                  tagName: release.tag_name,
                  publishedAt: release.published_at,
                  isPrerelease: true,
                  downloadUrl: asset.browser_download_url,
                  assetName: asset.name
                })
                break // Only get latest beta
              }
            }
          }
        }
      }
    } catch (err) {
      this.log(`Failed to fetch remote releases: ${err}`)
    }

    return releases
  }

  private findWindowsAsset(assets: any[], tagName: string): any {
    const version = tagName.replace('v', '')
    const expectedName = `sing-box-${version}-windows-amd64.zip`
    return assets?.find((a: any) => a.name === expectedName)
  }

  // Download and install kernel
  async downloadKernel(release: RemoteRelease, isAlpha = false): Promise<boolean> {
    const kernelPath = this.getKernelPath(isAlpha)
    const backupPath = this.getBackupPath(isAlpha)
    const zipPath = join(this.cacheDir, release.assetName)
    const extractDir = join(this.cacheDir, 'extract_temp')

    try {
      this.log(`Downloading ${release.version} (${isAlpha ? 'alpha' : 'stable'})...`)
      this.emit('downloadStart', release)

      // Download zip file
      await this.downloadFile(release.downloadUrl, zipPath, (progress) => {
        this.emit('downloadProgress', progress)
      })

      this.log('Extracting...')
      this.emit('extractStart')

      // Backup existing kernel
      if (existsSync(kernelPath)) {
        if (existsSync(backupPath)) {
          unlinkSync(backupPath)
        }
        renameSync(kernelPath, backupPath)
        this.log('Backed up existing kernel')
      }

      // Extract zip
      mkdirSync(extractDir, { recursive: true })
      await this.extractZip(zipPath, extractDir)

      // Find and move sing-box.exe
      const extractedExe = this.findExeInDir(extractDir)
      if (!extractedExe) {
        throw new Error('sing-box.exe not found in archive')
      }

      renameSync(extractedExe, kernelPath)

      // Cleanup
      this.cleanupDir(extractDir)
      unlinkSync(zipPath)

      this.log(`Kernel ${release.version} installed successfully`)
      this.emit('downloadComplete', release)
      return true

    } catch (err) {
      this.log(`Download failed: ${err}`)
      this.emit('downloadError', err)

      // Restore backup if exists
      if (existsSync(backupPath) && !existsSync(kernelPath)) {
        renameSync(backupPath, kernelPath)
        this.log('Restored backup')
      }

      // Cleanup
      if (existsSync(zipPath)) unlinkSync(zipPath)
      if (existsSync(extractDir)) this.cleanupDir(extractDir)

      return false
    }
  }

  // Rollback to previous version
  async rollback(isAlpha = false): Promise<boolean> {
    const kernelPath = this.getKernelPath(isAlpha)
    const backupPath = this.getBackupPath(isAlpha)

    if (!existsSync(backupPath)) {
      this.log('No backup available for rollback')
      return false
    }

    try {
      if (existsSync(kernelPath)) {
        unlinkSync(kernelPath)
      }
      renameSync(backupPath, kernelPath)
      this.log('Rollback successful')
      return true
    } catch (err) {
      this.log(`Rollback failed: ${err}`)
      return false
    }
  }

  // Check if rollback is available
  canRollback(isAlpha = false): boolean {
    return existsSync(this.getBackupPath(isAlpha))
  }

  // Clear kernel cache
  async clearCache(): Promise<{ success: boolean; freedBytes: number }> {
    let freedBytes = 0

    try {
      // Clear download cache
      if (existsSync(this.cacheDir)) {
        const files = readdirSync(this.cacheDir)
        for (const file of files) {
          const filePath = join(this.cacheDir, file)
          const stat = statSync(filePath)
          if (stat.isFile()) {
            freedBytes += stat.size
            unlinkSync(filePath)
          } else if (stat.isDirectory()) {
            freedBytes += this.getDirSize(filePath)
            this.cleanupDir(filePath)
          }
        }
      }

      // Clear sing-box cache.db
      const cacheDb = join(dirname(this.kernelDir), 'cache.db')
      if (existsSync(cacheDb)) {
        const stat = statSync(cacheDb)
        freedBytes += stat.size
        unlinkSync(cacheDb)
      }

      this.log(`Cache cleared, freed ${(freedBytes / 1024 / 1024).toFixed(2)} MB`)
      return { success: true, freedBytes }
    } catch (err) {
      this.log(`Clear cache failed: ${err}`)
      return { success: false, freedBytes }
    }
  }

  // Get all available versions (local)
  async getInstalledVersions(): Promise<KernelVersion[]> {
    const versions: KernelVersion[] = []

    const stable = await this.getLocalVersion(false)
    if (stable) versions.push(stable)

    const alpha = await this.getLocalVersion(true)
    if (alpha) versions.push(alpha)

    return versions
  }

  private async fetchJson(url: string): Promise<any> {
    return new Promise((resolve, reject) => {
      const options = {
        headers: {
          'User-Agent': 'KunBox/1.0',
          'Accept': 'application/vnd.github.v3+json'
        }
      }

      https.get(url, options, (res) => {
        if (res.statusCode === 301 || res.statusCode === 302) {
          this.fetchJson(res.headers.location!).then(resolve).catch(reject)
          return
        }

        let data = ''
        res.on('data', chunk => data += chunk)
        res.on('end', () => {
          try {
            resolve(JSON.parse(data))
          } catch {
            reject(new Error('Invalid JSON response'))
          }
        })
      }).on('error', reject)
    })
  }

  private downloadFile(url: string, destPath: string, onProgress?: (p: DownloadProgress) => void): Promise<void> {
    return new Promise((resolve, reject) => {
      const file = fs.createWriteStream(destPath)
      
      const request = (currentUrl: string) => {
        const protocol = currentUrl.startsWith('https') ? https : http
        
        protocol.get(currentUrl, {
          headers: { 'User-Agent': 'KunBox/1.0' }
        }, (res) => {
          if (res.statusCode === 301 || res.statusCode === 302) {
            request(res.headers.location!)
            return
          }

          if (res.statusCode !== 200) {
            reject(new Error(`Download failed: ${res.statusCode}`))
            return
          }

          const total = parseInt(res.headers['content-length'] || '0', 10)
          let downloaded = 0

          res.on('data', (chunk) => {
            downloaded += chunk.length
            if (onProgress && total > 0) {
              onProgress({
                downloaded,
                total,
                percent: Math.round((downloaded / total) * 100)
              })
            }
          })

          res.pipe(file)
          file.on('finish', () => {
            file.close()
            resolve()
          })
        }).on('error', (err) => {
          fs.unlink(destPath, () => {})
          reject(err)
        })
      }

      request(url)
    })
  }

  private async extractZip(zipPath: string, destDir: string): Promise<void> {
    // Use PowerShell to extract
    const cmd = `powershell -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${destDir}' -Force"`
    await execAsync(cmd, { windowsHide: true })
  }

  private findExeInDir(dir: string): string | null {
    const items = readdirSync(dir)
    for (const item of items) {
      const itemPath = join(dir, item)
      const stat = statSync(itemPath)
      if (stat.isDirectory()) {
        const found = this.findExeInDir(itemPath)
        if (found) return found
      } else if (item === 'sing-box.exe') {
        return itemPath
      }
    }
    return null
  }

  private cleanupDir(dir: string): void {
    if (!existsSync(dir)) return
    const items = readdirSync(dir)
    for (const item of items) {
      const itemPath = join(dir, item)
      const stat = statSync(itemPath)
      if (stat.isDirectory()) {
        this.cleanupDir(itemPath)
      } else {
        unlinkSync(itemPath)
      }
    }
    fs.rmdirSync(dir)
  }

  private getDirSize(dir: string): number {
    let size = 0
    const items = readdirSync(dir)
    for (const item of items) {
      const itemPath = join(dir, item)
      const stat = statSync(itemPath)
      if (stat.isDirectory()) {
        size += this.getDirSize(itemPath)
      } else {
        size += stat.size
      }
    }
    return size
  }

  private log(msg: string): void {
    this.logger(`[KernelManager] ${msg}`)
  }
}

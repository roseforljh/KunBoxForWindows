import { ipcMain, app, BrowserWindow, net } from 'electron'
import { spawn } from 'child_process'
import { createWriteStream, existsSync, mkdirSync, unlinkSync } from 'fs'
import { dirname, join } from 'path'
import log from 'electron-log'
import { APP_VERSION } from '../../shared/constants'
import { cleanupBeforeQuit } from './singbox'
import { compareSemver, selectWindowsInstallerAsset, type GitHubAsset } from './updaterUtils'

const GITHUB_OWNER = 'roseforljh'
const GITHUB_REPO = 'KunBoxForWindows'
const LATEST_RELEASE_URL = `https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/latest`

interface GitHubRelease {
  tag_name?: string
  published_at?: string
  body?: string
  assets?: GitHubAsset[]
}

export interface UpdateCheckResult {
  currentVersion: string
  hasUpdate: boolean
  version?: string
  date?: string
  body?: string
}

let cachedRelease: { version: string; date?: string; body?: string; asset?: GitHubAsset } | null = null

function broadcast(channel: string, payload?: unknown): void {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) {
      win.webContents.send(channel, payload)
    }
  }
}

async function fetchLatestRelease(currentVersion: string): Promise<GitHubRelease> {
  const response = await net.fetch(LATEST_RELEASE_URL, {
    headers: {
      Accept: 'application/vnd.github+json',
      'User-Agent': `KunBox/${currentVersion}`
    }
  })

  if (!response.ok) {
    throw new Error(`GitHub API 请求失败 (HTTP ${response.status})`)
  }

  return (await response.json()) as GitHubRelease
}

async function checkLatestUpdate(): Promise<UpdateCheckResult> {
  const currentVersion = app.getVersion() || APP_VERSION
  const release = await fetchLatestRelease(currentVersion)

  const latestVersion = release.tag_name?.replace(/^v/i, '').trim()
  if (!latestVersion) {
    throw new Error('未获取到有效版本号')
  }

  const hasUpdate = compareSemver(latestVersion, currentVersion) > 0
  const asset = selectWindowsInstallerAsset(release.assets)

  cachedRelease = {
    version: latestVersion,
    date: release.published_at,
    body: release.body,
    asset
  }

  return {
    currentVersion,
    hasUpdate,
    version: latestVersion,
    date: release.published_at,
    body: release.body
  }
}

async function downloadInstaller(asset: GitHubAsset, currentVersion: string): Promise<string> {
  const tempDir = join(app.getPath('temp'), 'kunbox-updater')
  if (!existsSync(tempDir)) {
    mkdirSync(tempDir, { recursive: true })
  }

  // 防止路径穿越，只取纯文件名
  const safeName = asset.name.replace(/[^A-Za-z0-9._-]/g, '_')
  const targetPath = join(tempDir, safeName)
  if (existsSync(targetPath)) {
    unlinkSync(targetPath)
  }

  const response = await net.fetch(asset.browser_download_url, {
    headers: {
      'User-Agent': `KunBox/${currentVersion}`
    }
  })

  if (!response.ok || !response.body) {
    throw new Error(`下载更新包失败 (HTTP ${response.status})`)
  }

  const headerLength = Number.parseInt(response.headers.get('content-length') || '0', 10)
  const contentLength = headerLength > 0 ? headerLength : asset.size || 0
  let downloaded = 0
  let lastEmit = 0

  const writer = createWriteStream(targetPath)
  const reader = response.body.getReader()

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      if (value) {
        if (!writer.write(Buffer.from(value))) {
          await new Promise<void>((resolve) => writer.once('drain', resolve))
        }
        downloaded += value.byteLength
        const now = Date.now()
        if (now - lastEmit >= 120 || downloaded === contentLength) {
          lastEmit = now
          broadcast('updater:download-progress', { downloaded, contentLength })
        }
      }
    }
    await new Promise<void>((resolve, reject) => {
      writer.on('error', reject)
      writer.end(() => resolve())
    })
  } catch (err) {
    writer.destroy()
    if (existsSync(targetPath)) {
      try {
        unlinkSync(targetPath)
      } catch {
        // 忽略清理失败
      }
    }
    throw err
  }

  broadcast('updater:download-progress', { downloaded, contentLength: contentLength || downloaded })
  broadcast('updater:download-finished')
  return targetPath
}

export function initUpdaterHandlers(): void {
  ipcMain.handle('updater:get-current-version', () => {
    return app.getVersion() || APP_VERSION
  })

  ipcMain.handle('updater:check', async () => {
    return checkLatestUpdate()
  })

  ipcMain.handle('updater:download-and-install', async () => {
    const currentVersion = app.getVersion() || APP_VERSION

    if (!cachedRelease?.asset) {
      await checkLatestUpdate()
    }

    const release = cachedRelease
    if (!release || compareSemver(release.version, currentVersion) <= 0) {
      return {
        currentVersion,
        hasUpdate: false
      }
    }

    if (!release.asset) {
      throw new Error(`版本 v${release.version} 未包含 Windows 安装包`)
    }

    const installerPath = await downloadInstaller(release.asset, currentVersion)
    const installDir = dirname(process.execPath)

    log.info(`Starting silent in-place update: ${installerPath} -> ${installDir}`)

    // 先停止代理内核并清理系统代理，避免覆盖安装时文件被占用
    await cleanupBeforeQuit()

    // NSIS 规定 /D= 必须是最后一个参数且不能加引号；--updated 会保留桌面/任务栏快捷方式与数据，--force-run 安装完自动重启
    const child = spawn(
      `"${installerPath}" --updated --force-run /S /D=${installDir}`,
      [],
      {
        detached: true,
        stdio: 'ignore',
        shell: true,
        windowsHide: true
      }
    )
    child.unref()

    ;(app as any).isQuitting = true
    setTimeout(() => {
      app.quit()
    }, 300)

    return {
      currentVersion,
      hasUpdate: true,
      version: release.version,
      date: release.date,
      body: release.body
    }
  })
}

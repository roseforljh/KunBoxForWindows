import { ipcMain, shell } from 'electron'
import { join } from 'path'
import { existsSync, mkdirSync } from 'fs'
import log from 'electron-log'
import { is } from '@electron-toolkit/utils'
import { KernelManager, RAYBOX_REPO_URL } from '../core/KernelManager'
import { getServiceInstance, generateAndSaveConfig } from './singbox'
import { stopTempSingbox } from './profiles'
import { settingsStore } from './settings'
import { getDataDir } from '../utils/paths'

let kernelManager: KernelManager | null = null

export function getKernelManager(): KernelManager {
  if (!kernelManager) {
    const dataDir = getDataDir()
    const dataKernelDir = join(dataDir, 'libs')
    const bundleKernelDir = is.dev
      ? join(__dirname, '../../resources/libs')
      : join(process.resourcesPath, 'resources/libs')
    const cacheDir = join(dataDir, 'cache')

    kernelManager = new KernelManager({
      dataKernelDir,
      bundleKernelDir,
      cacheDir,
      isDev: is.dev,
      logger: (msg) => log.info(msg),
      lifecycleHooks: {
        cleanupTempProcesses: () => {
          stopTempSingbox()
        },
        isRunning: () => {
          const service = getServiceInstance()
          if (!service) return false
          const state = service.getState()
          return service.isRunning() || state === 'starting' || state === 'stopping'
        },
        prepareForInstall: async () => {
          const service = getServiceInstance()
          if (!service) return { wasRunning: false }
          const state = service.getState()
          const wasRunning = service.isRunning() || state === 'starting'
          if (wasRunning) {
            const stopped = await service.stop({ clearProxy: true })
            if (!stopped.success) {
              throw new Error(stopped.error || '停止内核失败')
            }
          }
          return { wasRunning }
        },
        restartAfterInstall: async () => {
          const service = getServiceInstance()
          if (!service) return { success: false, error: 'VPN 服务未初始化' }
          const execPath = await getKernelManager().resolveKernelPath()
          const genResult = generateAndSaveConfig()
          if (!genResult.success || !genResult.configPath) {
            return { success: false, error: genResult.error || '生成配置失败' }
          }
          const settings = settingsStore.get('settings')
          const dataDir = getDataDir()
          service.updateConfig({
            core: {
              execPath,
              configDir: dataDir,
              workDir: dataDir,
              apiHost: '127.0.0.1',
              apiPort: 9090,
              apiSecret: ''
            },
            autoSystemProxy: settings.systemProxy,
            proxyPort: settings.localPort || 7890
          })
          return service.start({ configPath: genResult.configPath })
        },
        stopAfterFailedRestart: async () => {
          const service = getServiceInstance()
          if (!service) return
          const stopped = await service.stop({ clearProxy: true })
          if (!stopped.success) {
            throw new Error(stopped.error || '停止失败')
          }
        }
      }
    })
  }
  return kernelManager
}

export function initKernelHandlers(): void {
  const manager = getKernelManager()

  ipcMain.handle('kernel:get-local-version', async (_, isAlpha = false) => {
    return manager.getLocalVersion(isAlpha)
  })

  ipcMain.handle('kernel:get-installed-versions', async () => {
    return manager.getInstalledVersions()
  })

  ipcMain.handle('kernel:get-capabilities', async () => {
    return manager.getCapabilities()
  })

  ipcMain.handle('kernel:get-remote-releases', async (_, includePrerelease = false) => {
    return manager.getRemoteReleases(includePrerelease)
  })

  ipcMain.handle('kernel:download', async (event, tagName: string, isAlpha = false) => {
    const progressHandler = (progress: any) => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('kernel:download-progress', progress)
      }
    }
    const completeHandler = () => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('kernel:download-complete')
      }
    }
    const errorHandler = (err: any) => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('kernel:download-error', err?.message || String(err))
      }
    }

    manager.on('downloadProgress', progressHandler)
    manager.on('downloadComplete', completeHandler)
    manager.on('downloadError', errorHandler)

    try {
      const ok = await manager.downloadKernel(tagName, isAlpha)
      return { success: ok }
    } finally {
      manager.off('downloadProgress', progressHandler)
      manager.off('downloadComplete', completeHandler)
      manager.off('downloadError', errorHandler)
    }
  })

  ipcMain.handle('kernel:rollback', async (_, isAlpha = false) => {
    const result = await manager.rollback(isAlpha)
    return { success: result }
  })

  ipcMain.handle('kernel:can-rollback', async (_, isAlpha = false) => {
    return manager.canRollback(isAlpha)
  })

  ipcMain.handle('kernel:clear-cache', async () => {
    return manager.clearCache()
  })

  ipcMain.handle('kernel:open-releases-page', async () => {
    shell.openExternal(`${RAYBOX_REPO_URL}/releases`)
  })

  ipcMain.handle('kernel:open-directory', async () => {
    const targetDir = manager.getPreferredOpenDirectory()
    if (!existsSync(targetDir)) {
      mkdirSync(targetDir, { recursive: true })
    }
    shell.openPath(targetDir)
  })

  log.info('Kernel handlers initialized')
}

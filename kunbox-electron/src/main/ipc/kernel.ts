import { ipcMain, app, shell } from 'electron'
import { join } from 'path'
import log from 'electron-log'
import { is } from '@electron-toolkit/utils'
import { KernelManager } from '@kunbox/core'

let kernelManager: KernelManager | null = null

function getKernelManager(): KernelManager {
  if (!kernelManager) {
    const userDataPath = app.getPath('userData')
    const kernelDir = is.dev 
      ? join(__dirname, '../../resources/libs')
      : join(process.resourcesPath, 'resources/libs')
    const cacheDir = join(userDataPath, 'KunBox', 'cache')

    kernelManager = new KernelManager(kernelDir, cacheDir, (msg) => log.info(msg))
  }
  return kernelManager
}

export function initKernelHandlers(): void {
  const manager = getKernelManager()

  // Get local kernel version
  ipcMain.handle('kernel:get-local-version', async (_, isAlpha: boolean = false) => {
    return manager.getLocalVersion(isAlpha)
  })

  // Get all installed versions
  ipcMain.handle('kernel:get-installed-versions', async () => {
    return manager.getInstalledVersions()
  })

  // Get remote releases from GitHub
  ipcMain.handle('kernel:get-remote-releases', async (_, includePrerelease: boolean = true) => {
    return manager.getRemoteReleases(includePrerelease)
  })

  // Download and install kernel
  ipcMain.handle('kernel:download', async (event, release: any, isAlpha: boolean = false) => {
    // Set up progress forwarding
    const progressHandler = (progress: any) => {
      event.sender.send('kernel:download-progress', progress)
    }
    const startHandler = () => {
      event.sender.send('kernel:download-start')
    }
    const completeHandler = () => {
      event.sender.send('kernel:download-complete')
    }
    const errorHandler = (err: any) => {
      event.sender.send('kernel:download-error', err?.message || String(err))
    }

    manager.on('downloadProgress', progressHandler)
    manager.on('downloadStart', startHandler)
    manager.on('downloadComplete', completeHandler)
    manager.on('downloadError', errorHandler)

    try {
      const result = await manager.downloadKernel(release, isAlpha)
      return { success: result }
    } finally {
      manager.off('downloadProgress', progressHandler)
      manager.off('downloadStart', startHandler)
      manager.off('downloadComplete', completeHandler)
      manager.off('downloadError', errorHandler)
    }
  })

  // Rollback to previous version
  ipcMain.handle('kernel:rollback', async (_, isAlpha: boolean = false) => {
    const result = await manager.rollback(isAlpha)
    return { success: result }
  })

  // Check if rollback is available
  ipcMain.handle('kernel:can-rollback', async (_, isAlpha: boolean = false) => {
    return manager.canRollback(isAlpha)
  })

  // Clear kernel cache
  ipcMain.handle('kernel:clear-cache', async () => {
    return manager.clearCache()
  })

  // Open releases page in browser
  ipcMain.handle('kernel:open-releases-page', async () => {
    shell.openExternal('https://github.com/SagerNet/sing-box/releases')
  })

  // Open kernel directory
  ipcMain.handle('kernel:open-directory', async () => {
    const kernelDir = is.dev 
      ? join(__dirname, '../../resources/libs')
      : join(process.resourcesPath, 'resources/libs')
    shell.openPath(kernelDir)
  })

  log.info('Kernel handlers initialized')
}

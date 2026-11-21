import { ipcMain, app } from 'electron'
import https from 'https'
import { APP_VERSION } from '../../shared/constants'

export function initUpdaterHandlers(): void {
  ipcMain.handle('updater:get-current-version', () => {
    return app.getVersion() || APP_VERSION
  })

  ipcMain.handle('updater:check', async () => {
    const currentVersion = app.getVersion() || APP_VERSION
    try {
      const data = await new Promise<any>((resolve, reject) => {
        https.get('https://api.github.com/repos/kunbox/kunbox-windows/releases/latest', {
          headers: { 'User-Agent': 'KunBox/0.0.24' }
        }, (res) => {
          let body = ''
          res.on('data', chunk => body += chunk)
          res.on('end', () => {
            try {
              resolve(JSON.parse(body))
            } catch (e) {
              reject(e)
            }
          })
        }).on('error', reject)
      })

      const latestTag = data.tag_name ? data.tag_name.replace(/^v/, '') : currentVersion
      const hasUpdate = latestTag !== currentVersion

      return {
        currentVersion,
        hasUpdate,
        version: latestTag,
        date: data.published_at,
        body: data.body
      }
    } catch {
      return {
        currentVersion,
        hasUpdate: false
      }
    }
  })

  ipcMain.handle('updater:download-and-install', async () => {
    return {
      currentVersion: app.getVersion() || APP_VERSION,
      hasUpdate: false
    }
  })
}

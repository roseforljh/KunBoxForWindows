import { useState, useEffect, useCallback } from 'react'
import { Download, RotateCcw, Trash2, ExternalLink, FolderOpen, RefreshCw, AlertTriangle } from 'lucide-react'

interface KernelVersion {
  version: string
  versionDetail: string
  isAlpha: boolean
}

interface RemoteRelease {
  version: string
  tagName: string
  publishedAt: string
  isPrerelease: boolean
  downloadUrl: string
  assetName: string
}

interface DownloadProgress {
  downloaded: number
  total: number
  percent: number
}

export function KernelSettings() {
  const [localStable, setLocalStable] = useState<KernelVersion | null>(null)
  const [localAlpha, setLocalAlpha] = useState<KernelVersion | null>(null)
  const [remoteReleases, setRemoteReleases] = useState<RemoteRelease[]>([])
  const [loading, setLoading] = useState(true)
  const [downloading, setDownloading] = useState(false)
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null)
  const [canRollbackStable, setCanRollbackStable] = useState(false)
  const [canRollbackAlpha, setCanRollbackAlpha] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadVersions = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [stable, alpha, releases, rollbackStable, rollbackAlpha] = await Promise.all([
        window.api.kernel.getLocalVersion(false),
        window.api.kernel.getLocalVersion(true),
        window.api.kernel.getRemoteReleases(true),
        window.api.kernel.canRollback(false),
        window.api.kernel.canRollback(true)
      ])
      setLocalStable(stable)
      setLocalAlpha(alpha)
      setRemoteReleases(releases)
      setCanRollbackStable(rollbackStable)
      setCanRollbackAlpha(rollbackAlpha)
    } catch (err) {
      setError(String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadVersions()

    const unsubProgress = window.api.kernel.onDownloadProgress((progress) => {
      setDownloadProgress(progress)
    })
    const unsubComplete = window.api.kernel.onDownloadComplete(() => {
      setDownloading(false)
      setDownloadProgress(null)
      loadVersions()
    })
    const unsubError = window.api.kernel.onDownloadError((err) => {
      setDownloading(false)
      setDownloadProgress(null)
      setError(err)
    })

    return () => {
      unsubProgress()
      unsubComplete()
      unsubError()
    }
  }, [loadVersions])

  const handleDownload = async (release: RemoteRelease) => {
    setDownloading(true)
    setError(null)
    try {
      await window.api.kernel.download(release, release.isPrerelease)
    } catch (err) {
      setError(String(err))
      setDownloading(false)
    }
  }

  const handleRollback = async (isAlpha: boolean) => {
    try {
      const result = await window.api.kernel.rollback(isAlpha)
      if (result.success) {
        loadVersions()
      } else {
        setError('Rollback failed')
      }
    } catch (err) {
      setError(String(err))
    }
  }

  const handleClearCache = async () => {
    try {
      const result = await window.api.kernel.clearCache()
      if (result.success) {
        const freedMB = (result.freedBytes / 1024 / 1024).toFixed(2)
        alert(`Cache cleared! Freed ${freedMB} MB`)
      }
    } catch (err) {
      setError(String(err))
    }
  }

  const remoteStable = remoteReleases.find(r => !r.isPrerelease)
  const remoteBeta = remoteReleases.find(r => r.isPrerelease)

  const stableUpdatable = remoteStable && localStable?.version !== remoteStable.version
  const betaUpdatable = remoteBeta && localAlpha?.version !== remoteBeta.version

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-medium">内核管理</h3>
        <div className="flex gap-2">
          <button
            onClick={loadVersions}
            disabled={loading}
            className="p-2 rounded-lg bg-white/5 hover:bg-white/10 transition-colors"
            title="刷新"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button
            onClick={() => window.api.kernel.openReleasesPage()}
            className="p-2 rounded-lg bg-white/5 hover:bg-white/10 transition-colors"
            title="GitHub Releases"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          <button
            onClick={() => window.api.kernel.openDirectory()}
            className="p-2 rounded-lg bg-white/5 hover:bg-white/10 transition-colors"
            title="打开内核目录"
          >
            <FolderOpen className="w-4 h-4" />
          </button>
          <button
            onClick={handleClearCache}
            className="p-2 rounded-lg bg-white/5 hover:bg-white/10 transition-colors"
            title="清理缓存"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      </div>

      {error && (
        <div className="p-3 rounded-lg bg-red-500/20 border border-red-500/30 flex items-center gap-2">
          <AlertTriangle className="w-4 h-4 text-red-400" />
          <span className="text-sm text-red-400">{error}</span>
        </div>
      )}

      {downloading && downloadProgress && (
        <div className="p-4 rounded-lg bg-blue-500/10 border border-blue-500/30">
          <div className="flex items-center justify-between mb-2">
            <span className="text-sm">正在下载...</span>
            <span className="text-sm">{downloadProgress.percent}%</span>
          </div>
          <div className="h-2 bg-white/10 rounded-full overflow-hidden">
            <div 
              className="h-full bg-blue-500 transition-all duration-300"
              style={{ width: `${downloadProgress.percent}%` }}
            />
          </div>
          <div className="text-xs text-white/50 mt-1">
            {(downloadProgress.downloaded / 1024 / 1024).toFixed(2)} / {(downloadProgress.total / 1024 / 1024).toFixed(2)} MB
          </div>
        </div>
      )}

      {/* Stable Version */}
      <div className="p-4 rounded-lg bg-white/5 border border-white/10">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <span className="font-medium">正式版</span>
            {canRollbackStable && (
              <button
                onClick={() => handleRollback(false)}
                className="p-1 rounded bg-white/5 hover:bg-white/10 transition-colors"
                title="回退到上一版本"
              >
                <RotateCcw className="w-3 h-3" />
              </button>
            )}
          </div>
          {stableUpdatable && remoteStable && (
            <button
              onClick={() => handleDownload(remoteStable)}
              disabled={downloading}
              className="flex items-center gap-1 px-3 py-1 rounded-lg bg-blue-500 hover:bg-blue-600 text-sm transition-colors disabled:opacity-50"
            >
              <Download className="w-3 h-3" />
              更新到 {remoteStable.version}
            </button>
          )}
        </div>
        <div className="flex gap-4 text-sm">
          <div>
            <span className="text-white/50">本地: </span>
            <span className={localStable ? 'text-green-400' : 'text-red-400'}>
              {loading ? '加载中...' : localStable?.version || '未安装'}
            </span>
          </div>
          <div>
            <span className="text-white/50">远程: </span>
            <span>{remoteStable?.version || '-'}</span>
          </div>
        </div>
        {localStable?.versionDetail && (
          <div className="mt-2 text-xs text-white/40 font-mono whitespace-pre-wrap">
            {localStable.versionDetail}
          </div>
        )}
      </div>

      {/* Beta Version */}
      <div className="p-4 rounded-lg bg-white/5 border border-white/10">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <span className="font-medium">测试版 (Beta)</span>
            <span className="text-xs px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">Alpha</span>
            {canRollbackAlpha && (
              <button
                onClick={() => handleRollback(true)}
                className="p-1 rounded bg-white/5 hover:bg-white/10 transition-colors"
                title="回退到上一版本"
              >
                <RotateCcw className="w-3 h-3" />
              </button>
            )}
          </div>
          {betaUpdatable && remoteBeta && (
            <button
              onClick={() => handleDownload(remoteBeta)}
              disabled={downloading}
              className="flex items-center gap-1 px-3 py-1 rounded-lg bg-yellow-500/80 hover:bg-yellow-500 text-black text-sm transition-colors disabled:opacity-50"
            >
              <Download className="w-3 h-3" />
              安装 {remoteBeta.version}
            </button>
          )}
        </div>
        <div className="flex gap-4 text-sm">
          <div>
            <span className="text-white/50">本地: </span>
            <span className={localAlpha ? 'text-green-400' : 'text-white/30'}>
              {loading ? '加载中...' : localAlpha?.version || '未安装'}
            </span>
          </div>
          <div>
            <span className="text-white/50">远程: </span>
            <span>{remoteBeta?.version || '-'}</span>
          </div>
        </div>
        {localAlpha?.versionDetail && (
          <div className="mt-2 text-xs text-white/40 font-mono whitespace-pre-wrap">
            {localAlpha.versionDetail}
          </div>
        )}
      </div>

      <div className="text-xs text-white/40">
        <p>提示: 正式版更稳定，测试版包含最新功能但可能不稳定。</p>
        <p className="mt-1">内核将从 GitHub 官方仓库下载。</p>
      </div>
    </div>
  )
}

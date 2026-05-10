import { useCallback, useEffect, useState } from 'react'
import type { ComponentType } from 'react'
import { motion } from 'framer-motion'
import { AlertCircle, Check, Download, ExternalLink, FolderOpen, Loader2, PlugZap, RefreshCw } from 'lucide-react'
import { useToast } from './ui/Toast'

interface PluginVersion {
  version: string
  versionDetail: string
}

interface PluginRelease {
  version: string
  tagName: string
  publishedAt: string
  isPrerelease: boolean
  downloadUrl: string
  assetName: string
}

export default function Plugins() {
  const [localVersion, setLocalVersion] = useState<PluginVersion | null>(null)
  const [remoteReleases, setRemoteReleases] = useState<PluginRelease[]>([])
  const [loading, setLoading] = useState(true)
  const [downloading, setDownloading] = useState(false)
  const [progress, setProgress] = useState<number | null>(null)
  const { success: toastSuccess, error: toastError } = useToast()

  const loadPlugins = useCallback(async () => {
    setLoading(true)
    const [localResult, releasesResult] = await Promise.allSettled([
      window.api.plugin.getXrayLocalVersion(),
      window.api.plugin.getXrayRemoteReleases()
    ])

    if (localResult.status === 'fulfilled') {
      setLocalVersion(localResult.value)
    } else {
      setLocalVersion(null)
    }

    if (releasesResult.status === 'fulfilled') {
      setRemoteReleases(releasesResult.value)
    } else {
      setRemoteReleases([])
    }

    setLoading(false)
  }, [])

  useEffect(() => {
    void Promise.resolve().then(loadPlugins)

    const unsubProgress = window.api.plugin.onDownloadProgress((value) => {
      setProgress(value.percent)
    })
    const unsubComplete = window.api.plugin.onDownloadComplete(() => {
      setDownloading(false)
      setProgress(null)
      toastSuccess('Xray 插件核心安装完成')
      loadPlugins()
    })
    const unsubError = window.api.plugin.onDownloadError((err) => {
      setDownloading(false)
      setProgress(null)
      toastError(`下载失败: ${err}`)
    })

    return () => {
      unsubProgress()
      unsubComplete()
      unsubError()
    }
  }, [loadPlugins, toastError, toastSuccess])

  const latestRelease = remoteReleases.find((release) => !release.isPrerelease) ?? remoteReleases[0]
  const isInstalled = Boolean(localVersion)
  const isUpdatable = Boolean(localVersion && latestRelease && localVersion.version !== latestRelease.version)

  const handleDownload = async () => {
    if (!latestRelease) return
    setDownloading(true)
    setProgress(null)
    try {
      await window.api.plugin.downloadXray(latestRelease.tagName)
    } catch (err) {
      setDownloading(false)
      setProgress(null)
      toastError(String(err))
    }
  }

  return (
    <div className="flex-1 p-6 space-y-6 overflow-auto">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-11 h-11 rounded-2xl bg-[var(--accent-primary)]/10 flex items-center justify-center">
            <PlugZap className="w-6 h-6 text-[var(--accent-primary)]" />
          </div>
          <div>
            <h1 className="text-2xl font-bold text-[var(--text-primary)]">插件</h1>
            <p className="text-sm text-[var(--text-muted)]">管理 sing-box 不支持协议所需的插件核心</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <ActionButton icon={RefreshCw} onClick={loadPlugins} tooltip="刷新" />
          <ActionButton icon={ExternalLink} onClick={() => window.api.plugin.openXrayReleasesPage()} tooltip="GitHub" />
          <ActionButton icon={FolderOpen} onClick={() => window.api.plugin.openDirectory()} tooltip="打开目录" />
        </div>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        className="glass-card rounded-2xl p-6 border border-[var(--glass-border)]"
      >
        {loading ? (
          <div className="py-16 flex flex-col items-center justify-center">
            <Loader2 className="w-10 h-10 animate-spin text-[var(--accent-primary)]" />
            <p className="mt-4 text-sm text-[var(--text-muted)]">加载中...</p>
          </div>
        ) : (
          <div className="space-y-6">
            <div className="flex items-start justify-between gap-4">
              <div className="flex items-start gap-4">
                <div className={`w-12 h-12 rounded-xl flex items-center justify-center ${
                  isInstalled ? 'bg-emerald-500/10' : 'bg-amber-500/10'
                }`}>
                  {isInstalled
                    ? <Check className="w-6 h-6 text-emerald-400" />
                    : <AlertCircle className="w-6 h-6 text-amber-400" />
                  }
                </div>
                <div>
                  <h2 className="text-lg font-semibold text-[var(--text-primary)]">Xray 插件核心</h2>
                  <p className="text-sm text-[var(--text-muted)] mt-1">
                    用于桥接 VLESS xhttp 等 sing-box 当前不支持的传输协议。
                  </p>
                </div>
              </div>
              <span className={`px-3 py-1 rounded-full text-xs ${
                isInstalled ? 'bg-emerald-500/15 text-emerald-400' : 'bg-amber-500/15 text-amber-400'
              }`}>
                {isInstalled ? '已安装' : '未安装'}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <VersionCard label="本地版本" value={localVersion?.version || '未安装'} highlight={isInstalled} />
              <VersionCard label="最新版本" value={latestRelease?.version || '-'} highlight />
            </div>

            {!isInstalled && (
              <div className="p-4 rounded-xl bg-amber-500/10 border border-amber-500/20">
                <p className="text-sm font-medium text-amber-400">未检测到 Xray 插件核心</p>
                <p className="text-xs text-[var(--text-muted)] mt-1">
                  使用 xhttp 节点前需要先安装 Xray 插件核心。
                </p>
              </div>
            )}

            {localVersion?.versionDetail && (
              <div className="p-3 rounded-lg bg-[var(--bg-tertiary)]/30 text-xs font-mono text-[var(--text-faint)] whitespace-pre-wrap max-h-28 overflow-y-auto">
                {localVersion.versionDetail}
              </div>
            )}

            {latestRelease && (isUpdatable || !isInstalled) ? (
              <motion.button
                whileHover={{ scale: 1.01 }}
                whileTap={{ scale: 0.99 }}
                onClick={handleDownload}
                disabled={downloading}
                className="w-full flex items-center justify-center gap-2 py-3 rounded-xl text-white font-medium bg-gradient-to-r from-[var(--accent-primary)] to-[var(--accent-secondary)] disabled:opacity-70"
              >
                {downloading ? (
                  <>
                    <Loader2 className="w-5 h-5 animate-spin" />
                    <span>{progress == null ? '正在下载...' : `正在下载 ${progress}%`}</span>
                  </>
                ) : (
                  <>
                    <Download className="w-5 h-5" />
                    <span>{isInstalled ? `更新到 ${latestRelease.version}` : `下载 Xray ${latestRelease.version}`}</span>
                  </>
                )}
              </motion.button>
            ) : isInstalled && latestRelease ? (
              <div className="w-full flex items-center justify-center gap-2 py-3 rounded-xl bg-[var(--bg-tertiary)]/50 text-[var(--text-muted)]">
                <Check className="w-5 h-5" />
                <span>已是最新版本</span>
              </div>
            ) : !isInstalled ? (
              <div className="w-full flex items-center justify-center gap-2 py-3 rounded-xl bg-[var(--bg-tertiary)]/50 text-[var(--text-muted)]">
                <AlertCircle className="w-5 h-5" />
                <span>未获取到最新版本</span>
              </div>
            ) : null}
          </div>
        )}
      </motion.div>
    </div>
  )
}

function VersionCard({ label, value, highlight }: { label: string; value: string; highlight?: boolean }) {
  return (
    <div className="p-4 rounded-xl bg-[var(--bg-tertiary)]/50">
      <div className="text-xs text-[var(--text-muted)] mb-1">{label}</div>
      <div className={`text-lg font-mono font-semibold ${highlight ? 'text-emerald-400' : 'text-[var(--text-faint)]'}`}>
        {value}
      </div>
    </div>
  )
}

function ActionButton({
  icon: Icon,
  onClick,
  tooltip
}: {
  icon: ComponentType<{ className?: string }>
  onClick: () => void
  tooltip: string
}) {
  return (
    <button
      onClick={onClick}
      title={tooltip}
      className="w-9 h-9 flex items-center justify-center rounded-lg bg-[var(--bg-tertiary)]/50 hover:bg-[var(--bg-hover)] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors"
    >
      <Icon className="w-4 h-4" />
    </button>
  )
}

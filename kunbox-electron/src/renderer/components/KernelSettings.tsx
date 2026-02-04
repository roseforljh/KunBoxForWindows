import { useState, useEffect, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { 
  Download, RotateCcw, Trash2, ExternalLink, FolderOpen, RefreshCw, 
  Check, XCircle, AlertCircle, Loader2, Cpu, CheckCircle2
} from 'lucide-react'

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

interface ToastMessage {
  id: number
  message: string
  type: 'success' | 'error' | 'info'
}

export function KernelSettings() {
  const [localStable, setLocalStable] = useState<KernelVersion | null>(null)
  const [localAlpha, setLocalAlpha] = useState<KernelVersion | null>(null)
  const [remoteReleases, setRemoteReleases] = useState<RemoteRelease[]>([])
  const [loading, setLoading] = useState(true)
  const [downloadingStable, setDownloadingStable] = useState(false)
  const [downloadingAlpha, setDownloadingAlpha] = useState(false)
  const [canRollbackStable, setCanRollbackStable] = useState(false)
  const [canRollbackAlpha, setCanRollbackAlpha] = useState(false)
  const [toasts, setToasts] = useState<ToastMessage[]>([])

  const showToast = useCallback((message: string, type: 'success' | 'error' | 'info' = 'info') => {
    const id = Date.now()
    setToasts(prev => [...prev, { id, message, type }])
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id))
    }, 3000)
  }, [])

  const loadVersions = useCallback(async () => {
    setLoading(true)
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
      showToast('加载版本信息失败', 'error')
    } finally {
      setLoading(false)
    }
  }, [showToast])

  useEffect(() => {
    loadVersions()

    const unsubComplete = window.api.kernel.onDownloadComplete(() => {
      setDownloadingStable(false)
      setDownloadingAlpha(false)
      showToast('内核更新完成', 'success')
      loadVersions()
    })
    const unsubError = window.api.kernel.onDownloadError((err) => {
      setDownloadingStable(false)
      setDownloadingAlpha(false)
      showToast(`下载失败: ${err}`, 'error')
    })

    return () => {
      unsubComplete()
      unsubError()
    }
  }, [loadVersions, showToast])

  const handleDownload = async (release: RemoteRelease, isAlpha: boolean) => {
    if (isAlpha) {
      setDownloadingAlpha(true)
    } else {
      setDownloadingStable(true)
    }
    try {
      await window.api.kernel.download(release, isAlpha)
    } catch (err) {
      showToast(String(err), 'error')
      setDownloadingStable(false)
      setDownloadingAlpha(false)
    }
  }

  const handleRollback = async (isAlpha: boolean) => {
    try {
      const result = await window.api.kernel.rollback(isAlpha)
      if (result.success) {
        showToast('已回退到上一版本', 'success')
        loadVersions()
      } else {
        showToast('回退失败', 'error')
      }
    } catch (err) {
      showToast(String(err), 'error')
    }
  }

  const handleClearCache = async () => {
    try {
      const result = await window.api.kernel.clearCache()
      if (result.success) {
        const freedMB = (result.freedBytes / 1024 / 1024).toFixed(2)
        showToast(`缓存已清理，释放 ${freedMB} MB`, 'success')
      }
    } catch (err) {
      showToast(String(err), 'error')
    }
  }

  const remoteStable = remoteReleases.find(r => !r.isPrerelease)
  const remoteBeta = remoteReleases.find(r => r.isPrerelease)

  const stableUpdatable = remoteStable && localStable?.version !== remoteStable.version
  const betaUpdatable = remoteBeta && (!localAlpha || localAlpha.version !== remoteBeta.version)

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="p-2.5 rounded-xl bg-gradient-to-br from-[var(--accent-primary)]/20 to-[var(--accent-primary)]/5 border border-[var(--accent-primary)]/20">
            <Cpu className="w-5 h-5 text-[var(--accent-primary)]" />
          </div>
          <div>
            <h3 className="text-base font-semibold text-[var(--text-primary)]">sing-box 内核</h3>
            <p className="text-xs text-[var(--text-muted)]">管理代理核心程序版本</p>
          </div>
        </div>
        <div className="flex gap-1.5">
          <ActionButton 
            icon={RefreshCw} 
            onClick={loadVersions} 
            loading={loading}
            tooltip="刷新"
          />
          <ActionButton 
            icon={ExternalLink} 
            onClick={() => window.api.kernel.openReleasesPage()}
            tooltip="GitHub"
          />
          <ActionButton 
            icon={FolderOpen} 
            onClick={() => window.api.kernel.openDirectory()}
            tooltip="打开目录"
          />
          <ActionButton 
            icon={Trash2} 
            onClick={handleClearCache}
            tooltip="清理缓存"
          />
        </div>
      </div>

      {/* Stable Version Card */}
      <motion.div 
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
        className="glass-card rounded-2xl p-5 border border-[var(--glass-border)]"
      >
        <div className="flex items-start justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-emerald-500/10 flex items-center justify-center">
              <CheckCircle2 className="w-5 h-5 text-emerald-400" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-[var(--text-primary)]">正式版</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded-md bg-emerald-500/20 text-emerald-400 font-medium">
                  Stable
                </span>
              </div>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">推荐使用，稳定可靠</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {canRollbackStable && (
              <motion.button
                whileHover={{ scale: 1.05 }}
                whileTap={{ scale: 0.95 }}
                onClick={() => handleRollback(false)}
                className="p-2 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] transition-colors"
                title="回退版本"
              >
                <RotateCcw className="w-4 h-4 text-[var(--text-muted)]" />
              </motion.button>
            )}
            {stableUpdatable && remoteStable && (
              <motion.button
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.98 }}
                onClick={() => handleDownload(remoteStable, false)}
                disabled={downloadingStable}
                className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-emerald-500 to-emerald-600 hover:from-emerald-400 hover:to-emerald-500 text-white text-sm font-medium shadow-lg shadow-emerald-500/20 transition-all disabled:opacity-70"
              >
                {downloadingStable ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                <span>{downloadingStable ? '下载中...' : `更新 ${remoteStable.version}`}</span>
              </motion.button>
            )}
          </div>
        </div>
        <div className="flex items-center gap-6 text-sm">
          <div className="flex items-center gap-2">
            <span className="text-[var(--text-muted)]">本地:</span>
            <span className={`font-mono ${localStable ? 'text-emerald-400' : 'text-[var(--text-faint)]'}`}>
              {loading ? '...' : localStable?.version || '未安装'}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[var(--text-muted)]">最新:</span>
            <span className="font-mono text-[var(--text-secondary)]">
              {remoteStable?.version || '-'}
            </span>
          </div>
        </div>
      </motion.div>

      {/* Beta Version Card */}
      <motion.div 
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay: 0.1 }}
        className="glass-card rounded-2xl p-5 border border-[var(--glass-border)]"
      >
        <div className="flex items-start justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-amber-500/10 flex items-center justify-center">
              <AlertCircle className="w-5 h-5 text-amber-400" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-[var(--text-primary)]">测试版</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded-md bg-amber-500/20 text-amber-400 font-medium">
                  Beta
                </span>
              </div>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">包含最新功能，可能不稳定</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {canRollbackAlpha && (
              <motion.button
                whileHover={{ scale: 1.05 }}
                whileTap={{ scale: 0.95 }}
                onClick={() => handleRollback(true)}
                className="p-2 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] transition-colors"
                title="回退版本"
              >
                <RotateCcw className="w-4 h-4 text-[var(--text-muted)]" />
              </motion.button>
            )}
            {betaUpdatable && remoteBeta && (
              <motion.button
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.98 }}
                onClick={() => handleDownload(remoteBeta, true)}
                disabled={downloadingAlpha}
                className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 text-white text-sm font-medium shadow-lg shadow-amber-500/20 transition-all disabled:opacity-70"
              >
                {downloadingAlpha ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                <span>{downloadingAlpha ? '下载中...' : `安装 ${remoteBeta.version}`}</span>
              </motion.button>
            )}
          </div>
        </div>
        <div className="flex items-center gap-6 text-sm">
          <div className="flex items-center gap-2">
            <span className="text-[var(--text-muted)]">本地:</span>
            <span className={`font-mono ${localAlpha ? 'text-amber-400' : 'text-[var(--text-faint)]'}`}>
              {loading ? '...' : localAlpha?.version || '未安装'}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[var(--text-muted)]">最新:</span>
            <span className="font-mono text-[var(--text-secondary)]">
              {remoteBeta?.version || '-'}
            </span>
          </div>
        </div>
      </motion.div>

      {/* Info */}
      <motion.div 
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.3, delay: 0.2 }}
        className="text-xs text-[var(--text-faint)] space-y-1"
      >
        <p>内核将从 GitHub 官方仓库下载并自动安装。</p>
        <p>更新前会自动备份当前版本，支持一键回退。</p>
      </motion.div>

      {/* Toast Notifications */}
      <AnimatePresence>
        {toasts.map((toast, index) => (
          <motion.div
            key={toast.id}
            initial={{ opacity: 0, y: 50, scale: 0.9 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 20, scale: 0.9 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            style={{ bottom: 24 + index * 60 }}
            className="fixed left-1/2 -translate-x-1/2 z-[200] px-4 py-3 glass-card rounded-xl border border-[var(--glass-border)] shadow-xl flex items-center gap-3"
          >
            {toast.type === 'success' && (
              <div className="w-6 h-6 rounded-full bg-emerald-500/20 flex items-center justify-center">
                <Check className="w-4 h-4 text-emerald-400" />
              </div>
            )}
            {toast.type === 'error' && (
              <div className="w-6 h-6 rounded-full bg-red-500/20 flex items-center justify-center">
                <XCircle className="w-4 h-4 text-red-400" />
              </div>
            )}
            {toast.type === 'info' && (
              <div className="w-6 h-6 rounded-full bg-blue-500/20 flex items-center justify-center">
                <AlertCircle className="w-4 h-4 text-blue-400" />
              </div>
            )}
            <p className="text-sm font-medium text-[var(--text-primary)]">
              {toast.message}
            </p>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  )
}

function ActionButton({ 
  icon: Icon, 
  onClick, 
  loading = false,
  tooltip 
}: { 
  icon: React.ElementType
  onClick: () => void
  loading?: boolean
  tooltip?: string
}) {
  return (
    <motion.button
      whileHover={{ scale: 1.05 }}
      whileTap={{ scale: 0.95 }}
      onClick={onClick}
      disabled={loading}
      className="p-2.5 rounded-xl bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] border border-transparent hover:border-[var(--border-secondary)] transition-all disabled:opacity-50"
      title={tooltip}
    >
      <Icon className={`w-4 h-4 text-[var(--text-muted)] ${loading ? 'animate-spin' : ''}`} />
    </motion.button>
  )
}

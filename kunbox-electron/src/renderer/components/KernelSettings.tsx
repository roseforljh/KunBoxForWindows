import { useState, useEffect, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { 
  Download, RotateCcw, Trash2, ExternalLink, FolderOpen, RefreshCw, 
  Check, XCircle, AlertCircle, Loader2, CheckCircle2
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

type KernelBranch = 'stable' | 'alpha'

export function KernelSettings() {
  const [activeBranch, setActiveBranch] = useState<KernelBranch>(() => {
    return (localStorage.getItem('kunbox-kernel-branch') as KernelBranch) || 'stable'
  })
  const [localStable, setLocalStable] = useState<KernelVersion | null>(null)
  const [localAlpha, setLocalAlpha] = useState<KernelVersion | null>(null)
  const [remoteReleases, setRemoteReleases] = useState<RemoteRelease[]>([])
  const [loading, setLoading] = useState(true)
  const [downloading, setDownloading] = useState(false)
  const [canRollback, setCanRollback] = useState(false)
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
      setCanRollback(activeBranch === 'stable' ? rollbackStable : rollbackAlpha)
    } catch (err) {
      showToast('加载版本信息失败', 'error')
    } finally {
      setLoading(false)
    }
  }, [showToast, activeBranch])

  useEffect(() => {
    loadVersions()

    const unsubComplete = window.api.kernel.onDownloadComplete(() => {
      setDownloading(false)
      showToast('内核更新完成', 'success')
      loadVersions()
    })
    const unsubError = window.api.kernel.onDownloadError((err) => {
      setDownloading(false)
      showToast(`下载失败: ${err}`, 'error')
    })

    return () => {
      unsubComplete()
      unsubError()
    }
  }, [loadVersions, showToast])

  useEffect(() => {
    // Update canRollback when branch changes
    const updateRollback = async () => {
      const result = await window.api.kernel.canRollback(activeBranch === 'alpha')
      setCanRollback(result)
    }
    updateRollback()
  }, [activeBranch])

  const handleBranchChange = (branch: KernelBranch) => {
    setActiveBranch(branch)
    localStorage.setItem('kunbox-kernel-branch', branch)
    showToast(`已切换到${branch === 'stable' ? '正式版' : '测试版'}内核`, 'info')
  }

  const currentLocal = activeBranch === 'stable' ? localStable : localAlpha
  const currentRemote = remoteReleases.find(r => 
    activeBranch === 'stable' ? !r.isPrerelease : r.isPrerelease
  )
  const isUpdatable = currentRemote && currentLocal?.version !== currentRemote.version

  const handleDownload = async () => {
    if (!currentRemote) return
    setDownloading(true)
    try {
      await window.api.kernel.download(currentRemote, activeBranch === 'alpha')
    } catch (err) {
      showToast(String(err), 'error')
      setDownloading(false)
    }
  }

  const handleRollback = async () => {
    try {
      const result = await window.api.kernel.rollback(activeBranch === 'alpha')
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

  return (
    <div className="space-y-8">
      {/* Branch Selection */}
      <div>
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-4">选择内核版本</h3>
        <div className="grid grid-cols-2 gap-4">
          <BranchCard
            title="正式版"
            subtitle="Stable"
            description="推荐使用，稳定可靠"
            selected={activeBranch === 'stable'}
            onClick={() => handleBranchChange('stable')}
            color="emerald"
          />
          <BranchCard
            title="测试版"
            subtitle="Alpha"
            description="包含最新功能，可能不稳定"
            selected={activeBranch === 'alpha'}
            onClick={() => handleBranchChange('alpha')}
            color="amber"
          />
        </div>
      </div>

      {/* Current Branch Details */}
      <motion.div
        key={activeBranch}
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2 }}
        className="glass-card rounded-2xl p-6 border border-[var(--glass-border)]"
      >
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-3">
            <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${
              activeBranch === 'stable' 
                ? 'bg-emerald-500/10' 
                : 'bg-amber-500/10'
            }`}>
              {activeBranch === 'stable' 
                ? <CheckCircle2 className="w-5 h-5 text-emerald-400" />
                : <AlertCircle className="w-5 h-5 text-amber-400" />
              }
            </div>
            <div>
              <h4 className="font-semibold text-[var(--text-primary)]">
                {activeBranch === 'stable' ? '正式版' : '测试版'}内核
              </h4>
              <p className="text-xs text-[var(--text-muted)]">sing-box</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <ActionButton icon={RefreshCw} onClick={loadVersions} loading={loading} tooltip="刷新" />
            <ActionButton icon={ExternalLink} onClick={() => window.api.kernel.openReleasesPage()} tooltip="GitHub" />
            <ActionButton icon={FolderOpen} onClick={() => window.api.kernel.openDirectory()} tooltip="打开目录" />
            <ActionButton icon={Trash2} onClick={handleClearCache} tooltip="清理缓存" />
            {canRollback && (
              <ActionButton icon={RotateCcw} onClick={handleRollback} tooltip="回退版本" />
            )}
          </div>
        </div>

        {/* Version Info */}
        <div className="grid grid-cols-2 gap-4 mb-6">
          <div className="p-4 rounded-xl bg-[var(--bg-tertiary)]/50">
            <div className="text-xs text-[var(--text-muted)] mb-1">本地版本</div>
            <div className={`text-lg font-mono font-semibold ${
              currentLocal 
                ? (activeBranch === 'stable' ? 'text-emerald-400' : 'text-amber-400')
                : 'text-[var(--text-faint)]'
            }`}>
              {loading ? '...' : currentLocal?.version || '未安装'}
            </div>
          </div>
          <div className="p-4 rounded-xl bg-[var(--bg-tertiary)]/50">
            <div className="text-xs text-[var(--text-muted)] mb-1">最新版本</div>
            <div className="text-lg font-mono font-semibold text-[var(--text-primary)]">
              {currentRemote?.version || '-'}
            </div>
          </div>
        </div>

        {/* Update Button */}
        {isUpdatable && currentRemote && (
          <motion.button
            whileHover={{ scale: 1.01 }}
            whileTap={{ scale: 0.99 }}
            onClick={handleDownload}
            disabled={downloading}
            className={`w-full flex items-center justify-center gap-2 py-3 rounded-xl text-white font-medium transition-all disabled:opacity-70 ${
              activeBranch === 'stable'
                ? 'bg-gradient-to-r from-emerald-500 to-emerald-600 hover:from-emerald-400 hover:to-emerald-500 shadow-lg shadow-emerald-500/20'
                : 'bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 shadow-lg shadow-amber-500/20'
            }`}
          >
            {downloading ? (
              <>
                <Loader2 className="w-5 h-5 animate-spin" />
                <span>正在下载...</span>
              </>
            ) : (
              <>
                <Download className="w-5 h-5" />
                <span>更新到 {currentRemote.version}</span>
              </>
            )}
          </motion.button>
        )}

        {!isUpdatable && currentLocal && (
          <div className="w-full flex items-center justify-center gap-2 py-3 rounded-xl bg-[var(--bg-tertiary)]/50 text-[var(--text-muted)]">
            <Check className="w-5 h-5" />
            <span>已是最新版本</span>
          </div>
        )}

        {!currentLocal && !loading && (
          <motion.button
            whileHover={{ scale: 1.01 }}
            whileTap={{ scale: 0.99 }}
            onClick={handleDownload}
            disabled={downloading || !currentRemote}
            className={`w-full flex items-center justify-center gap-2 py-3 rounded-xl text-white font-medium transition-all disabled:opacity-70 ${
              activeBranch === 'stable'
                ? 'bg-gradient-to-r from-emerald-500 to-emerald-600 shadow-lg shadow-emerald-500/20'
                : 'bg-gradient-to-r from-amber-500 to-amber-600 shadow-lg shadow-amber-500/20'
            }`}
          >
            {downloading ? (
              <>
                <Loader2 className="w-5 h-5 animate-spin" />
                <span>正在下载...</span>
              </>
            ) : (
              <>
                <Download className="w-5 h-5" />
                <span>安装 {currentRemote?.version || '内核'}</span>
              </>
            )}
          </motion.button>
        )}

        {/* Version Details */}
        {currentLocal?.versionDetail && (
          <div className="mt-4 p-3 rounded-lg bg-[var(--bg-tertiary)]/30 text-xs font-mono text-[var(--text-faint)] whitespace-pre-wrap max-h-24 overflow-y-auto">
            {currentLocal.versionDetail}
          </div>
        )}
      </motion.div>

      {/* Tips */}
      <div className="text-xs text-[var(--text-faint)] space-y-1">
        <p>切换版本后，将使用对应分支的内核运行代理服务。</p>
        <p>更新前会自动备份当前版本，支持一键回退。</p>
      </div>

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

function BranchCard({
  title,
  subtitle,
  description,
  selected,
  onClick,
  color
}: {
  title: string
  subtitle: string
  description: string
  selected: boolean
  onClick: () => void
  color: 'emerald' | 'amber'
}) {
  const colorClasses = {
    emerald: {
      selected: 'border-emerald-500/50 bg-emerald-500/5',
      icon: 'bg-emerald-500/20',
      iconText: 'text-emerald-400',
      badge: 'bg-emerald-500/20 text-emerald-400'
    },
    amber: {
      selected: 'border-amber-500/50 bg-amber-500/5',
      icon: 'bg-amber-500/20',
      iconText: 'text-amber-400',
      badge: 'bg-amber-500/20 text-amber-400'
    }
  }
  const colors = colorClasses[color]

  return (
    <motion.button
      whileHover={{ scale: 1.02 }}
      whileTap={{ scale: 0.98 }}
      onClick={onClick}
      className={`relative p-5 rounded-2xl border-2 transition-all text-left ${
        selected 
          ? colors.selected
          : 'border-[var(--border-secondary)] bg-[var(--bg-secondary)] hover:border-[var(--border-primary)]'
      }`}
    >
      {selected && (
        <motion.div
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
          className={`absolute top-3 right-3 w-5 h-5 rounded-full ${colors.icon} flex items-center justify-center`}
        >
          <Check className={`w-3 h-3 ${colors.iconText}`} />
        </motion.div>
      )}
      <div className="flex items-center gap-2 mb-2">
        <span className="font-semibold text-[var(--text-primary)]">{title}</span>
        <span className={`text-[10px] px-1.5 py-0.5 rounded-md font-medium ${colors.badge}`}>
          {subtitle}
        </span>
      </div>
      <p className="text-xs text-[var(--text-muted)]">{description}</p>
    </motion.button>
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
      whileHover={{ scale: 1.1 }}
      whileTap={{ scale: 0.9 }}
      onClick={onClick}
      disabled={loading}
      className="p-2 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] transition-colors disabled:opacity-50"
      title={tooltip}
    >
      <Icon className={`w-4 h-4 text-[var(--text-muted)] ${loading ? 'animate-spin' : ''}`} />
    </motion.button>
  )
}

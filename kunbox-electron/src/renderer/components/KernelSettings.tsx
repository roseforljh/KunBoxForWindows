import { useState, useEffect, useCallback, useRef } from 'react'
import { isNewerRelease } from './kernel-version'
import { Download, RotateCcw, Trash2, ExternalLink, FolderOpen, RefreshCw, Loader2, Check, AlertCircle } from 'lucide-react'
import { useToast } from './ui/Toast'

interface KernelVersion { version: string; versionDetail: string }
interface RemoteRelease { version: string; tagName: string; isPrerelease?: boolean }
interface KernelCapabilities { supportsNaive: boolean; supportsIcmpProxy: boolean; supportsBypassAction: boolean }

export function KernelSettings() {
  const [local, setLocal] = useState<KernelVersion | null>(null)
  const [remote, setRemote] = useState<RemoteRelease | null>(null)
  const [capabilities, setCapabilities] = useState<KernelCapabilities | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [operation, setOperation] = useState<'download' | 'rollback' | null>(null)
  const operationRef = useRef(false)
  const [phase, setPhase] = useState<'downloading' | 'verifying' | 'installing'>('downloading')
  const [progress, setProgress] = useState(0)
  const [canRollback, setCanRollback] = useState(false)
  const { success: toastSuccess, error: toastError } = useToast()

  const loadVersions = useCallback(async () => {
    setLoading(true)
    setLoadError('')
    const [installed, rollback, caps] = await Promise.allSettled([
      window.api.kernel.getLocalVersion(), window.api.kernel.canRollback(), window.api.kernel.getCapabilities()
    ])
    if (installed.status === 'fulfilled') setLocal(installed.value)
    if (rollback.status === 'fulfilled') setCanRollback(rollback.value)
    if (caps.status === 'fulfilled') setCapabilities(caps.value)
    try {
      const releases = await window.api.kernel.getRemoteReleases(false)
      setRemote(releases[0] || null)
      if (!releases.length) setLoadError('没有可用的正式版本')
    } catch (err) {
      setRemote(null)
      setLoadError(`检查更新失败：${String(err)}`)
    } finally { setLoading(false) }
  }, [])

  useEffect(() => {
    loadVersions()
    const unsubProgress = window.api.kernel.onDownloadProgress(value => {
      if (!operationRef.current) return
      const event = value as typeof value & { phase?: 'downloading' | 'verifying' | 'installing' }
      setPhase(event.phase || 'downloading')
      setProgress(event.percent)
    })
    return () => { unsubProgress() }
  }, [loadVersions])

  const download = async () => {
    if (!remote || operationRef.current) return
    operationRef.current = true
    setOperation('download')
    setPhase('downloading')
    setProgress(0)
    try {
      const result = await window.api.kernel.download(remote.tagName)
      if (!result.success) throw new Error('更新失败')
      toastSuccess('RayBox 内核更新完成')
      await loadVersions()
    } catch (err) { toastError(`更新失败: ${String(err)}`) }
    finally { operationRef.current = false; setOperation(null) }
  }
  const rollback = async () => {
    if (operationRef.current) return
    operationRef.current = true
    setOperation('rollback')
    try {
      const result = await window.api.kernel.rollback()
      if (result.success) { toastSuccess('已回退到上一版本'); await loadVersions() }
      else toastError('回退失败')
    } catch (err) { toastError(String(err)) }
    finally { operationRef.current = false; setOperation(null) }
  }
  const clearCache = async () => {
    try {
      const result = await window.api.kernel.clearCache()
      if (result.success) toastSuccess(`缓存已清理，释放 ${(result.freedBytes / 1024 / 1024).toFixed(2)} MB`)
    } catch (err) { toastError(String(err)) }
  }
  const updateAvailable = !!remote && isNewerRelease(local?.version, remote.version)

  return (
    <div className="space-y-6">
      <div className="glass-card rounded-2xl p-6 border border-[var(--glass-border)]">
        <div className="flex items-center justify-between mb-6">
          <div><h3 className="font-semibold text-[var(--text-primary)]">RayBox 内核</h3><p className="text-xs text-[var(--text-muted)]">可信来源与完整性校验</p></div>
          <div className="flex items-center gap-2">
            <ActionButton icon={RefreshCw} onClick={loadVersions} tooltip="刷新" disabled={!!operation} />
            <ActionButton icon={ExternalLink} onClick={() => window.api.kernel.openReleasesPage()} tooltip="GitHub" />
            <ActionButton icon={FolderOpen} onClick={() => window.api.kernel.openDirectory()} tooltip="打开目录" />
            <ActionButton icon={Trash2} onClick={clearCache} tooltip="清理缓存" disabled={!!operation} />
            {canRollback && <ActionButton icon={RotateCcw} onClick={rollback} tooltip="回退版本" disabled={!!operation} />}
            {operation === 'rollback' && <span className="text-xs text-[var(--text-muted)]">正在回退…</span>}
          </div>
        </div>
        {loading ? <div className="flex justify-center py-12"><Loader2 className="w-8 h-8 animate-spin text-emerald-400" /></div> : <>
          <div className="grid grid-cols-2 gap-4 mb-6">
            <div className="p-4 rounded-xl bg-[var(--bg-tertiary)]/50"><div className="text-xs text-[var(--text-muted)]">本地版本</div><div className="text-lg font-mono">{local?.version || '未安装'}</div></div>
            <div className="p-4 rounded-xl bg-[var(--bg-tertiary)]/50"><div className="text-xs text-[var(--text-muted)]">最新正式版本</div><div className="text-lg font-mono">{remote?.version || '不可用'}</div></div>
          </div>
          {local && capabilities && <div className="mb-6 flex flex-wrap gap-4 text-xs text-[var(--text-muted)]">
            <span>NaiveProxy：{capabilities.supportsNaive ? '支持' : '不支持'}</span>
            <span>ICMP：{capabilities.supportsIcmpProxy ? '支持' : '不支持'}</span>
            <span>绕过规则：{capabilities.supportsBypassAction ? '支持' : '不支持'}</span>
          </div>}
          {loadError ? <div className="flex items-center gap-2 text-amber-400"><AlertCircle className="w-5 h-5" />{loadError}</div> :
            updateAvailable ? <button onClick={download} disabled={!!operation} className="w-full flex justify-center items-center gap-2 py-3 rounded-xl bg-emerald-600 text-white disabled:opacity-70">
              {operation === 'download' ? <><Loader2 className="w-5 h-5 animate-spin" />{phase === 'downloading' ? `正在下载 ${progress.toFixed(0)}%` : phase === 'verifying' ? '正在校验' : '正在安装'}</> : <><Download className="w-5 h-5" />{local ? '更新到' : '安装'} {remote?.version}</>}
            </button> : local && remote ? <div className="flex justify-center items-center gap-2 py-3 text-[var(--text-muted)]"><Check className="w-5 h-5" />已是最新正式版本</div> : null}
          {local?.versionDetail && <div className="mt-4 p-3 rounded-lg bg-[var(--bg-tertiary)]/30 text-xs font-mono text-[var(--text-faint)] whitespace-pre-wrap max-h-24 overflow-y-auto">{local.versionDetail}</div>}
        </>}
      </div>
      <p className="text-xs text-[var(--text-faint)]">更新前自动备份当前版本，支持回退。</p>
    </div>
  )
}

function ActionButton({ icon: Icon, onClick, tooltip, disabled }: { icon: React.ElementType; onClick: () => void; tooltip: string; disabled?: boolean }) {
  return <button onClick={onClick} disabled={disabled} className="p-2 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] disabled:opacity-50" title={tooltip}><Icon className="w-4 h-4 text-[var(--text-muted)]" /></button>
}

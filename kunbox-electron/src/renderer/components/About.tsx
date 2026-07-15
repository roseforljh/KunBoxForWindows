import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Download, Info, RefreshCw } from 'lucide-react'
import { useToast } from './ui/Toast'
import { formatBytes } from '../lib/utils'

type UpdateInfo = {
  currentVersion: string
  hasUpdate: boolean
  version?: string
  date?: string
  body?: string
}

type DownloadProgress = {
  downloaded: number
  contentLength: number
}

export default function About() {
  const toast = useToast()
  const [checking, setChecking] = useState(false)
  const [updating, setUpdating] = useState(false)
  const [progress, setProgress] = useState(0)
  const [downloadedBytes, setDownloadedBytes] = useState(0)
  const [downloadTotal, setDownloadTotal] = useState(0)
  const [downloadSpeed, setDownloadSpeed] = useState<number | null>(null)
  const [currentVersion, setCurrentVersion] = useState(__APP_VERSION__)
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null)
  const checkingRef = useRef(false)
  const lastProgressRef = useRef<{ downloaded: number; time: number } | null>(null)

  const hasUpdate = updateInfo?.hasUpdate ?? false

  const checkUpdate = useCallback(async (silent = false) => {
    if (checkingRef.current || updating) return

    checkingRef.current = true
    setChecking(true)
    try {
      const info = await window.api.updater.check()
      setCurrentVersion(info.currentVersion)
      setUpdateInfo(info)
      if (!silent) {
        if (!info.hasUpdate) {
          toast.success('当前已是最新版本')
        } else {
          toast.info(`检测到新版本 ${info.version}`)
        }
      }
    } catch (e) {
      if (!silent) {
        toast.error(`检查更新失败: ${String(e)}`)
      }
    } finally {
      checkingRef.current = false
      setChecking(false)
    }
  }, [toast, updating])

  useEffect(() => {
    let mounted = true

    window.api.updater.getCurrentVersion()
      .then((version) => {
        if (mounted && version) setCurrentVersion(version)
      })
      .catch((error) => {
        console.error('Failed to get app version:', error)
      })

    return () => {
      mounted = false
    }
  }, [])

  const installUpdate = useCallback(async () => {
    if (updating) return
    setUpdating(true)
    setProgress(0)
    setDownloadedBytes(0)
    setDownloadTotal(0)
    setDownloadSpeed(null)
    lastProgressRef.current = null
    try {
      await window.api.updater.downloadAndInstall()
      toast.success('更新包下载完成，应用将开始安装')
    } catch (e) {
      toast.error(`更新失败: ${String(e)}`)
      setUpdating(false)
    }
  }, [toast, updating])

  useEffect(() => {
    const offProgress = window.api.updater.onDownloadProgress(({ downloaded, contentLength }: DownloadProgress) => {
      const now = Date.now()
      const previous = lastProgressRef.current

      setDownloadedBytes(downloaded)
      setDownloadTotal(contentLength)

      if (previous) {
        const seconds = (now - previous.time) / 1000
        const bytes = downloaded - previous.downloaded
        if (seconds > 0 && bytes >= 0) {
          setDownloadSpeed(bytes / seconds)
        }
      }
      lastProgressRef.current = { downloaded, time: now }

      if (contentLength > 0) {
        const next = Math.min(100, (downloaded / contentLength) * 100)
        setProgress(Number.isFinite(next) ? next : 0)
      }
    })

    const offFinished = window.api.updater.onDownloadFinished(() => {
      setProgress(100)
      setDownloadSpeed(null)
      lastProgressRef.current = null
      setUpdating(false)
    })

    return () => {
      offProgress()
      offFinished()
    }
  }, [])

  const latestLabel = useMemo(() => {
    if (!updateInfo) return '-'
    return hasUpdate ? updateInfo.version || '-' : updateInfo.currentVersion
  }, [hasUpdate, updateInfo])

  return (
    <div className="h-full flex flex-col px-6 pb-6">
      <div className="flex items-center justify-between mb-10">
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">关于</h2>
          <p className="text-[var(--text-muted)] text-sm font-medium">版本信息与在线更新</p>
        </div>
      </div>

      <div className="space-y-4">
        <div className="glass-card rounded-2xl p-6">
          <div className="flex items-center gap-3 mb-4">
            <Info className="w-5 h-5 text-[var(--accent-primary)]" />
            <h3 className="text-lg font-semibold text-[var(--text-primary)]">KunBox</h3>
          </div>
          <div className="space-y-2 text-sm">
            <div className="flex justify-between">
              <span className="text-[var(--text-muted)]">当前版本</span>
              <span className="text-[var(--text-primary)]">{currentVersion || '-'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-[var(--text-muted)]">最新版本</span>
              <span className="text-[var(--text-primary)]">{latestLabel}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-[var(--text-muted)]">更新源</span>
              <span className="text-[var(--text-primary)]">GitHub Releases</span>
            </div>
          </div>
        </div>

        <div className="glass-card rounded-2xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <button
              onClick={() => {
                void checkUpdate(false)
              }}
              disabled={checking || updating}
              className="glass-btn h-11 px-5 rounded-xl text-sm font-medium flex items-center gap-2 disabled:opacity-50"
            >
              <RefreshCw className={`w-4 h-4 ${checking ? 'animate-spin' : ''}`} />
              检查更新
            </button>

            <button
              onClick={installUpdate}
              disabled={!hasUpdate || checking || updating}
              className="h-11 px-5 rounded-xl text-sm font-medium flex items-center gap-2 text-white bg-[var(--accent-primary)] hover:opacity-90 disabled:opacity-50"
            >
              <Download className={`w-4 h-4 ${updating ? 'animate-bounce' : ''}`} />
              一键更新
            </button>
          </div>

          {updating && (
            <div className="space-y-2">
              <div className="w-full h-2 rounded-full bg-[var(--bg-tertiary)] overflow-hidden">
                <div className="h-full bg-[var(--accent-primary)] transition-all" style={{ width: `${Math.max(1, Math.round(progress))}%` }} />
              </div>
              <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                <span>下载中 {Math.round(progress)}%</span>
                <span>
                  {downloadSpeed == null ? '计算网速中' : `${formatBytes(downloadSpeed)}/s`}
                  {downloadedBytes > 0 && (
                    <>
                      {' · '}
                      {formatBytes(downloadedBytes)}
                      {downloadTotal > 0 ? ` / ${formatBytes(downloadTotal)}` : ''}
                    </>
                  )}
                </span>
              </div>
            </div>
          )}

          {hasUpdate && updateInfo?.body && (
            <div className="rounded-xl bg-[var(--bg-tertiary)]/40 p-3 text-xs text-[var(--text-secondary)] whitespace-pre-wrap max-h-40 overflow-auto">
              {updateInfo.body}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

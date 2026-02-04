import { useState, useEffect, useRef } from 'react'
import { Trash2, FileText } from 'lucide-react'
import type { LogEntry } from '@shared/types'

export default function Logs() {
  const [logs, setLogs] = useState<LogEntry[]>([])
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const unsubscribe = window.api.singbox.onLog((entry) => {
      setLogs((prev) => [...prev.slice(-499), entry])
    })

    return () => unsubscribe()
  }, [])

  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight
    }
  }, [logs])

  const getLevelColor = (level: string) => {
    switch (level) {
      case 'error': return 'text-status-error'
      case 'warn': return 'text-status-warning'
      case 'info': return 'text-status-success'
      default: return 'text-text-muted'
    }
  }

  const formatTime = (timestamp: number) => {
    return new Date(timestamp).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    })
  }

  return (
    <div className="h-full flex flex-col px-6 pb-6">
      <div className="flex items-center justify-between mb-10">
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">日志</h2>
          <p className="text-[var(--text-muted)] text-sm font-medium">查看代理核心运行日志</p>
        </div>
        <button
          onClick={() => setLogs([])}
          className="glass-btn h-11 px-5 rounded-xl text-sm font-medium flex items-center gap-2"
        >
          <Trash2 className="w-4 h-4" />
          清空
        </button>
      </div>

      <div
        ref={containerRef}
        className="flex-1 glass-card p-5 rounded-2xl overflow-auto font-mono text-xs"
      >
        {logs.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center">
            <div className="w-16 h-16 rounded-2xl bg-[var(--bg-elevated)] flex items-center justify-center mb-3">
              <FileText className="w-7 h-7 text-[var(--text-faint)]" />
            </div>
            <p className="text-[var(--text-faint)] font-sans text-sm">暂无日志</p>
          </div>
        ) : (
          logs.map((log, i) => (
            <div key={i} className="py-1.5 flex gap-3 hover:bg-[var(--bg-hover)] rounded px-2 -mx-2">
              <span className="text-[var(--text-faint)] shrink-0">{formatTime(log.timestamp)}</span>
              <span className={`shrink-0 uppercase font-bold ${getLevelColor(log.level)}`}>
                [{log.level.padEnd(5)}]
              </span>
              <span className="text-[var(--text-secondary)] shrink-0">[{log.tag}]</span>
              <span className="text-[var(--text-primary)] break-all">{log.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

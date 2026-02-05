import { useState, useEffect, useRef } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Download, Upload, Wifi, Server, Play, Square, RotateCw, Zap, Loader2, AlertTriangle } from 'lucide-react'
import { useConnectionStore } from '../stores/connectionStore'
import { useNodesStore } from '../stores/nodesStore'
import { formatBytes, formatDuration, cn } from '../lib/utils'
import { useToast } from './ui/Toast'
import type { Profile } from '@shared/types'

export default function Dashboard() {
  const { state, traffic, connect, disconnect, needsRestart, setNeedsRestart } = useConnectionStore()
  const { nodes, activeNodeTag, loadNodes, testNodeLatency } = useNodesStore()
  const [isAnimating, setIsAnimating] = useState(false)
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [isTesting, setIsTesting] = useState(false)
  const [trafficHistory, setTrafficHistory] = useState<{ download: number; upload: number }[]>([])
  const toast = useToast()

  const isOn = state === 'connected'
  const isConnecting = state === 'connecting'
  const isDisconnecting = state === 'disconnecting'
  const isConnected = isOn

  // Load profiles and nodes on mount
  useEffect(() => {
    loadProfiles()
    loadNodes()
  }, [loadNodes])

  // Update traffic history for chart
  useEffect(() => {
    if (isConnected && traffic) {
      setTrafficHistory(prev => {
        const newHistory = [...prev, { download: traffic.downloadSpeed, upload: traffic.uploadSpeed }]
        // Keep last 20 data points
        return newHistory.slice(-20)
      })
    } else {
      setTrafficHistory([])
    }
  }, [isConnected, traffic?.downloadSpeed, traffic?.uploadSpeed])

  const loadProfiles = async () => {
    try {
      const list = await window.api.profile.list()
      setProfiles(list)
    } catch (error) {
      console.error('Failed to load profiles:', error)
    }
  }

  // Get active node info
  const activeNode = nodes.find(n => n.tag === activeNodeTag)
  const activeProfile = profiles.find(p => p.enabled)
  const currentLatency = activeNode?.latencyMs ?? null

  // Calculate total nodes count from all enabled profiles
  const totalNodes = nodes.length
  const enabledProfilesCount = profiles.filter(p => p.enabled).length

  const handleToggle = async () => {
    if (isAnimating || isConnecting || isDisconnecting) return
    
    setIsAnimating(true)
    try {
      if (isOn) {
        const result = await disconnect()
        if (result.success) {
          toast.info('已断开连接')
        } else {
          toast.error(result.error || '断开失败')
        }
      } else {
        const result = await connect()
        if (result.success) {
          toast.success('已连接')
          // Auto test latency after connect
          setTimeout(() => testLatency(), 1000)
        } else {
          toast.error(result.error || '连接失败')
        }
      }
    } catch (err) {
      toast.error(`操作失败: ${err}`)
    } finally {
      setIsAnimating(false)
    }
  }

  const handleRestart = async () => {
    if (isAnimating || !isOn) return
    
    setIsAnimating(true)
    try {
      const stopResult = await disconnect()
      if (!stopResult.success) {
        toast.error(stopResult.error || '停止失败')
        return
      }
      await new Promise(resolve => setTimeout(resolve, 500))
      const startResult = await connect()
      if (startResult.success) {
        toast.success('已重新连接')
        testLatency()
      } else {
        toast.error(startResult.error || '重启失败')
      }
    } catch (err) {
      toast.error(`重启失败: ${err}`)
    } finally {
      setIsAnimating(false)
    }
  }

  const testLatency = async () => {
    if (isTesting || !activeNodeTag) return
    
    setIsTesting(true)
    try {
      await testNodeLatency(activeNodeTag)
    } catch {
      // Error handled in store
    } finally {
      setIsTesting(false)
    }
  }

  // Auto test latency when connected and node has no latency
  useEffect(() => {
    if (isConnected && activeNodeTag && currentLatency === null && !isTesting) {
      const timer = setTimeout(() => testLatency(), 1000)
      return () => clearTimeout(timer)
    }
  }, [isConnected, activeNodeTag, currentLatency])

  // Show error toast when state changes to error
  useEffect(() => {
    if (state === 'error') {
      toast.error('连接出错，请检查配置')
    }
  }, [state, toast])

  // Calculate health score based on latency
  const getHealthScore = () => {
    if (!isConnected) return 0
    if (currentLatency === null) return 80
    if (currentLatency < 100) return 95
    if (currentLatency < 200) return 85
    if (currentLatency < 500) return 70
    return 50
  }

  const healthScore = getHealthScore()

  // Get latency color
  const getLatencyColor = (latency: number | null) => {
    if (latency === null) return 'text-[var(--text-muted)]'
    if (latency < 100) return 'text-emerald-500'
    if (latency < 300) return 'text-amber-500'
    return 'text-red-500'
  }

  return (
    <div className="space-y-6 px-6 pb-6">
      {/* Restart Required Banner */}
      <AnimatePresence>
        {needsRestart && isConnected && (
          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            className="flex items-center justify-between p-3 rounded-xl bg-amber-500/15 border border-amber-500/30"
          >
            <div className="flex items-center gap-3">
              <AlertTriangle className="w-5 h-5 text-amber-500" />
              <span className="text-sm text-amber-500">配置已修改，需要重启 VPN 才能生效</span>
            </div>
            <button
              onClick={handleRestart}
              className="px-3 py-1.5 text-xs font-medium rounded-lg bg-amber-500/20 text-amber-500 hover:bg-amber-500/30 transition-colors"
            >
              立即重启
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-[var(--text-primary)]">
            仪表盘
          </h1>
          <p className="text-sm text-[var(--text-muted)] mt-1">
            连接状态与流量监控
          </p>
        </div>
        <div className="flex items-center gap-3">
          <ProxyToggle 
            isOn={isOn} 
            isLoading={isAnimating} 
            onToggle={handleToggle}
            onRestart={handleRestart}
          />
        </div>
      </div>

      <div className="glass-card p-6">
        <div className="flex flex-col lg:flex-row items-center gap-8">
          <div className="flex items-center gap-6">
            <HealthGauge score={healthScore} label="连接质量" />
            <div className="hidden sm:flex flex-col gap-2">
              <ProgressBar 
                value={isConnected ? (currentLatency && currentLatency < 200 ? 98 : 80) : 0} 
                label="可用性" 
                color="var(--accent-primary)" 
              />
              <ProgressBar 
                value={isConnected ? 85 : 0} 
                label="稳定性" 
                color="var(--accent-secondary)" 
              />
              <ProgressBar 
                value={isConnected ? (currentLatency ? Math.max(0, 100 - currentLatency / 5) : 80) : 0} 
                label="速度" 
                color="var(--accent-tertiary)" 
              />
            </div>
          </div>

          <div className="hidden lg:block w-px h-24 bg-[var(--border-primary)]" />

          <div className="flex-1 grid grid-cols-2 lg:grid-cols-4 gap-4">
            <div className="text-center cursor-pointer" onClick={() => isConnected && testLatency()}>
              <div className={`text-3xl font-bold ${isConnected ? getLatencyColor(currentLatency) : 'text-[var(--text-primary)]'}`}>
                {isTesting ? (
                  <Loader2 className="w-8 h-8 animate-spin mx-auto" />
                ) : isConnected ? (
                  currentLatency !== null ? (
                    <>{currentLatency}<span className="text-base font-normal">ms</span></>
                  ) : (
                    <span className="text-lg">测试中...</span>
                  )
                ) : '-'}
              </div>
              <div className="text-xs text-[var(--text-muted)] mt-1 flex items-center justify-center gap-1">
                <Zap className="w-3 h-3" />
                延迟
              </div>
            </div>
            <StatValue 
              value={isConnected ? formatDuration(traffic?.duration || 0) : '-'} 
              label="运行时间" 
            />
            <StatValue 
              value={String(totalNodes)} 
              label="可用节点" 
            />
            <StatValue 
              value={String(profiles.length)} 
              label="订阅配置" 
            />
          </div>
        </div>
      </div>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <MetricCard
          icon={<Server className="w-5 h-5 text-[var(--accent-primary)]" />}
          title="当前节点"
          value={activeNode?.tag || '未选择'}
          subtitle={currentLatency !== null ? `延迟 ${currentLatency}ms` : (activeNode?.server || '请选择节点')}
          iconBg="bg-[rgba(20,184,166,0.15)]"
        />
        <MetricCard
          icon={<Wifi className="w-5 h-5 text-rose-500" />}
          title="配置文件"
          value={activeProfile?.name || '未选择'}
          subtitle={activeProfile ? `${activeProfile.nodeCount || 0} 个节点` : '请添加订阅'}
          iconBg="bg-rose-500/15"
        />
        <MetricCard
          icon={<Download className="w-5 h-5 text-emerald-500" />}
          title="下载速度"
          value={isConnected && traffic ? formatBytes(traffic.downloadSpeed) + '/s' : '0 B/s'}
          subtitle={isConnected && traffic ? `总计 ${formatBytes(traffic.downloadTotal)}` : '等待连接'}
          iconBg="bg-emerald-500/15"
          valueColor={isConnected ? 'text-emerald-500' : undefined}
        />
        <MetricCard
          icon={<Upload className="w-5 h-5 text-[var(--accent-tertiary)]" />}
          title="上传速度"
          value={isConnected && traffic ? formatBytes(traffic.uploadSpeed) + '/s' : '0 B/s'}
          subtitle={isConnected && traffic ? `总计 ${formatBytes(traffic.uploadTotal)}` : '等待连接'}
          iconBg="bg-[rgba(212,184,150,0.15)]"
          valueColor={isConnected ? 'text-[var(--accent-tertiary)]' : undefined}
        />
      </div>

      <div className="glass-card p-5">
        <div className="flex items-center justify-between mb-4">
          <div>
            <div className="text-xs tracking-wider text-[var(--text-dim)] uppercase">
              流量趋势
            </div>
            <h3 className="text-lg font-semibold text-[var(--text-primary)] mt-1">
              实时监控
            </h3>
          </div>
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full bg-emerald-500" />
              <span className="text-xs text-[var(--text-muted)]">下载</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full bg-amber-500" />
              <span className="text-xs text-[var(--text-muted)]">上传</span>
            </div>
          </div>
        </div>
        <TrafficChart data={trafficHistory} />
        <div className="flex items-center justify-between mt-4 pt-4 border-t border-[var(--border-secondary)]">
          <TrafficItem
            icon={Download}
            label="下载"
            value={isConnected && traffic ? formatBytes(traffic.downloadSpeed) + '/s' : '0 B/s'}
            color="text-emerald-500"
          />
          <TrafficItem
            icon={Upload}
            label="上传"
            value={isConnected && traffic ? formatBytes(traffic.uploadSpeed) + '/s' : '0 B/s'}
            color="text-amber-500"
          />
          <div className="text-right">
            <p className="text-xs text-[var(--text-muted)]">总流量</p>
            <p className="text-xl font-bold text-[var(--text-primary)]">
              {isConnected && traffic 
                ? formatBytes(traffic.downloadTotal + traffic.uploadTotal)
                : '0 B'}
            </p>
          </div>
        </div>
      </div>
    </div>
  )
}

function ProxyToggle({ 
  isOn, 
  isLoading, 
  onToggle,
  onRestart
}: { 
  isOn: boolean
  isLoading: boolean
  onToggle: () => void
  onRestart: () => void
}) {
  const [isRestarting, setIsRestarting] = useState(false)

  const handleRestart = async () => {
    if (isRestarting || isLoading || !isOn) return
    setIsRestarting(true)
    await onRestart()
    setIsRestarting(false)
  }

  return (
    <div className="flex items-center gap-2">
      <AnimatePresence>
        {isOn && (
          <motion.button
            initial={{ opacity: 0, scale: 0.8, x: 10 }}
            animate={{ opacity: 1, scale: 1, x: 0 }}
            exit={{ opacity: 0, scale: 0.8, x: 10 }}
            transition={{ 
              type: 'spring',
              stiffness: 400,
              damping: 25,
              mass: 0.8
            }}
            onClick={handleRestart}
            disabled={isLoading || isRestarting}
            whileHover={{ scale: 1.1 }}
            whileTap={{ scale: 0.9 }}
            className={cn(
              'w-8 h-8 flex items-center justify-center rounded-full',
              'border backdrop-blur-sm',
              'disabled:opacity-50 disabled:cursor-not-allowed',
              'bg-[var(--glass-bg)] border-[var(--glass-border)]',
              'text-[var(--text-secondary)]',
              'hover:text-[var(--accent-primary)] hover:border-[var(--accent-primary)]/40',
              'hover:bg-[color-mix(in_srgb,var(--accent-primary)_8%,transparent)]',
              'transition-colors duration-200'
            )}
            title="重启代理"
          >
            <motion.div
              animate={isRestarting ? { rotate: 360 } : { rotate: 0 }}
              transition={isRestarting 
                ? { duration: 0.6, repeat: Infinity, ease: 'linear' } 
                : { duration: 0.3 }
              }
            >
              <RotateCw className="w-3.5 h-3.5" />
            </motion.div>
          </motion.button>
        )}
      </AnimatePresence>

      <div
        className={cn(
          'flex items-center gap-2 pl-3 pr-1.5 py-1.5 rounded-full transition-all duration-300',
          'border backdrop-blur-sm',
          isOn
            ? 'proxy-toggle-active'
            : 'bg-[var(--glass-bg)] border-[var(--glass-border)]'
        )}
      >
        <div
          className={cn(
            'w-2 h-2 rounded-full transition-all duration-300',
            isLoading
              ? 'bg-[var(--warning)] animate-pulse'
              : isOn
                ? 'bg-[var(--accent-primary)] shadow-[0_0_8px_var(--accent-primary)]'
                : 'bg-[var(--text-dim)]'
          )}
        />
        
        <span
          className={cn(
            'text-xs font-medium font-mono transition-colors duration-300',
            isOn
              ? 'text-[var(--accent-primary)]'
              : 'text-[var(--text-muted)]'
          )}
        >
          127.0.0.1:7890
        </span>
        
        <motion.button
          onClick={onToggle}
          disabled={isLoading}
          whileHover={{ scale: 1.08 }}
          whileTap={{ scale: 0.92 }}
          className={cn(
            'w-7 h-7 flex items-center justify-center rounded-full transition-all duration-200',
            'disabled:opacity-50 disabled:cursor-not-allowed',
            isOn
              ? 'proxy-toggle-btn-stop'
              : 'proxy-toggle-btn-start'
          )}
        >
          {isLoading ? (
            <motion.div
              animate={{ rotate: 360 }}
              transition={{ duration: 0.8, repeat: Infinity, ease: 'linear' }}
              className="w-3 h-3 border-2 border-current border-t-transparent rounded-full"
            />
          ) : (
            <motion.div
              key={isOn ? 'stop' : 'play'}
              initial={{ scale: 0.5, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={{ type: 'spring', stiffness: 400, damping: 25 }}
            >
              {isOn ? (
                <Square className="w-3 h-3" fill="currentColor" />
              ) : (
                <Play className="w-3.5 h-3.5 ml-0.5" fill="currentColor" />
              )}
            </motion.div>
          )}
        </motion.button>
      </div>
    </div>
  )
}

function HealthGauge({ score, label }: { score: number; label: string }) {
  const color = score >= 80 ? 'var(--accent-primary)' : score >= 50 ? '#f59e0b' : '#ef4444'
  const circumference = 2 * Math.PI * 40
  const strokeDashoffset = circumference - (score / 100) * circumference

  return (
    <div className="flex flex-col items-center">
      <div className="relative w-24 h-24">
        <svg className="w-24 h-24 -rotate-90" viewBox="0 0 100 100">
          <circle
            cx="50"
            cy="50"
            r="40"
            fill="none"
            stroke="var(--border-primary)"
            strokeWidth="8"
          />
          <circle
            cx="50"
            cy="50"
            r="40"
            fill="none"
            stroke={color}
            strokeWidth="8"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={strokeDashoffset}
            className="transition-all duration-500"
          />
        </svg>
        <div className="absolute inset-0 flex items-center justify-center">
          <span className="text-2xl font-bold text-[var(--text-primary)]">
            {score}
          </span>
        </div>
      </div>
      <span className="text-xs text-[var(--text-muted)] mt-2">{label}</span>
    </div>
  )
}

function ProgressBar({ value, label, color }: { value: number; label: string; color: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="w-16 h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{ width: `${value}%`, backgroundColor: color }}
        />
      </div>
      <span className="text-xs text-[var(--text-muted)]">
        {label} {value}%
      </span>
    </div>
  )
}

function StatValue({ value, unit, label, accent }: { value: string; unit?: string; label: string; accent?: boolean }) {
  return (
    <div className="text-center">
      <div className={`text-3xl font-bold ${accent ? 'text-[var(--accent-primary)]' : 'text-[var(--text-primary)]'}`}>
        {value}
        {unit && <span className="text-base font-normal">{unit}</span>}
      </div>
      <div className="text-xs text-[var(--text-muted)] mt-1">
        {label}
      </div>
    </div>
  )
}

function MetricCard({
  icon,
  title,
  value,
  subtitle,
  iconBg = 'bg-[var(--bg-tertiary)]',
  valueColor,
}: {
  icon: React.ReactNode
  title: string
  value: string
  subtitle?: string
  iconBg?: string
  valueColor?: string
}) {
  return (
    <div className="glass-card glass-card-hover p-4">
      <div className="flex items-start justify-between mb-3">
        <div className={`p-2 rounded-xl ${iconBg}`}>
          {icon}
        </div>
      </div>
      <div className="text-xs text-[var(--text-muted)] mb-1">{title}</div>
      <div className="flex items-baseline gap-1">
        <span className={`text-lg font-semibold truncate ${valueColor || 'text-[var(--text-primary)]'}`}>
          {value}
        </span>
      </div>
      {subtitle && (
        <div className="text-xs text-[var(--text-dim)] mt-1">{subtitle}</div>
      )}
    </div>
  )
}

function TrafficItem({ 
  icon: Icon, 
  label, 
  value,
  color = 'text-[var(--accent-primary)]'
}: { 
  icon: typeof Download
  label: string
  value: string
  color?: string
}) {
  return (
    <div className="flex items-center gap-4">
      <div
        className="w-12 h-12 rounded-2xl flex items-center justify-center"
        style={{ background: 'var(--bg-elevated)' }}
      >
        <Icon className={`w-5 h-5 ${color}`} />
      </div>
      <div>
        <p
          className="text-[11px] font-bold uppercase tracking-[0.15em] mb-0.5"
          style={{ color: 'var(--text-muted)', opacity: 0.6 }}
        >
          {label}
        </p>
        <p className="text-xl font-bold font-mono" style={{ color: 'var(--text-primary)' }}>
          {value}
        </p>
      </div>
    </div>
  )
}

function TrafficChart({ data }: { data: { download: number; upload: number }[] }) {
  // Use real data or show empty state
  const downloadData = data.length > 0 ? data.map(d => d.download) : Array(20).fill(0)
  const uploadData = data.length > 0 ? data.map(d => d.upload) : Array(20).fill(0)
  
  const maxValue = Math.max(...downloadData, ...uploadData, 1) * 1.1
  const height = 120
  const width = 100
  
  const createPath = (values: number[]) => {
    if (values.length < 2) return ''
    
    const points = values.map((value, index) => ({
      x: (index / (values.length - 1)) * width,
      y: height - (value / maxValue) * height
    }))
    
    let path = `M ${points[0].x} ${points[0].y}`
    for (let i = 0; i < points.length - 1; i++) {
      const current = points[i]
      const next = points[i + 1]
      const controlX = (current.x + next.x) / 2
      path += ` C ${controlX} ${current.y}, ${controlX} ${next.y}, ${next.x} ${next.y}`
    }
    return path
  }
  
  const createAreaPath = (values: number[]) => {
    const linePath = createPath(values)
    if (!linePath) return ''
    
    const points = values.map((value, index) => ({
      x: (index / (values.length - 1)) * width,
      y: height - (value / maxValue) * height
    }))
    return linePath + ` L ${points[points.length - 1].x} ${height} L ${points[0].x} ${height} Z`
  }

  return (
    <div className="relative h-32">
      <svg 
        viewBox={`0 0 ${width} ${height}`} 
        className="w-full h-full"
        preserveAspectRatio="none"
      >
        <defs>
          <linearGradient id="downloadGradient" x1="0%" y1="0%" x2="0%" y2="100%">
            <stop offset="0%" stopColor="rgba(16, 185, 129, 0.3)" />
            <stop offset="100%" stopColor="rgba(16, 185, 129, 0)" />
          </linearGradient>
          <linearGradient id="uploadGradient" x1="0%" y1="0%" x2="0%" y2="100%">
            <stop offset="0%" stopColor="rgba(245, 158, 11, 0.2)" />
            <stop offset="100%" stopColor="rgba(245, 158, 11, 0)" />
          </linearGradient>
        </defs>
        
        {[0, 0.25, 0.5, 0.75, 1].map((ratio) => (
          <line
            key={ratio}
            x1="0"
            y1={height * ratio}
            x2={width}
            y2={height * ratio}
            stroke="var(--border-secondary)"
            strokeWidth="0.3"
            strokeDasharray="2,2"
          />
        ))}
        
        <path d={createAreaPath(downloadData)} fill="url(#downloadGradient)" />
        <path 
          d={createPath(downloadData)} 
          fill="none" 
          stroke="#10b981" 
          strokeWidth="1.5"
          strokeLinecap="round"
        />
        
        <path d={createAreaPath(uploadData)} fill="url(#uploadGradient)" />
        <path 
          d={createPath(uploadData)} 
          fill="none" 
          stroke="#f59e0b" 
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
      
      {data.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center">
          <span className="text-xs text-[var(--text-dim)]">等待连接...</span>
        </div>
      )}
    </div>
  )
}

import { useState } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Download, Upload, Wifi, Server, Activity, Play, Square, RotateCw, BarChart3, ArrowUpRight, ArrowDownRight } from 'lucide-react'
import { useConnectionStore } from '../stores/connectionStore'
import { formatBytes, formatDuration, cn } from '../lib/utils'

export default function Dashboard() {
  const { traffic, currentNodeName, currentProfileName } = useConnectionStore()
  const [isOn, setIsOn] = useState(false)
  const [isAnimating, setIsAnimating] = useState(false)

  const isConnected = isOn

  const handleToggle = async () => {
    if (isAnimating) return
    
    setIsAnimating(true)
    await new Promise(resolve => setTimeout(resolve, 800))
    setIsOn(!isOn)
    setIsAnimating(false)
  }

  const handleRestart = async () => {
    if (isAnimating || !isOn) return
    
    setIsAnimating(true)
    await new Promise(resolve => setTimeout(resolve, 600))
    setIsAnimating(false)
  }

  const healthScore = isConnected ? 95 : 0

  const nodeTrafficData = [
    { name: 'Hong Kong 01', download: 2.4 * 1024 * 1024 * 1024, upload: 512 * 1024 * 1024, lastUsed: '刚刚', color: 'var(--accent-primary)' },
    { name: 'Singapore 02', download: 1.8 * 1024 * 1024 * 1024, upload: 320 * 1024 * 1024, lastUsed: '2小时前', color: '#8b5cf6' },
    { name: 'Japan 03', download: 956 * 1024 * 1024, upload: 128 * 1024 * 1024, lastUsed: '昨天', color: '#f59e0b' },
    { name: 'US West 01', download: 512 * 1024 * 1024, upload: 64 * 1024 * 1024, lastUsed: '3天前', color: '#ec4899' },
    { name: 'Taiwan 01', download: 256 * 1024 * 1024, upload: 32 * 1024 * 1024, lastUsed: '上周', color: '#06b6d4' },
  ]

  const totalNodeTraffic = nodeTrafficData.reduce((acc, node) => acc + node.download + node.upload, 0)
  const maxNodeTraffic = Math.max(...nodeTrafficData.map(n => n.download + n.upload))

  return (
    <div className="space-y-6 px-6 pb-6">
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
                value={isConnected ? 98 : 0} 
                label="可用性" 
                color="var(--accent-primary)" 
              />
              <ProgressBar 
                value={isConnected ? 85 : 0} 
                label="稳定性" 
                color="var(--accent-secondary)" 
              />
              <ProgressBar 
                value={isConnected ? 92 : 0} 
                label="速度" 
                color="var(--accent-tertiary)" 
              />
            </div>
          </div>

          <div className="hidden lg:block w-px h-24 bg-[var(--border-primary)]" />

          <div className="flex-1 grid grid-cols-2 lg:grid-cols-4 gap-4">
            <StatValue 
              value={isConnected ? '45' : '-'} 
              unit="ms" 
              label="延迟" 
              accent={isConnected} 
            />
            <StatValue 
              value={isConnected ? formatDuration(traffic?.duration || 0) : '-'} 
              label="运行时间" 
            />
            <StatValue 
              value="12" 
              label="可用节点" 
            />
            <StatValue 
              value="3" 
              label="订阅配置" 
            />
          </div>
        </div>
      </div>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <MetricCard
          icon={<Server className="w-5 h-5 text-[var(--accent-primary)]" />}
          title="当前节点"
          value={currentNodeName || 'Hong Kong 01'}
          subtitle="延迟 45ms"
          iconBg="bg-[rgba(20,184,166,0.15)]"
        />
        <MetricCard
          icon={<Wifi className="w-5 h-5 text-rose-500" />}
          title="配置文件"
          value={currentProfileName || 'Default'}
          subtitle="12 个节点"
          iconBg="bg-rose-500/15"
        />
        <MetricCard
          icon={<Download className="w-5 h-5 text-emerald-500" />}
          title="下载速度"
          value={isConnected && traffic ? formatBytes(traffic.downloadSpeed) + '/s' : '0 B/s'}
          subtitle={isConnected && traffic ? `总计 ${formatBytes(traffic.totalDownload)}` : '等待连接'}
          iconBg="bg-emerald-500/15"
          valueColor={isConnected ? 'text-emerald-500' : undefined}
        />
        <MetricCard
          icon={<Upload className="w-5 h-5 text-[var(--accent-tertiary)]" />}
          title="上传速度"
          value={isConnected && traffic ? formatBytes(traffic.uploadSpeed) + '/s' : '0 B/s'}
          subtitle={isConnected && traffic ? `总计 ${formatBytes(traffic.totalUpload)}` : '等待连接'}
          iconBg="bg-[rgba(212,184,150,0.15)]"
          valueColor={isConnected ? 'text-[var(--accent-tertiary)]' : undefined}
        />
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2 glass-card p-5">
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
          <TrafficChart />
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
                {formatBytes(totalNodeTraffic)}
              </p>
            </div>
          </div>
        </div>

        <div className="glass-card p-5">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2">
              <BarChart3 className="w-4 h-4 text-[var(--text-muted)]" />
              <span className="text-sm font-medium text-[var(--text-primary)]">
                节点流量
              </span>
            </div>
            <span className="text-xs text-[var(--text-dim)]">
              {nodeTrafficData.length} 个节点
            </span>
          </div>
          <div className="space-y-3">
            {nodeTrafficData.map((node, index) => (
              <NodeTrafficRow
                key={node.name}
                rank={index + 1}
                name={node.name}
                download={node.download}
                upload={node.upload}
                lastUsed={node.lastUsed}
                color={node.color}
                percentage={(node.download + node.upload) / maxNodeTraffic * 100}
              />
            ))}
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

function NodeTrafficRow({
  rank,
  name,
  download,
  upload,
  lastUsed,
  color,
  percentage
}: {
  rank: number
  name: string
  download: number
  upload: number
  lastUsed: string
  color: string
  percentage: number
}) {
  return (
    <div className="group">
      <div className="flex items-center gap-3 mb-2">
        <span 
          className="w-5 h-5 rounded-md flex items-center justify-center text-xs font-bold"
          style={{ 
            background: `color-mix(in srgb, ${color} 15%, transparent)`,
            color: color
          }}
        >
          {rank}
        </span>
        <span className="text-sm font-medium text-[var(--text-primary)] flex-1 truncate">
          {name}
        </span>
        <div className="flex items-center gap-4 text-xs">
          <span className="flex items-center gap-1 text-emerald-500">
            <ArrowDownRight className="w-3 h-3" />
            {formatBytes(download)}
          </span>
          <span className="flex items-center gap-1 text-amber-500">
            <ArrowUpRight className="w-3 h-3" />
            {formatBytes(upload)}
          </span>
          <span className="text-[var(--text-dim)] w-16 text-right">
            {lastUsed}
          </span>
        </div>
      </div>
      <div className="ml-8 h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
        <motion.div
          className="h-full rounded-full"
          style={{ backgroundColor: color }}
          initial={{ width: 0 }}
          animate={{ width: `${percentage}%` }}
          transition={{ duration: 0.5, ease: 'easeOut' }}
        />
      </div>
    </div>
  )
}

function TrafficChart() {
  const downloadData = [120, 180, 150, 220, 280, 240, 320, 380, 350, 420, 480, 450]
  const uploadData = [40, 60, 50, 80, 100, 90, 120, 140, 130, 160, 180, 170]
  const labels = ['00:00', '02:00', '04:00', '06:00', '08:00', '10:00', '12:00', '14:00', '16:00', '18:00', '20:00', '22:00']
  
  const maxValue = Math.max(...downloadData, ...uploadData) * 1.1
  const height = 120
  const width = 100
  
  const createPath = (data: number[]) => {
    const points = data.map((value, index) => ({
      x: (index / (data.length - 1)) * width,
      y: height - (value / maxValue) * height
    }))
    
    if (points.length < 2) return ''
    
    let path = `M ${points[0].x} ${points[0].y}`
    for (let i = 0; i < points.length - 1; i++) {
      const current = points[i]
      const next = points[i + 1]
      const controlX = (current.x + next.x) / 2
      path += ` C ${controlX} ${current.y}, ${controlX} ${next.y}, ${next.x} ${next.y}`
    }
    return path
  }
  
  const createAreaPath = (data: number[]) => {
    const linePath = createPath(data)
    const points = data.map((value, index) => ({
      x: (index / (data.length - 1)) * width,
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
      
      <div className="absolute bottom-0 left-0 right-0 flex justify-between text-[10px] text-[var(--text-dim)]">
        {labels.filter((_, i) => i % 3 === 0).map((label) => (
          <span key={label}>{label}</span>
        ))}
      </div>
    </div>
  )
}

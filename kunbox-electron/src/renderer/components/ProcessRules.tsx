import { useState, useEffect, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import * as Switch from '@radix-ui/react-switch'
import {
  Cpu,
  Plus,
  Globe,
  Zap,
  Ban,
  Edit2,
  Trash2,
  Server,
  FileText,
  Check,
  XCircle,
  AlertCircle,
  Loader2,
  Sparkles,
  AlertTriangle,
  ArrowRight
} from 'lucide-react'
import { Modal, ModalButton } from './ui/Modal'
import { useNodesStore } from '../stores/nodesStore'
import type { Profile, AppSettings } from '@shared/types'

interface ProcessRulesProps {
  onNavigate?: (page: string, tab?: string) => void
}

const fastTransition = { duration: 0.15, ease: [0.4, 0, 0.2, 1] }

type OutboundMode = 'direct' | 'proxy' | 'block' | 'node' | 'profile'

interface ProcessRule {
  id: string
  processName: string
  outboundMode: OutboundMode
  outboundValue?: string
  enabled: boolean
}

interface ToastMessage {
  id: number
  message: string
  type: 'success' | 'error' | 'info'
}

const commonProcesses = [
  { name: 'chrome.exe', desc: 'Google Chrome 浏览器' },
  { name: 'msedge.exe', desc: 'Microsoft Edge 浏览器' },
  { name: 'firefox.exe', desc: 'Mozilla Firefox 浏览器' },
  { name: 'Telegram.exe', desc: 'Telegram 即时通讯' },
  { name: 'Discord.exe', desc: 'Discord 语音聊天' },
  { name: 'Spotify.exe', desc: 'Spotify 音乐' },
  { name: 'Steam.exe', desc: 'Steam 游戏平台' },
  { name: 'Code.exe', desc: 'Visual Studio Code' },
  { name: 'WeChat.exe', desc: '微信' },
  { name: 'QQ.exe', desc: 'QQ' },
  { name: 'cloudmusic.exe', desc: '网易云音乐' },
  { name: 'BaiduNetdisk.exe', desc: '百度网盘' }
]

export default function ProcessRules({ onNavigate }: ProcessRulesProps) {
  const [rules, setRules] = useState<ProcessRule[]>(() => {
    const saved = localStorage.getItem('kunbox-process-rules')
    return saved ? JSON.parse(saved) : []
  })
  const [showAddDialog, setShowAddDialog] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null)
  const [editingRule, setEditingRule] = useState<ProcessRule | null>(null)
  const [toasts, setToasts] = useState<ToastMessage[]>([])
  const [showPresets, setShowPresets] = useState(false)
  const [tunEnabled, setTunEnabled] = useState(true) // Default to true to avoid flash

  const [profiles, setProfiles] = useState<Profile[]>([])
  const [isLoadingData, setIsLoadingData] = useState(false)

  const { nodes, loadNodes } = useNodesStore()

  const [dialogData, setDialogData] = useState({
    processName: '',
    outboundMode: 'proxy' as OutboundMode,
    outboundValue: ''
  })

  useEffect(() => {
    localStorage.setItem('kunbox-process-rules', JSON.stringify(rules))
  }, [rules])

  // Load TUN status
  useEffect(() => {
    window.api.settings.get().then((settings: AppSettings | null) => {
      if (settings) {
        setTunEnabled(settings.tunEnabled)
      }
    })
  }, [])

  const loadProfiles = async () => {
    try {
      const list = await window.api.profile.list()
      setProfiles(list)
    } catch (error) {
      console.error('Failed to load profiles:', error)
    }
  }

  const loadAllData = useCallback(async () => {
    setIsLoadingData(true)
    try {
      await Promise.all([loadNodes(), loadProfiles()])
    } finally {
      setIsLoadingData(false)
    }
  }, [loadNodes])

  useEffect(() => {
    loadAllData()
  }, [loadAllData])

  const showToast = useCallback(
    (message: string, type: 'success' | 'error' | 'info' = 'info') => {
      const id = Date.now()
      setToasts((prev) => [...prev, { id, message, type }])
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id))
      }, 3000)
    },
    []
  )

  const getModeIcon = (mode: OutboundMode) => {
    switch (mode) {
      case 'direct':
        return <Globe className="w-3.5 h-3.5" />
      case 'proxy':
        return <Zap className="w-3.5 h-3.5" />
      case 'block':
        return <Ban className="w-3.5 h-3.5" />
      case 'node':
        return <Server className="w-3.5 h-3.5" />
      case 'profile':
        return <FileText className="w-3.5 h-3.5" />
    }
  }

  const getModeStyle = (mode: OutboundMode): string => {
    switch (mode) {
      case 'direct':
        return 'text-emerald-400 bg-emerald-500/15 border-emerald-500/30'
      case 'proxy':
        return 'text-violet-400 bg-violet-500/15 border-violet-500/30'
      case 'block':
        return 'text-red-400 bg-red-500/15 border-red-500/30'
      case 'node':
        return 'text-orange-400 bg-orange-500/15 border-orange-500/30'
      case 'profile':
        return 'text-cyan-400 bg-cyan-500/15 border-cyan-500/30'
    }
  }

  const getModeLabel = (mode: OutboundMode): string => {
    switch (mode) {
      case 'direct':
        return '直连'
      case 'proxy':
        return '代理'
      case 'block':
        return '拦截'
      case 'node':
        return '节点'
      case 'profile':
        return '配置'
    }
  }

  const toggleRule = (id: string) => {
    setRules((prev) =>
      prev.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r))
    )
  }

  const changeOutboundMode = (id: string, mode: OutboundMode) => {
    setRules((prev) =>
      prev.map((r) =>
        r.id === id ? { ...r, outboundMode: mode, outboundValue: '' } : r
      )
    )
  }

  const confirmDelete = (id: string) => {
    setDeleteTargetId(id)
    setShowDeleteConfirm(true)
  }

  const deleteRule = () => {
    if (deleteTargetId) {
      const target = rules.find((r) => r.id === deleteTargetId)
      setRules((prev) => prev.filter((r) => r.id !== deleteTargetId))
      showToast(`已删除规则「${target?.processName}」`, 'success')
      setDeleteTargetId(null)
    }
    setShowDeleteConfirm(false)
  }

  const openEditDialog = (rule: ProcessRule) => {
    setEditingRule(rule)
    setDialogData({
      processName: rule.processName,
      outboundMode: rule.outboundMode,
      outboundValue: rule.outboundValue || ''
    })
    setShowAddDialog(true)
  }

  const openAddDialog = () => {
    setEditingRule(null)
    setDialogData({
      processName: '',
      outboundMode: 'proxy',
      outboundValue: ''
    })
    setShowAddDialog(true)
  }

  const addFromPreset = (processName: string) => {
    if (rules.some((r) => r.processName.toLowerCase() === processName.toLowerCase())) {
      showToast(`规则「${processName}」已存在`, 'error')
      return
    }
    const newRule: ProcessRule = {
      id: Date.now().toString(),
      processName,
      outboundMode: 'proxy',
      enabled: true
    }
    setRules((prev) => [...prev, newRule])
    showToast(`已添加规则「${processName}」`, 'success')
  }

  const saveRule = () => {
    const processName = dialogData.processName.trim()
    if (!processName) return

    if (
      (dialogData.outboundMode === 'node' ||
        dialogData.outboundMode === 'profile') &&
      !dialogData.outboundValue
    ) {
      showToast(
        dialogData.outboundMode === 'node' ? '请选择节点' : '请选择配置',
        'error'
      )
      return
    }

    if (editingRule) {
      setRules((prev) =>
        prev.map((r) =>
          r.id === editingRule.id
            ? {
                ...r,
                processName,
                outboundMode: dialogData.outboundMode,
                outboundValue: dialogData.outboundValue || undefined
              }
            : r
        )
      )
      showToast(`规则「${processName}」已更新`, 'success')
    } else {
      if (rules.some((r) => r.processName.toLowerCase() === processName.toLowerCase())) {
        showToast(`规则「${processName}」已存在`, 'error')
        return
      }
      const newRule: ProcessRule = {
        id: Date.now().toString(),
        processName,
        outboundMode: dialogData.outboundMode,
        outboundValue: dialogData.outboundValue || undefined,
        enabled: true
      }
      setRules((prev) => [...prev, newRule])
      showToast(`规则「${processName}」已添加`, 'success')
    }
    setShowAddDialog(false)
  }

  const needsOutboundValue =
    dialogData.outboundMode === 'node' || dialogData.outboundMode === 'profile'

  const availableNodes = nodes.filter((n) => n.tag)
  const availableProfiles = profiles
  const unusedPresets = commonProcesses.filter(
    (p) => !rules.some((r) => r.processName.toLowerCase() === p.name.toLowerCase())
  )

  return (
    <div className="h-full flex flex-col px-6 pb-6 overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-orange-500/20 to-orange-500/5 flex items-center justify-center border border-orange-500/20">
            <Cpu className="w-7 h-7 text-orange-400" />
          </div>
          <div className="space-y-1">
            <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">
              进程分流
            </h2>
            <p className="text-[var(--text-muted)] text-sm font-medium">
              为特定应用程序设置路由规则
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowPresets(!showPresets)}
            className={`h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 border transition-colors duration-150 ${
              showPresets
                ? 'bg-amber-500/20 text-amber-400 border-amber-500/30'
                : 'bg-[var(--bg-secondary)] text-[var(--text-primary)] border-[var(--glass-border)] hover:bg-[var(--bg-tertiary)]'
            }`}
          >
            <Sparkles className="w-4 h-4" />
            常用程序
          </button>
          <button
            onClick={openAddDialog}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90 shadow-lg shadow-[var(--accent-primary)]/20 transition-colors duration-150 active:scale-[0.98]"
          >
            <Plus className="w-4 h-4" />
            添加规则
          </button>
        </div>
      </div>

      {/* TUN Mode Warning */}
      {!tunEnabled && (
        <div className="mb-4 p-4 rounded-2xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-between gap-4">
          <div className="flex items-start gap-3">
            <AlertTriangle className="w-5 h-5 text-amber-400 flex-shrink-0 mt-0.5" />
            <div>
              <p className="text-sm font-medium text-amber-400">需要启用 TUN 模式</p>
              <p className="text-xs text-[var(--text-muted)] mt-1">
                进程分流功能仅在 TUN 模式下有效，系统代理模式无法识别进程信息。
              </p>
            </div>
          </div>
          <button
            onClick={() => onNavigate?.('settings', 'tun')}
            className="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-medium bg-amber-500 text-white hover:bg-amber-600 transition-colors duration-150 whitespace-nowrap shadow-md"
          >
            前往设置
            <ArrowRight className="w-4 h-4" />
          </button>
        </div>
      )}

      {/* Presets Panel */}
      {showPresets && (
        <div className="mb-4">
          <div className="glass-card p-4 rounded-2xl border border-[var(--glass-border)]">
            <div className="flex items-center gap-2 mb-3">
              <Sparkles className="w-4 h-4 text-amber-400" />
              <span className="text-sm font-semibold text-[var(--text-primary)]">
                快速添加常用程序
              </span>
              <span className="text-xs text-[var(--text-muted)]">
                (点击添加)
              </span>
            </div>
            {unusedPresets.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {unusedPresets.map((preset) => (
                  <button
                    key={preset.name}
                    onClick={() => addFromPreset(preset.name)}
                    className="px-3 py-1.5 rounded-lg text-xs font-medium bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] hover:bg-[var(--bg-tertiary)] hover:border-[var(--accent-primary)]/50 transition-colors duration-150"
                    title={preset.desc}
                  >
                    {preset.name}
                  </button>
                ))}
              </div>
            ) : (
              <p className="text-xs text-[var(--text-muted)]">
                所有常用程序都已添加
              </p>
            )}
          </div>
        </div>
      )}

      {/* Rules List */}
      <div className="glass-card p-4 rounded-2xl border border-[var(--glass-border)]">
        <div className="flex items-center gap-2 mb-4">
          <div className="w-8 h-8 rounded-lg bg-orange-500/10 flex items-center justify-center">
            <Cpu className="w-4 h-4 text-orange-400" />
          </div>
          <span className="text-sm font-semibold text-[var(--text-primary)]">
            进程规则
          </span>
          <span className="text-xs text-[var(--text-muted)] bg-[var(--bg-tertiary)] px-2 py-0.5 rounded-full">
            {rules.length} 条规则
          </span>
        </div>

        <div className="space-y-2">
          {rules.map((rule) => (
            <div
              key={rule.id}
              className={`flex items-center gap-3 p-3 rounded-xl bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150 ${
                !rule.enabled && 'opacity-50'
              }`}
            >
              <Switch.Root
                checked={rule.enabled}
                onCheckedChange={() => toggleRule(rule.id)}
                className="w-10 h-6 rounded-full bg-[var(--bg-tertiary)] data-[state=checked]:bg-[var(--accent-primary)] transition-colors flex-shrink-0"
              >
                <Switch.Thumb className="block w-4 h-4 bg-white rounded-full transition-transform translate-x-1 data-[state=checked]:translate-x-5 shadow-sm" />
              </Switch.Root>

              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-0.5">
                  <Cpu className="w-3.5 h-3.5 text-orange-400 flex-shrink-0" />
                  <span className="text-sm font-semibold text-[var(--text-primary)] truncate">
                    {rule.processName}
                  </span>
                </div>
                {rule.outboundValue && (
                  <p className="text-[10px] text-[var(--text-faint)]">
                    → {rule.outboundValue}
                  </p>
                )}
              </div>

              <div
                className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border text-xs font-semibold ${getModeStyle(rule.outboundMode)}`}
              >
                {getModeIcon(rule.outboundMode)}
                <span>{getModeLabel(rule.outboundMode)}</span>
              </div>

              <select
                value={rule.outboundMode}
                onChange={(e) =>
                  changeOutboundMode(rule.id, e.target.value as OutboundMode)
                }
                className="h-8 px-2 rounded-lg bg-[var(--bg-tertiary)] text-sm text-[var(--text-primary)] border border-[var(--glass-border)] outline-none cursor-pointer hover:bg-[var(--bg-hover)] transition-colors duration-150"
              >
                <option value="direct">直连</option>
                <option value="proxy">代理</option>
                <option value="block">拦截</option>
                <option value="node">节点</option>
                <option value="profile">配置</option>
              </select>

              <div className="flex items-center gap-1">
                <button
                  onClick={() => openEditDialog(rule)}
                  className="p-2 rounded-lg hover:bg-[var(--bg-hover)] transition-colors duration-150"
                  title="编辑"
                >
                  <Edit2 className="w-4 h-4 text-[var(--text-muted)]" />
                </button>
                <button
                  onClick={() => confirmDelete(rule.id)}
                  className="p-2 rounded-lg hover:bg-red-500/10 transition-colors duration-150"
                  title="删除"
                >
                  <Trash2 className="w-4 h-4 text-red-400" />
                </button>
              </div>
            </div>
          ))}
        </div>

        {rules.length === 0 && (
          <div className="text-center py-12 text-[var(--text-muted)]">
            <Cpu className="w-12 h-12 mx-auto mb-3 opacity-30" />
            <p>暂无进程规则</p>
            <p className="text-xs mt-1">点击上方按钮添加应用程序分流规则</p>
          </div>
        )}

        <p className="text-xs text-[var(--text-faint)] mt-4">
          进程规则优先级高于规则集，匹配的流量将按指定的出站模式处理
        </p>
      </div>

      {/* Usage Guide */}
      <div className="mt-6 glass-card p-5 rounded-2xl border border-[var(--glass-border)]">
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-4">
          使用说明
        </h3>
        <div className="space-y-3">
          <div className="flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-secondary)]">
            <span className="px-2 py-1 text-xs rounded font-bold text-orange-400 bg-orange-500/20 shrink-0">
              提示
            </span>
            <div>
              <p className="text-sm text-[var(--text-primary)] font-medium">
                如何获取进程名称？
              </p>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">
                打开任务管理器 (Ctrl+Shift+Esc)，在"详细信息"选项卡查看进程名称
              </p>
            </div>
          </div>
          <div className="flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-secondary)]">
            <span className="px-2 py-1 text-xs rounded font-bold text-blue-400 bg-blue-500/20 shrink-0">
              示例
            </span>
            <div>
              <p className="text-sm text-[var(--text-primary)] font-medium">
                chrome.exe、Telegram.exe、WeChat.exe
              </p>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">
                进程名需包含 .exe 后缀，区分大小写不敏感
              </p>
            </div>
          </div>
        </div>
      </div>

      {/* Outbound Modes Legend */}
      <div className="mt-4 glass-card p-5 rounded-2xl border border-[var(--glass-border)]">
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-4">
          出站模式说明
        </h3>
        <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
          {(['direct', 'proxy', 'block', 'node', 'profile'] as const).map(
            (mode) => (
              <div
                key={mode}
                className={`flex items-center gap-2 px-3 py-2 rounded-xl border ${getModeStyle(mode)}`}
              >
                {getModeIcon(mode)}
                <span className="font-semibold text-sm">
                  {getModeLabel(mode)}
                </span>
              </div>
            )
          )}
        </div>
        <div className="mt-3 text-xs text-[var(--text-faint)] space-y-1">
          <p>
            <strong>直连</strong> - 不经过代理，直接连接
          </p>
          <p>
            <strong>代理</strong> - 通过当前激活的代理节点
          </p>
          <p>
            <strong>拦截</strong> - 阻止该程序的网络连接
          </p>
          <p>
            <strong>节点</strong> - 指定使用特定节点
          </p>
          <p>
            <strong>配置</strong> - 指定使用特定订阅配置
          </p>
        </div>
      </div>

      {/* Add/Edit Modal */}
      <Modal
        isOpen={showAddDialog}
        onClose={() => setShowAddDialog(false)}
        title={editingRule ? '编辑进程规则' : '添加进程规则'}
        maxWidth="max-w-md"
        footer={
          <>
            <ModalButton variant="ghost" onClick={() => setShowAddDialog(false)}>
              取消
            </ModalButton>
            <ModalButton
              onClick={saveRule}
              disabled={
                !dialogData.processName.trim() ||
                (needsOutboundValue && !dialogData.outboundValue)
              }
            >
              保存
            </ModalButton>
          </>
        }
      >
        <div className="space-y-4">
          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              进程名称 *
            </label>
            <input
              type="text"
              value={dialogData.processName}
              onChange={(e) =>
                setDialogData({ ...dialogData, processName: e.target.value })
              }
              placeholder="例如: chrome.exe、Telegram.exe"
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent-primary)] transition-colors"
            />
            <p className="text-xs text-[var(--text-faint)] mt-1.5">
              从任务管理器"详细信息"选项卡获取进程名
            </p>
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              出站模式
            </label>
            <select
              value={dialogData.outboundMode}
              onChange={(e) =>
                setDialogData({
                  ...dialogData,
                  outboundMode: e.target.value as OutboundMode,
                  outboundValue: ''
                })
              }
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none cursor-pointer"
            >
              <option value="direct">直连 - 不经过代理</option>
              <option value="proxy">代理 - 通过代理服务器</option>
              <option value="block">拦截 - 阻止连接</option>
              <option value="node">节点 - 指定特定节点</option>
              <option value="profile">配置 - 指定特定配置</option>
            </select>
          </div>

          {dialogData.outboundMode === 'node' && (
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1.5">
                选择节点 *
              </label>
              {isLoadingData ? (
                <div className="flex items-center gap-2 h-10 px-3 rounded-xl bg-[var(--bg-secondary)] border border-[var(--glass-border)]">
                  <Loader2 className="w-4 h-4 animate-spin text-[var(--text-muted)]" />
                  <span className="text-sm text-[var(--text-muted)]">
                    加载中...
                  </span>
                </div>
              ) : availableNodes.length === 0 ? (
                <div className="p-3 rounded-xl bg-amber-500/10 border border-amber-500/20">
                  <p className="text-xs text-amber-400">
                    暂无可用节点，请先添加订阅或节点
                  </p>
                </div>
              ) : (
                <select
                  value={dialogData.outboundValue}
                  onChange={(e) =>
                    setDialogData({ ...dialogData, outboundValue: e.target.value })
                  }
                  className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none cursor-pointer"
                >
                  <option value="">请选择节点...</option>
                  {availableNodes.map((node) => (
                    <option key={node.tag} value={node.tag}>
                      {node.tag} ({node.type})
                    </option>
                  ))}
                </select>
              )}
              <p className="text-xs text-[var(--text-faint)] mt-1.5">
                共 {availableNodes.length} 个可用节点
              </p>
            </div>
          )}

          {dialogData.outboundMode === 'profile' && (
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1.5">
                选择配置 *
              </label>
              {isLoadingData ? (
                <div className="flex items-center gap-2 h-10 px-3 rounded-xl bg-[var(--bg-secondary)] border border-[var(--glass-border)]">
                  <Loader2 className="w-4 h-4 animate-spin text-[var(--text-muted)]" />
                  <span className="text-sm text-[var(--text-muted)]">
                    加载中...
                  </span>
                </div>
              ) : availableProfiles.length === 0 ? (
                <div className="p-3 rounded-xl bg-amber-500/10 border border-amber-500/20">
                  <p className="text-xs text-amber-400">
                    暂无可用配置，请先添加订阅
                  </p>
                </div>
              ) : (
                <select
                  value={dialogData.outboundValue}
                  onChange={(e) =>
                    setDialogData({ ...dialogData, outboundValue: e.target.value })
                  }
                  className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none cursor-pointer"
                >
                  <option value="">请选择配置...</option>
                  {availableProfiles.map((profile) => (
                    <option key={profile.id} value={profile.name}>
                      {profile.name}
                    </option>
                  ))}
                </select>
              )}
              <p className="text-xs text-[var(--text-faint)] mt-1.5">
                共 {availableProfiles.length} 个可用配置
              </p>
            </div>
          )}
        </div>
      </Modal>

      {/* Delete Confirm Modal */}
      <Modal
        isOpen={showDeleteConfirm}
        onClose={() => setShowDeleteConfirm(false)}
        title="确认删除"
        maxWidth="max-w-sm"
        footer={
          <>
            <ModalButton
              variant="ghost"
              onClick={() => setShowDeleteConfirm(false)}
            >
              取消
            </ModalButton>
            <ModalButton variant="danger" onClick={deleteRule}>
              删除
            </ModalButton>
          </>
        }
      >
        <p className="text-[var(--text-secondary)]">
          确定要删除这条进程规则吗？此操作无法撤销。
        </p>
      </Modal>

      {/* Toast Notifications */}
      <AnimatePresence>
        {toasts.map((toast, index) => (
          <motion.div
            key={toast.id}
            initial={{ opacity: 0, y: 30 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            transition={fastTransition}
            style={{ bottom: `${32 + index * 60}px` }}
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

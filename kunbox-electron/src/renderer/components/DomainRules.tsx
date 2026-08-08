import { useState, useEffect, useCallback, useMemo } from 'react'
import * as Switch from '@radix-ui/react-switch'
import {
  AlertTriangle,
  Globe2,
  Plus,
  Globe,
  Zap,
  Ban,
  Edit2,
  Trash2,
  Server,
  FileText,
  Loader2,
  ChevronDown
} from 'lucide-react'
import { Modal, ModalButton } from './ui/Modal'
import { AppSelect } from './ui/Select'
import { useShallow } from 'zustand/react/shallow'
import { useNodesStore } from '../stores/nodesStore'
import type { DomainRule, DomainRuleType, OutboundMode } from '@shared/types'
import { useProfiles } from '../lib/useProfiles'
import { useToast } from './ui/Toast'

const DOMAIN_TYPE_OPTIONS = [
  { value: 'domain', label: '精确匹配' },
  { value: 'domain_suffix', label: '后缀匹配' },
  { value: 'domain_keyword', label: '关键字匹配' },
] as const

const OUTBOUND_MODE_OPTIONS = [
  { value: 'direct', label: '直连，不经过代理' },
  { value: 'proxy', label: '代理，通过当前代理服务器' },
  { value: 'block', label: '拦截，阻止连接' },
  { value: 'node', label: '节点，指定特定节点' },
  { value: 'profile', label: '配置，指定特定配置' },
] as const

function isBuiltInDomainRule(rule: DomainRule): boolean {
  return rule.id.startsWith('default-') || rule.id.startsWith('ms-')
}

export default function DomainRules() {
  const [rules, setRules] = useState<DomainRule[]>([])
  const [showAddDialog, setShowAddDialog] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null)
  const [editingRule, setEditingRule] = useState<DomainRule | null>(null)
  const [showBuiltInRules, setShowBuiltInRules] = useState(false)

  const { profiles, loadProfiles } = useProfiles()
  const [isLoadingData, setIsLoadingData] = useState(false)
  const toast = useToast()

  const { allNodes, loadAllNodes } = useNodesStore(
    useShallow((s) => ({
      allNodes: s.allNodes,
      loadAllNodes: s.loadAllNodes
    }))
  )

  const [dialogData, setDialogData] = useState({
    value: '',
    type: 'domain_suffix' as DomainRuleType,
    outboundMode: 'proxy' as OutboundMode,
    outboundValue: ''
  })

  // Load rules from backend
  const loadRules = useCallback(async () => {
    try {
      const savedRules = await window.api.customRules.getDomainRules()
      setRules(savedRules)
    } catch (error) {
      console.error('Failed to load domain rules:', error)
    }
  }, [])

  // Save rules to backend
  const saveRules = useCallback(async (newRules: DomainRule[]) => {
    try {
      await window.api.customRules.saveDomainRules(newRules)
      setRules(newRules)
      return true
    } catch (error) {
      console.error('Failed to save domain rules:', error)
      toast.error('保存域名规则失败')
      return false
    }
  }, [toast])

  useEffect(() => {
    loadRules()
  }, [loadRules])

  const loadProfilesSafe = useCallback(async () => {
    try {
      await loadProfiles()
    } catch (error) {
      console.error('Failed to load profiles:', error)
    }
  }, [loadProfiles])

  const loadAllData = useCallback(async () => {
    setIsLoadingData(true)
    try {
      await Promise.all([loadAllNodes(), loadProfilesSafe()])
    } finally {
      setIsLoadingData(false)
    }
  }, [loadAllNodes, loadProfilesSafe])

  useEffect(() => {
    loadAllData()
  }, [loadAllData])


  const getTypeDescription = (type: DomainRuleType): string => {
    switch (type) {
      case 'domain':
        return '只匹配输入的完整域名'
      case 'domain_suffix':
        return '匹配输入域名及其所有子域名'
      case 'domain_keyword':
        return '匹配域名中包含的关键字'
    }
  }

  const getTypeLabel = (type: DomainRuleType): string => {
    switch (type) {
      case 'domain':
        return '精确'
      case 'domain_suffix':
        return '后缀'
      case 'domain_keyword':
        return '关键字'
    }
  }

  const getTypeStyle = (type: DomainRuleType): string => {
    switch (type) {
      case 'domain':
        return 'text-blue-400 bg-blue-500/15'
      case 'domain_suffix':
        return 'text-purple-400 bg-purple-500/15'
      case 'domain_keyword':
        return 'text-amber-400 bg-amber-500/15'
    }
  }

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

  const toggleRule = async (id: string) => {
    const rule = rules.find(r => r.id === id)
    const newRules = rules.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r))
    const saved = await saveRules(newRules)
    if (saved) {
      toast.showRestartToast(`规则「${rule?.name || rule?.value}」已${rule?.enabled ? '禁用' : '启用'}`)
    }
  }

  const confirmDelete = (id: string) => {
    setDeleteTargetId(id)
    setShowDeleteConfirm(true)
  }

  const deleteRule = async () => {
    if (deleteTargetId) {
      const target = rules.find((r) => r.id === deleteTargetId)
      const newRules = rules.filter((r) => r.id !== deleteTargetId)
      const saved = await saveRules(newRules)
      if (saved) {
        toast.showRestartToast(`已删除规则「${target?.name || target?.value}」`)
      }
      setDeleteTargetId(null)
    }
    setShowDeleteConfirm(false)
  }

  const openEditDialog = (rule: DomainRule) => {
    setEditingRule(rule)
    setDialogData({
      value: rule.value,
      type: rule.type,
      outboundMode: rule.outboundMode,
      outboundValue: rule.outboundValue || ''
    })
    setShowAddDialog(true)
  }

  const openAddDialog = () => {
    setEditingRule(null)
    setDialogData({
      value: '',
      type: 'domain_suffix',
      outboundMode: 'proxy',
      outboundValue: ''
    })
    setShowAddDialog(true)
  }

  const saveRule = async () => {
    const rawValue = dialogData.value.trim()
    if (!rawValue) return

    if (
      (dialogData.outboundMode === 'node' ||
        dialogData.outboundMode === 'profile') &&
      !dialogData.outboundValue
    ) {
      toast.error(dialogData.outboundMode === 'node' ? '请选择节点' : '请选择配置')
      return
    }

    const finalValue = rawValue
    const finalName = finalValue.substring(0, 50)

    if (editingRule) {
      const newRules = rules.map((r) =>
        r.id === editingRule.id
          ? {
              ...r,
              name: finalName,
              type: dialogData.type,
              value: finalValue,
              outboundMode: dialogData.outboundMode,
              outboundValue: dialogData.outboundValue || undefined
            }
          : r
      )
      const saved = await saveRules(newRules)
      if (saved) {
        toast.showRestartToast(`规则「${finalName}」已更新`)
        setShowAddDialog(false)
      }
    } else {
      const newRule: DomainRule = {
        id: Date.now().toString(),
        name: finalName,
        type: dialogData.type,
        value: finalValue,
        outboundMode: dialogData.outboundMode,
        outboundValue: dialogData.outboundValue || undefined,
        enabled: true
      }
      const saved = await saveRules([...rules, newRule])
      if (saved) {
        toast.showRestartToast(`规则「${finalName}」已添加`)
        setShowAddDialog(false)
      }
    }
  }

  const needsOutboundValue =
    dialogData.outboundMode === 'node' || dialogData.outboundMode === 'profile'

  const availableNodes = allNodes.filter((n) => n.tag)
  const availableProfiles = profiles
  const builtInRules = rules.filter(isBuiltInDomainRule)
  const userRules = rules.filter((rule) => !isBuiltInDomainRule(rule))

  const domainNotices = useMemo(() => {
    const value = dialogData.value.trim()
    if (!value) return []

    const notices: string[] = []
    const duplicate = rules.find((rule) =>
      rule.id !== editingRule?.id &&
      rule.value.toLocaleLowerCase() === value.toLocaleLowerCase()
    )

    if (duplicate) {
      notices.push(`已有规则“${duplicate.value}”，仍可保存。发生冲突时，列表中更靠前的域名规则先匹配。`)
    }
    if (value.startsWith('=') || value.includes('*')) {
      notices.push('匹配类型已改为手动选择，等号和星号会作为规则内容保存。')
    }
    if (value.includes('://') || value.includes('/')) {
      notices.push('输入内容包含协议或路径，通常只需填写域名部分。当前内容仍可保存。')
    }

    return notices
  }, [dialogData.value, editingRule?.id, rules])

  const renderRule = (rule: DomainRule) => (
    <div
      key={rule.id}
      className={`flex items-center gap-3 p-3 rounded-xl bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] border border-[var(--glass-border)] transition-colors duration-150 ${
        !rule.enabled ? 'opacity-50' : ''
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
        <div className="flex items-center gap-2 mb-0.5 flex-wrap">
          <span className="text-sm font-semibold text-[var(--text-primary)] truncate">
            {rule.value}
          </span>
          <span
            className={`px-1.5 py-0.5 text-[10px] rounded font-semibold ${getTypeStyle(rule.type)}`}
          >
            {getTypeLabel(rule.type)}
          </span>
        </div>
        {rule.outboundValue && (
          <p className="text-[10px] text-[var(--text-faint)]">
            → {rule.outboundMode === 'profile'
              ? (profiles.find((profile) => profile.id === rule.outboundValue)?.name || rule.outboundValue)
              : rule.outboundValue.includes('::')
                ? rule.outboundValue.split('::')[1]
                : rule.outboundValue}
          </p>
        )}
      </div>

      <div
        className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border text-xs font-semibold ${getModeStyle(rule.outboundMode)}`}
      >
        {getModeIcon(rule.outboundMode)}
        <span>{getModeLabel(rule.outboundMode)}</span>
      </div>

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
  )

  return (
    <div className="h-full flex flex-col px-6 pb-6 overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-blue-500/20 to-blue-500/5 flex items-center justify-center border border-blue-500/20">
            <Globe2 className="w-7 h-7 text-blue-400" />
          </div>
          <div className="space-y-1">
            <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">
              域名分流
            </h2>
            <p className="text-[var(--text-muted)] text-sm font-medium">
              自定义域名的路由规则
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={openAddDialog}
            className="h-10 px-4 rounded-xl text-sm font-medium flex items-center gap-2 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90 shadow-lg shadow-[var(--accent-primary)]/20 transition-colors duration-150 active:scale-[0.98]"
          >
            <Plus className="w-4 h-4" />
            添加规则
          </button>
        </div>
      </div>

      {/* Rules List */}
      <div className="glass-card p-4 rounded-2xl border border-[var(--glass-border)]">
        <div className="flex items-center gap-2 mb-4">
          <div className="w-8 h-8 rounded-lg bg-blue-500/10 flex items-center justify-center">
            <Globe2 className="w-4 h-4 text-blue-400" />
          </div>
          <span className="text-sm font-semibold text-[var(--text-primary)]">
            域名规则
          </span>
          <span className="text-xs text-[var(--text-muted)] bg-[var(--bg-tertiary)] px-2 py-0.5 rounded-full">
            {rules.length} 条规则
          </span>
        </div>

        <div className="max-h-[560px] space-y-4 overflow-y-auto pr-1">
          {builtInRules.length > 0 && (
            <section className="rounded-xl border border-[var(--glass-border)] bg-[var(--bg-secondary)]/45 p-2">
              <button
                type="button"
                onClick={() => setShowBuiltInRules((isOpen) => !isOpen)}
                aria-expanded={showBuiltInRules}
                aria-controls="built-in-domain-rules"
                className="flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2.5 text-left transition-colors hover:bg-[var(--bg-hover)]"
              >
                <div>
                  <p className="text-sm font-semibold text-[var(--text-primary)]">内置域名规则</p>
                  <p className="mt-0.5 text-xs text-[var(--text-faint)]">{builtInRules.length} 条，默认收起</p>
                </div>
                <ChevronDown className={`h-4 w-4 text-[var(--text-muted)] transition-transform ${showBuiltInRules ? 'rotate-180' : ''}`} />
              </button>

              {showBuiltInRules && (
                <div id="built-in-domain-rules" className="mt-2 space-y-2 border-t border-[var(--glass-border)] pt-2">
                  {builtInRules.map(renderRule)}
                </div>
              )}
            </section>
          )}

          <section>
            <div className="mb-2 flex items-center gap-2 px-1">
              <span className="text-xs font-semibold text-[var(--text-secondary)]">用户域名规则</span>
              <span className="rounded-full bg-[var(--bg-tertiary)] px-2 py-0.5 text-[10px] text-[var(--text-muted)]">
                {userRules.length} 条
              </span>
            </div>

            {userRules.length > 0 ? (
              <div className="space-y-2">{userRules.map(renderRule)}</div>
            ) : (
              <div className="py-10 text-center text-[var(--text-muted)]">
                <Globe2 className="mx-auto mb-3 h-10 w-10 opacity-30" />
                <p>暂无用户域名规则</p>
                <p className="mt-1 text-xs">点击上方按钮添加自定义域名分流规则</p>
              </div>
            )}
          </section>
        </div>

        <p className="text-xs text-[var(--text-faint)] mt-4">
          域名规则优先级高于规则集，匹配的流量将按指定的出站模式处理
        </p>
      </div>

      {/* Usage Guide */}
      <div className="mt-6 glass-card p-5 rounded-2xl border border-[var(--glass-border)]">
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-4">
          域名匹配规则
        </h3>
        <p className="mb-4 text-xs text-[var(--text-faint)]">
          新增和编辑时手动选择匹配类型，输入框只填写域名或关键字。
        </p>
        <div className="space-y-3">
          <div className="flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-secondary)]">
            <span className="px-2 py-1 text-xs rounded font-bold text-purple-400 bg-purple-500/20 shrink-0">
              后缀
            </span>
            <div>
              <p className="text-sm text-[var(--text-primary)] font-medium">
                google.com
              </p>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">
                匹配 google.com 及所有子域名（如 www.google.com、mail.google.com）
              </p>
            </div>
          </div>
          <div className="flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-secondary)]">
            <span className="px-2 py-1 text-xs rounded font-bold text-blue-400 bg-blue-500/20 shrink-0">
              精确
            </span>
            <div>
              <p className="text-sm text-[var(--text-primary)] font-medium">
                www.google.com
              </p>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">
                仅匹配 www.google.com，不匹配其他子域名
              </p>
            </div>
          </div>
          <div className="flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-secondary)]">
            <span className="px-2 py-1 text-xs rounded font-bold text-amber-400 bg-amber-500/20 shrink-0">
              关键字
            </span>
            <div>
              <p className="text-sm text-[var(--text-primary)] font-medium">
                google
              </p>
              <p className="text-xs text-[var(--text-muted)] mt-0.5">
                匹配所有包含 google 的域名（如 googleapis.com、googlevideo.com）
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
            <strong>拦截</strong> - 阻止连接（用于屏蔽广告等）
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
        title={editingRule ? '编辑域名规则' : '添加域名规则'}
        maxWidth="max-w-md"
        className="min-h-[520px]"
        footer={
          <>
            <ModalButton variant="ghost" onClick={() => setShowAddDialog(false)}>
              取消
            </ModalButton>
            <ModalButton
              onClick={saveRule}
              disabled={
                !dialogData.value.trim() ||
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
              匹配类型
            </label>
            <AppSelect
              value={dialogData.type}
              options={DOMAIN_TYPE_OPTIONS}
              onValueChange={(value) =>
                setDialogData({ ...dialogData, type: value as DomainRuleType })
              }
              ariaLabel="选择域名匹配类型"
            />
            <p className="mt-1.5 text-xs text-[var(--text-faint)]">
              {getTypeDescription(dialogData.type)}
            </p>
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              域名 *
            </label>
            <input
              type="text"
              value={dialogData.value}
              onChange={(e) =>
                setDialogData({ ...dialogData, value: e.target.value })
              }
              placeholder={dialogData.type === 'domain_keyword' ? '例如 google' : '例如 google.com'}
              className="w-full h-10 px-3 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] border border-[var(--glass-border)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent-primary)] transition-colors"
            />
            {domainNotices.map((notice) => (
              <div
                key={notice}
                className="mt-2 flex items-start gap-2 rounded-lg border border-amber-500/25 bg-amber-500/10 p-2.5"
              >
                <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-400" />
                <p className="text-xs leading-5 text-amber-300">{notice}</p>
              </div>
            ))}
          </div>

          <div>
            <label className="block text-sm text-[var(--text-muted)] mb-1.5">
              出站模式
            </label>
            <AppSelect
              value={dialogData.outboundMode}
              options={OUTBOUND_MODE_OPTIONS}
              onValueChange={(value) =>
                setDialogData({
                  ...dialogData,
                  outboundMode: value as OutboundMode,
                  outboundValue: ''
                })
              }
              ariaLabel="选择出站模式"
            />
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
                <AppSelect
                  value={dialogData.outboundValue || ''}
                  onValueChange={(value) => setDialogData({ ...dialogData, outboundValue: value })}
                  options={availableNodes.map((node) => ({
                    value: `${node.sourceProfileId}::${node.tag}`,
                    label: `${node.tag} (${node.sourceProfileName})`
                  }))}
                  placeholder="请选择节点..."
                  ariaLabel="选择节点"
                />
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
                <AppSelect
                  value={dialogData.outboundValue}
                  onValueChange={(value) =>
                    setDialogData({ ...dialogData, outboundValue: value })
                  }
                  options={availableProfiles.map((profile) => ({
                    value: profile.id,
                    label: profile.name,
                  }))}
                  placeholder="请选择配置..."
                  ariaLabel="选择配置"
                />
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
          确定要删除这条域名规则吗？此操作无法撤销。
        </p>
      </Modal>

    </div>
  )
}

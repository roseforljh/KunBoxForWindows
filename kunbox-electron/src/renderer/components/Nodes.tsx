import { useEffect, useMemo, useState } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Search, RefreshCw, Check, Loader2, Zap, MoreVertical, Edit3, Share2, Trash2, Filter, Plus, X } from 'lucide-react'
import { useNodesStore } from '../stores/nodesStore'
import { cn } from '../lib/utils'
import { ConfirmModal } from './ui/ConfirmModal'
import { NodeDetailModal } from './ui/NodeDetailModal'
import { NodeFilterModal } from './ui/NodeFilterModal'
import { AddNodeModal } from './ui/AddNodeModal'
import { useToast } from './ui/Toast'
import type { SingBoxOutbound, Profile } from '@shared/types'

interface NodeItem extends SingBoxOutbound {
  latencyMs?: number | null
  isTimeout?: boolean
  isTesting?: boolean
}

const SORT_OPTIONS = [
  { id: 'default', label: '默认' },
  { id: 'region', label: '地区' },
  { id: 'latency', label: '延迟' },
  { id: 'name', label: '名称' }
] as const

export default function Nodes() {
  const {
    nodes,
    activeNodeTag,
    searchText,
    sortMode,
    nodeFilter,
    isTesting,
    testProgress,
    testTotal,
    setSearchText,
    setSortMode,
    setNodeFilter,
    selectNode,
    testAllLatency,
    cancelTestAllLatency,
    testNodeLatency,
    loadNodes
  } = useNodesStore()

  const [openMenuTag, setOpenMenuTag] = useState<string | null>(null)

  const [deleteModalOpen, setDeleteModalOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<NodeItem | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const [detailModalOpen, setDetailModalOpen] = useState(false)
  const [detailTarget, setDetailTarget] = useState<NodeItem | null>(null)

  const [filterModalOpen, setFilterModalOpen] = useState(false)

  const [addNodeModalOpen, setAddNodeModalOpen] = useState(false)

  const [profiles, setProfiles] = useState<Profile[]>([])
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null)
  const toast = useToast()

  useEffect(() => {
    loadNodes()
    loadProfiles()
  }, [loadNodes])

  const loadProfiles = async () => {
    try {
      const list = await window.api.profile.list()
      setProfiles(list)
      if (list.length > 0) {
        const active = list.find(p => p.enabled) || list[0]
        setActiveProfileId(active?.id || null)
      }
    } catch (error) {
      console.error('Failed to load profiles:', error)
    }
  }

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      if (!target.closest('[data-menu-container]')) {
        setOpenMenuTag(null)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  const filteredNodes = useMemo(() => {
    let result = [...nodes]

    if (nodeFilter.filterMode === 'include' && nodeFilter.includeKeywords.length > 0) {
      result = result.filter(n =>
        nodeFilter.includeKeywords.some(keyword =>
          n.tag?.toLowerCase().includes(keyword.toLowerCase())
        )
      )
    } else if (nodeFilter.filterMode === 'exclude' && nodeFilter.excludeKeywords.length > 0) {
      result = result.filter(n =>
        !nodeFilter.excludeKeywords.some(keyword =>
          n.tag?.toLowerCase().includes(keyword.toLowerCase())
        )
      )
    }

    if (searchText) {
      const search = searchText.toLowerCase()
      result = result.filter(n =>
        n.tag?.toLowerCase().includes(search) ||
        n.type?.toLowerCase().includes(search)
      )
    }

    switch (sortMode) {
      case 'latency':
        result.sort((a, b) => (a.latencyMs ?? Infinity) - (b.latencyMs ?? Infinity))
        break
      case 'name':
        result.sort((a, b) => (a.tag || '').localeCompare(b.tag || ''))
        break
    }

    return result
  }, [nodes, searchText, sortMode, nodeFilter])

  const getProtocolDisplay = (type?: string) => {
    const map: Record<string, string> = {
      shadowsocks: 'SS',
      vmess: 'VMess',
      vless: 'VLESS',
      trojan: 'Trojan',
      hysteria: 'Hysteria',
      hysteria2: 'Hy2',
      tuic: 'TUIC',
      http: 'HTTP',
      socks: 'SOCKS5'
    }
    return map[type?.toLowerCase() || ''] || type?.toUpperCase() || 'Unknown'
  }

  const getLatencyColor = (latency?: number | null, isTimeout?: boolean) => {
    if (isTimeout) return 'text-red-400'
    if (!latency || latency < 0) return 'text-text-muted'
    if (latency < 500) return 'text-green-400'
    if (latency < 1500) return 'text-yellow-400'
    return 'text-red-400'
  }

  const getLatencyDisplay = (node: NodeItem) => {
    if (node.isTimeout) return '超时'
    if (node.latencyMs && node.latencyMs > 0) return `${node.latencyMs}ms`
    return '延迟'
  }

  const handleTestSingleNode = async (tag: string) => {
    setOpenMenuTag(null)
    await testNodeLatency(tag)
  }

  const openDeleteModal = (node: NodeItem) => {
    setOpenMenuTag(null)
    setDeleteTarget(node)
    setDeleteModalOpen(true)
  }

  const handleConfirmDelete = async () => {
    if (!deleteTarget?.tag) return
    setIsDeleting(true)
    try {
      await window.api.node.delete(deleteTarget.tag)
      await loadNodes()
      setDeleteModalOpen(false)
      toast.success('节点已删除')
      setDeleteTarget(null)
    } catch (err) {
      toast.error(`删除失败: ${err}`)
    } finally {
      setIsDeleting(false)
    }
  }

  const handleEdit = (node: NodeItem) => {
    setOpenMenuTag(null)
    setDetailTarget(node)
    setDetailModalOpen(true)
  }

  const handleExport = async (node: NodeItem) => {
    setOpenMenuTag(null)
    if (!node.tag) return
    
    try {
      const link = await window.api.node.export(node.tag)
      await navigator.clipboard.writeText(link)
      toast.success(`已复制「${node.tag}」的分享链接`)
    } catch (error) {
      toast.error('导出失败')
    }
  }

  const handleAddNode = async (link: string, target: { type: 'existing'; profileId: string } | { type: 'new'; profileName: string }) => {
    try {
      await window.api.node.add(link, target)
      await loadNodes()
      await loadProfiles()
      toast.success('节点添加成功')
    } catch (err) {
      toast.error(`添加失败: ${err}`)
    }
  }

  return (
    <div className="h-full flex flex-col px-6 pb-6 relative">
      <div className="flex items-center justify-between mb-10">
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">节点选择</h2>
          <p className="text-[var(--text-muted)] text-sm font-medium">选择代理节点并测试延迟</p>
        </div>

        <div className="flex items-center gap-3">
          <div className="glass-card flex items-center gap-2 px-4 py-2.5 rounded-xl">
            <Search className="w-4 h-4 text-[var(--text-muted)]" />
            <input
              type="text"
              placeholder="搜索节点..."
              value={searchText}
              onChange={(e) => setSearchText(e.target.value)}
              className="bg-transparent border-none outline-none text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] w-36"
            />
          </div>

          <button
            onClick={() => setAddNodeModalOpen(true)}
            className="glass-btn h-11 w-11 !p-0 flex items-center justify-center rounded-xl"
            title="添加节点"
          >
            <Plus className="w-4 h-4 text-[var(--text-primary)]" />
          </button>

          <button
            onClick={() => setFilterModalOpen(true)}
            className={cn(
              'glass-btn h-11 w-11 !p-0 flex items-center justify-center rounded-xl',
              nodeFilter.filterMode !== 'none' && 'border-[var(--accent-primary)] bg-[var(--accent-muted)]'
            )}
            title="节点过滤"
          >
            <Filter className={cn(
              'w-4 h-4',
              nodeFilter.filterMode !== 'none' ? 'text-[var(--accent-primary)]' : 'text-[var(--text-primary)]'
            )} />
          </button>

          <button
            onClick={isTesting ? cancelTestAllLatency : testAllLatency}
            className={cn(
              "glass-btn h-11 w-11 !p-0 flex items-center justify-center rounded-xl",
              isTesting && "!bg-red-500/20 !border-red-500/30 hover:!bg-red-500/30"
            )}
          >
            {isTesting ? (
              <X className="w-4 h-4 text-red-400" />
            ) : (
              <RefreshCw className="w-4 h-4 text-[var(--text-primary)]" />
            )}
          </button>
        </div>
      </div>

      <div className="relative inline-flex p-1.5 bg-[var(--glass-bg)]/60 backdrop-blur-2xl rounded-2xl border border-[var(--glass-border)] mb-6">
        {SORT_OPTIONS.map((option) => (
          <button
            key={option.id}
            onClick={() => setSortMode(option.id)}
            className={cn(
              'relative px-5 py-2.5 rounded-xl text-sm font-semibold transition-colors duration-300 z-10',
              sortMode === option.id
                ? 'text-white'
                : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'
            )}
          >
            {option.label}
            {sortMode === option.id && (
              <motion.div
                layoutId="sort-tab-bg"
                className="absolute inset-0 bg-gradient-to-br from-[var(--accent-primary)] to-[var(--accent-secondary,var(--accent-primary))] rounded-xl -z-10 shadow-lg shadow-[var(--accent-primary)]/25"
                transition={{ type: "spring", bounce: 0.15, duration: 0.5 }}
              />
            )}
          </button>
        ))}
      </div>

      {isTesting && (
        <motion.div
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          className="glass-card p-5 mb-6 rounded-2xl"
        >
          <div className="flex items-center gap-4">
            <Loader2 className="w-6 h-6 text-[var(--accent-primary)] animate-spin" />
            <div className="flex-1">
              <p className="text-[var(--text-primary)] font-semibold">
                正在测试节点 ({testProgress}/{testTotal})
              </p>
              <div className="mt-2 h-1.5 bg-[var(--bg-elevated)] rounded-full overflow-hidden">
                <div
                  className="h-full bg-[var(--accent-primary)] transition-all rounded-full"
                  style={{ width: `${(testProgress / testTotal) * 100}%` }}
                />
              </div>
            </div>
          </div>
        </motion.div>
      )}

      <div className="flex-1 overflow-auto">
        {filteredNodes.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center">
            <div className="w-20 h-20 rounded-3xl bg-[var(--bg-elevated)] flex items-center justify-center mb-4">
              <Zap className="w-9 h-9 text-[var(--text-faint)]" />
            </div>
            <p className="text-xl font-semibold text-[var(--text-primary)]">暂无节点</p>
            <p className="text-sm text-[var(--text-faint)] mt-1">导入订阅以开始使用</p>
          </div>
        ) : (
          <div className="grid grid-cols-4 gap-5">
            {filteredNodes.map((node) => (
              <motion.div
                key={node.tag}
                whileTap={{ scale: 0.98 }}
                onClick={() => node.tag && selectNode(node.tag)}
                className={cn(
                  'group/card glass-card p-5 rounded-2xl cursor-pointer relative',
                  activeNodeTag === node.tag && 'border-[var(--accent-primary)]/50 bg-[var(--accent-muted)]',
                  openMenuTag === node.tag ? 'z-50' : 'z-0'
                )}
              >
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-2">
                    <div className="w-5 h-5 relative">
                      <AnimatePresence>
                        {activeNodeTag === node.tag && (
                          <motion.div
                            initial={{ scale: 0, opacity: 0 }}
                            animate={{ scale: 1, opacity: 1 }}
                            exit={{ scale: 0, opacity: 0 }}
                            transition={{ type: "spring", stiffness: 400, damping: 20 }}
                            className="absolute inset-0 rounded-full bg-[var(--accent-primary)] flex items-center justify-center"
                          >
                            <Check className="w-3 h-3 text-white" />
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                    <span className="px-2.5 py-1 bg-[var(--bg-elevated)] rounded-lg text-xs font-bold text-[var(--text-primary)]">
                      {getProtocolDisplay(node.type)}
                    </span>
                  </div>
                  {node.isTesting ? (
                    <Loader2 className="w-4 h-4 text-[var(--accent-primary)] animate-spin" />
                  ) : (
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        node.tag && handleTestSingleNode(node.tag)
                      }}
                      className={cn(
                        'text-xs font-bold font-mono flex items-center gap-1 px-2 py-1 rounded-lg transition-all duration-200',
                        'hover:bg-[var(--bg-elevated)] active:scale-95',
                        getLatencyColor(node.latencyMs, node.isTimeout)
                      )}
                    >
                      <Zap className="w-3 h-3" />
                      {getLatencyDisplay(node)}
                    </button>
                  )}
                </div>

                <p className="text-sm font-semibold text-[var(--text-primary)] truncate mb-3 transition-transform duration-200 origin-left group-hover/card:scale-105" title={node.tag}>
                  {node.tag}
                </p>

                <div className="flex items-center justify-between pt-3 border-t border-[var(--border-secondary)]">
                  <div className="flex items-center gap-2">
                    <span className="px-1.5 py-0.5 bg-[var(--bg-elevated)] rounded text-[10px] font-mono text-[var(--text-muted)]">
                      {getProtocolDisplay(node.type)}
                    </span>
                    <span className="text-[10px] text-[var(--text-faint)] truncate max-w-[80px]">
                      {node.server}
                    </span>
                  </div>

                  <div 
                    className="relative"
                    data-menu-container
                    onClick={(e) => e.stopPropagation()}
                  >
                    <button
                      onClick={() => setOpenMenuTag(openMenuTag === node.tag ? null : node.tag ?? null)}
                      className="w-7 h-7 flex items-center justify-center rounded-lg hover:bg-[var(--bg-elevated)] transition-colors"
                    >
                      <MoreVertical className="w-4 h-4 text-[var(--text-muted)]" />
                    </button>

                    <div
                      className={`absolute right-0 top-9 z-[100] w-24 py-1 glass-card rounded-xl border border-[var(--glass-border)] shadow-xl transition-opacity duration-150 ${
                        openMenuTag === node.tag
                          ? 'opacity-100 pointer-events-auto'
                          : 'opacity-0 pointer-events-none'
                      }`}
                    >
                      <button
                        onClick={() => handleEdit(node)}
                        className="w-full px-3 py-2 text-xs text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-2 transition-colors"
                      >
                        <Edit3 className="w-3.5 h-3.5 shrink-0" />
                        <span>编辑</span>
                      </button>
                      <button
                        onClick={() => handleExport(node)}
                        className="w-full px-3 py-2 text-xs text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-2 transition-colors"
                      >
                        <Share2 className="w-3.5 h-3.5 shrink-0" />
                        <span>导出</span>
                      </button>
                      <button
                        onClick={() => node.tag && handleTestSingleNode(node.tag)}
                        className="w-full px-3 py-2 text-xs text-[var(--text-primary)] hover:bg-[var(--bg-elevated)] flex items-center gap-2 transition-colors"
                      >
                        <Zap className="w-3.5 h-3.5 shrink-0" />
                        <span>延迟</span>
                      </button>
                      <div className="my-1 mx-2 border-t border-[var(--border-secondary)]" />
                      <button
                        onClick={() => openDeleteModal(node)}
                        className="w-full px-3 py-2 text-xs text-[var(--status-error)] hover:bg-[var(--status-error)]/10 flex items-center gap-2 transition-colors"
                      >
                        <Trash2 className="w-3.5 h-3.5 shrink-0" />
                        <span>删除</span>
                      </button>
                    </div>
                  </div>
                </div>
              </motion.div>
            ))}
          </div>
        )}
      </div>

      <ConfirmModal
        isOpen={deleteModalOpen}
        onClose={() => {
          setDeleteModalOpen(false)
          setDeleteTarget(null)
        }}
        onConfirm={handleConfirmDelete}
        title="删除节点"
        description={`确定要删除「${deleteTarget?.tag || ''}」吗？此操作不可撤销。`}
        confirmText="删除"
        cancelText="取消"
        variant="danger"
        isLoading={isDeleting}
      />

      <NodeDetailModal
        isOpen={detailModalOpen}
        onClose={() => {
          setDetailModalOpen(false)
          setDetailTarget(null)
        }}
        node={detailTarget}
        onExport={async (tag) => {
          const link = await window.api.node.export(tag)
          await navigator.clipboard.writeText(link)
          toast.success('已复制分享链接')
        }}
      />

      <NodeFilterModal
        isOpen={filterModalOpen}
        onClose={() => setFilterModalOpen(false)}
        currentFilter={nodeFilter}
        onApply={setNodeFilter}
      />

      <AddNodeModal
        isOpen={addNodeModalOpen}
        onClose={() => setAddNodeModalOpen(false)}
        onAdd={handleAddNode}
        profiles={profiles}
        currentProfileId={activeProfileId}
      />


    </div>
  )
}

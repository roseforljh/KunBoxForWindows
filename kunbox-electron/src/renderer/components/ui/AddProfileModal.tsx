import { useState } from 'react'
import { createPortal } from 'react-dom'
import { motion, AnimatePresence } from 'framer-motion'
import { X, Link, FileText, Clipboard, Loader2, Search, ListChecks, Plus, Trash2 } from 'lucide-react'
import type { CustomProfileNodeSelection, NodeWithProfile } from '@shared/types'
import { ManualNodeEditor } from './ManualNodeEditor'
import { MANUAL_NODE_PROTOCOLS, type ManualNodeDefinition } from './manual-node'
import { AppSelect } from './Select'

type ImportType = 'url' | 'clipboard' | 'file' | 'custom'

interface ProfileSettings {
  autoUpdateInterval: number
  dnsPreResolve: boolean
  dnsServer: string | null
}

interface AddProfileModalProps {
  isOpen: boolean
  onClose: () => void
  onImportUrl: (name: string, url: string, settings: ProfileSettings) => Promise<void>
  onImportContent: (name: string, content: string, settings: ProfileSettings) => Promise<void>
  onCreateCustom: (name: string, selections: CustomProfileNodeSelection[], newNodeLinks: string[]) => Promise<void>
  allNodes: NodeWithProfile[]
  isLoadingNodes: boolean
}

const IMPORT_TYPES: { id: ImportType; label: string; icon: typeof Link; description: string }[] = [
  { id: 'url', label: '订阅链接', icon: Link, description: '从 URL 导入订阅' },
  { id: 'clipboard', label: '剪贴板', icon: Clipboard, description: '从剪贴板内容导入' },
  { id: 'file', label: '本地文件', icon: FileText, description: '从 JSON/YAML 文件导入' },
  { id: 'custom', label: '自定义', icon: ListChecks, description: '组合已有节点或新建节点' }
]

const DNS_SERVERS = [
  { value: 'https://cloudflare-dns.com/dns-query', label: 'Cloudflare DNS' },
  { value: 'https://dns.google/dns-query', label: 'Google DNS' },
  { value: 'https://dns.alidns.com/dns-query', label: '阿里云 DNS' }
]

export function AddProfileModal({ isOpen, onClose, onImportUrl, onImportContent, onCreateCustom, allNodes, isLoadingNodes }: AddProfileModalProps) {
  const [importType, setImportType] = useState<ImportType>('url')
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [content, setContent] = useState('')
  const [customSearch, setCustomSearch] = useState('')
  const [selectedNodeKeys, setSelectedNodeKeys] = useState<Set<string>>(new Set())
  const [manualNodes, setManualNodes] = useState<Array<ManualNodeDefinition & { id: string }>>([])
  const [manualNodeEditorOpen, setManualNodeEditorOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const [autoUpdateEnabled, setAutoUpdateEnabled] = useState(false)
  const [autoUpdateMinutes, setAutoUpdateMinutes] = useState('60')
  const [dnsPreResolve, setDnsPreResolve] = useState(false)
  const [dnsServer, setDnsServer] = useState(DNS_SERVERS[0].value)

  const nodeKey = (node: NodeWithProfile) => `${node.sourceProfileId}::${node.tag || ''}`
  const selectableNodes = allNodes.filter((node) => node.tag)
  const customSearchText = customSearch.trim().toLowerCase()
  const filteredCustomNodes = customSearchText
    ? selectableNodes.filter((node) =>
        [node.tag, node.sourceProfileName, node.type]
          .filter(Boolean)
          .some((value) => String(value).toLowerCase().includes(customSearchText))
      )
    : selectableNodes
  const selectedCustomNodes = selectableNodes.filter((node) => selectedNodeKeys.has(nodeKey(node)))
  const selectedNodeCount = selectedCustomNodes.length + manualNodes.length

  const toggleCustomNode = (key: string) => {
    setSelectedNodeKeys((prev) => {
      const next = new Set(prev)
      const action = next.has(key) ? 'delete' : 'add'
      next[action](key)
      return next
    })
  }

  const handleClose = () => {
    if (loading) return
    setName('')
    setUrl('')
    setContent('')
    setCustomSearch('')
    setSelectedNodeKeys(new Set())
    setManualNodes([])
    setManualNodeEditorOpen(false)
    setError('')
    setImportType('url')
    setAutoUpdateEnabled(false)
    setAutoUpdateMinutes('60')
    setDnsPreResolve(false)
    setDnsServer(DNS_SERVERS[0].value)
    onClose()
  }

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText()
      setContent(text)
    } catch {
      setError('无法读取剪贴板')
    }
  }

  const handleFileSelect = async () => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.json,.yaml,.yml,.txt'
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0]
      if (!file) return

      try {
        const text = await file.text()
        setContent(text)
        if (!name) {
          setName(file.name.replace(/\.(json|yaml|yml|txt)$/i, ''))
        }
      } catch {
        setError('无法读取文件')
      }
    }
    input.click()
  }

  const handleImport = async () => {
    setError('')
    setLoading(true)

    try {
      if (importType === 'custom') {
        if (!name.trim()) {
          setError('请输入订阅名称')
          setLoading(false)
          return
        }
        if (selectedNodeCount === 0) {
          setError('请选择至少一个节点')
          setLoading(false)
          return
        }
        await onCreateCustom(
          name.trim(),
          selectedCustomNodes.map((node) => ({
            sourceProfileId: node.sourceProfileId,
            tag: node.tag || ''
          })),
          manualNodes.map((node) => node.link),
        )
      } else {
        const settings: ProfileSettings = {
          autoUpdateInterval: autoUpdateEnabled ? Math.max(15, parseInt(autoUpdateMinutes) || 60) : 0,
          dnsPreResolve,
          dnsServer: dnsPreResolve ? dnsServer : null
        }
        if (importType === 'url') {
          if (!url.trim()) {
            setError('请输入订阅链接')
            setLoading(false)
            return
          }
          if (autoUpdateEnabled && (parseInt(autoUpdateMinutes) || 0) < 15) {
            setError('自动更新间隔最少 15 分钟')
            setLoading(false)
            return
          }
          await onImportUrl(name.trim(), url.trim(), settings)
        } else {
          if (!content.trim()) {
            setError('内容不能为空')
            setLoading(false)
            return
          }
          await onImportContent(name.trim() || 'Imported', content.trim(), settings)
        }
      }
      handleClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : '导入失败')
    } finally {
      setLoading(false)
    }
  }

  const canImport = importType === 'custom'
    ? Boolean(name.trim() && (selectedNodeKeys.size > 0 || manualNodes.length > 0))
    : Boolean(importType === 'url' ? url.trim() : content.trim())

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm"
            onClick={handleClose}
          />
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            className={`relative ${manualNodeEditorOpen ? 'w-[720px]' : 'w-[560px]'} max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] glass-card rounded-2xl border border-[var(--glass-border)] shadow-2xl flex flex-col`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-5 border-b border-[var(--border-secondary)]">
              <h3 className="text-lg font-bold text-[var(--text-primary)]">{manualNodeEditorOpen ? '新建节点' : '添加订阅'}</h3>
              <button
                onClick={handleClose}
                disabled={loading}
                className="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-[var(--bg-elevated)] transition-colors disabled:opacity-50"
              >
                <X className="w-4 h-4 text-[var(--text-muted)]" />
              </button>
            </div>

            <div className="p-5 space-y-5 overflow-y-auto">
              {manualNodeEditorOpen ? (
                <ManualNodeEditor
                  disabled={loading}
                  onCancel={() => setManualNodeEditorOpen(false)}
                  onSave={(node) => {
                    setManualNodes((current) => [...current, { ...node, id: `${Date.now()}-${current.length}` }])
                    setManualNodeEditorOpen(false)
                  }}
                />
              ) : (
                <>
              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">导入方式</label>
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                  {IMPORT_TYPES.map((type) => {
                    const Icon = type.icon
                    return (
                      <button
                        key={type.id}
                        onClick={() => setImportType(type.id)}
                        disabled={loading}
                        className={`p-3 rounded-xl border transition-all ${
                          importType === type.id
                            ? 'border-[var(--accent-primary)] bg-[var(--accent-muted)]'
                            : 'border-[var(--border-secondary)] hover:border-[var(--text-faint)]'
                        } disabled:opacity-50`}
                      >
                        <Icon className={`w-5 h-5 mx-auto mb-1 ${
                          importType === type.id ? 'text-[var(--accent-primary)]' : 'text-[var(--text-muted)]'
                        }`} />
                        <p className={`text-sm font-semibold ${
                          importType === type.id ? 'text-[var(--accent-primary)]' : 'text-[var(--text-primary)]'
                        }`}>
                          {type.label}
                        </p>
                      </button>
                    )
                  })}
                </div>
                <p className="text-xs text-[var(--text-faint)]">
                  {IMPORT_TYPES.find(t => t.id === importType)?.description}
                </p>
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">
                  名称 <span className="text-[var(--text-faint)]">{importType === 'custom' ? '(必填)' : '(可选)'}</span>
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={importType === 'custom' ? '输入自定义订阅名称' : '自动从链接提取'}
                  disabled={loading}
                  className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                />
              </div>

              <AnimatePresence mode="wait">
                {importType === 'custom' ? (
                  <motion.div
                    key="custom-input"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-3">
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <label className="text-sm font-medium text-[var(--text-muted)]">选择节点</label>
                          <p className="text-xs text-[var(--text-faint)] mt-0.5">可混合使用已有节点与本次新建节点</p>
                        </div>
                        <div className="flex items-center gap-3">
                          <span className="text-xs text-[var(--text-faint)]">已选 {selectedNodeCount} 个</span>
                          <button
                            type="button"
                            onClick={() => setManualNodeEditorOpen(true)}
                            disabled={loading}
                            className="glass-btn h-9 px-3 rounded-lg text-sm font-medium flex items-center gap-1.5 text-[var(--accent-primary)] disabled:opacity-50"
                          >
                            <Plus className="w-4 h-4" />
                            新节点
                          </button>
                        </div>
                      </div>
                      {manualNodes.length > 0 && (
                        <div className="rounded-lg border border-[var(--accent-primary)]/30 bg-[var(--accent-muted)]/50 overflow-hidden">
                          <div className="px-3 py-2 border-b border-[var(--accent-primary)]/20 text-xs font-medium text-[var(--accent-primary)]">
                            本次新建
                          </div>
                          {manualNodes.map((node) => {
                            const protocol = MANUAL_NODE_PROTOCOLS.find((item) => item.id === node.protocol)
                            return (
                              <div key={node.id} className="min-h-[52px] px-3 py-2 flex items-center gap-3 border-b border-[var(--accent-primary)]/15 last:border-b-0">
                                <span className="inline-flex min-w-8 h-6 px-1.5 items-center justify-center rounded-md bg-[var(--accent-muted)] font-mono text-[11px] font-bold text-[var(--accent-primary)]">
                                  {protocol?.code || node.protocol.toUpperCase()}
                                </span>
                                <span className="min-w-0 flex-1">
                                  <span className="block text-sm font-medium text-[var(--text-primary)] truncate">{node.tag}</span>
                                  <span className="block text-xs text-[var(--text-faint)]">{protocol?.label || node.protocol}</span>
                                </span>
                                <button
                                  type="button"
                                  title="移除新节点"
                                  onClick={() => setManualNodes((current) => current.filter((item) => item.id !== node.id))}
                                  disabled={loading}
                                  className="w-8 h-8 rounded-lg flex items-center justify-center text-[var(--text-faint)] hover:text-[var(--status-error)] hover:bg-[var(--status-error)]/10 disabled:opacity-50"
                                >
                                  <Trash2 className="w-4 h-4" />
                                </button>
                              </div>
                            )
                          })}
                        </div>
                      )}
                      <div className="relative">
                        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-faint)]" />
                        <input
                          type="text"
                          value={customSearch}
                          onChange={(e) => setCustomSearch(e.target.value)}
                          placeholder="搜索节点或订阅"
                          disabled={loading}
                          className="w-full pl-9 pr-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                        />
                      </div>
                      <div className="max-h-64 overflow-auto rounded-lg border border-[var(--border-secondary)] bg-[var(--bg-elevated)]/40">
                        {filteredCustomNodes.length > 0 ? (
                          filteredCustomNodes.map((node) => {
                            const key = nodeKey(node)
                            const checked = selectedNodeKeys.has(key)
                            return (
                              <label
                                key={key}
                                className={`w-full min-h-[56px] px-3 py-2.5 flex items-center gap-3 text-left border-b border-[var(--border-secondary)] last:border-b-0 hover:bg-[var(--bg-elevated)] transition-colors ${
                                  checked ? 'bg-[var(--accent-muted)]' : ''
                                } ${loading ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
                              >
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  onChange={() => toggleCustomNode(key)}
                                  disabled={loading}
                                  className="w-5 h-5 shrink-0 accent-[var(--accent-primary)]"
                                />
                                <span className="min-w-0 flex-1">
                                  <span className="block text-sm font-medium text-[var(--text-primary)] truncate">{node.tag}</span>
                                  <span className="block text-xs text-[var(--text-faint)] truncate">{node.sourceProfileName} · {node.type || 'unknown'}</span>
                                </span>
                              </label>
                            )
                          })
                        ) : (
                          <div className="h-24 flex items-center justify-center text-sm text-[var(--text-faint)]">
                            {isLoadingNodes ? '节点加载中...' : (selectableNodes.length === 0 ? '暂无可选节点' : '未找到节点')}
                          </div>
                        )}
                      </div>
                    </div>
                  </motion.div>
                ) : importType === 'url' ? (
                  <motion.div
                    key="url-input"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-2">
                      <label className="text-sm font-medium text-[var(--text-muted)]">订阅链接</label>
                      <input
                        type="text"
                        value={url}
                        onChange={(e) => setUrl(e.target.value)}
                        placeholder="https://..."
                        disabled={loading}
                        className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                      />
                    </div>
                  </motion.div>
                ) : (
                  <motion.div
                    key="content-input"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <label className="text-sm font-medium text-[var(--text-muted)]">内容</label>
                        <div className="flex gap-2">
                          {importType === 'clipboard' && (
                            <button
                              onClick={handlePaste}
                              disabled={loading}
                              className="text-xs text-[var(--accent-primary)] hover:underline disabled:opacity-50"
                            >
                              粘贴
                            </button>
                          )}
                          {importType === 'file' && (
                            <button
                              onClick={handleFileSelect}
                              disabled={loading}
                              className="text-xs text-[var(--accent-primary)] hover:underline disabled:opacity-50"
                            >
                              选择文件
                            </button>
                          )}
                        </div>
                      </div>
                      <textarea
                        value={content}
                        onChange={(e) => setContent(e.target.value)}
                        placeholder={importType === 'clipboard' ? '点击"粘贴"或手动粘贴内容...' : '选择文件或粘贴内容...'}
                        disabled={loading}
                        rows={6}
                        className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] resize-none font-mono disabled:opacity-50"
                      />
                      <p className="text-xs text-[var(--text-faint)]">
                        支持 YAML、JSON、Base64 编码或节点链接格式
                      </p>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {importType !== 'custom' && (
                <div className="pt-2 border-t border-[var(--border-secondary)]">
                  <div className="flex items-center justify-between py-2">
                    <div>
                      <p className="text-sm font-medium text-[var(--text-primary)]">自动更新</p>
                      <p className="text-xs text-[var(--text-faint)]">定时更新订阅内容</p>
                    </div>
                    <button
                      onClick={() => setAutoUpdateEnabled(!autoUpdateEnabled)}
                      disabled={loading}
                      className={`w-11 h-6 rounded-full transition-colors ${
                        autoUpdateEnabled ? 'bg-[var(--accent-primary)]' : 'bg-[var(--bg-elevated)]'
                      } disabled:opacity-50`}
                    >
                      <div className={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                        autoUpdateEnabled ? 'translate-x-5' : 'translate-x-0.5'
                      }`} />
                    </button>
                  </div>

                  <AnimatePresence>
                    {autoUpdateEnabled && (
                      <motion.div
                        initial={{ height: 0, opacity: 0 }}
                        animate={{ height: 'auto', opacity: 1 }}
                        exit={{ height: 0, opacity: 0 }}
                        transition={{ duration: 0.2 }}
                        className="overflow-hidden"
                      >
                        <div className="pt-2 pb-1">
                          <label className="text-xs text-[var(--text-muted)]">更新间隔（分钟，最少 15）</label>
                          <input
                            type="number"
                            value={autoUpdateMinutes}
                            onChange={(e) => setAutoUpdateMinutes(e.target.value.replace(/\D/g, ''))}
                            min={15}
                            disabled={loading}
                            className="mt-1 w-full px-3 py-2 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                          />
                        </div>
                      </motion.div>
                    )}
                  </AnimatePresence>

                  <div className="flex items-center justify-between py-2 mt-2">
                    <div>
                      <p className="text-sm font-medium text-[var(--text-primary)]">DNS 预解析</p>
                      <p className="text-xs text-[var(--text-faint)]">启动前预解析节点域名，加快连接</p>
                    </div>
                    <button
                      onClick={() => setDnsPreResolve(!dnsPreResolve)}
                      disabled={loading}
                      className={`w-11 h-6 rounded-full transition-colors ${
                        dnsPreResolve ? 'bg-[var(--accent-primary)]' : 'bg-[var(--bg-elevated)]'
                      } disabled:opacity-50`}
                    >
                      <div className={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                        dnsPreResolve ? 'translate-x-5' : 'translate-x-0.5'
                      }`} />
                    </button>
                  </div>

                  <AnimatePresence>
                    {dnsPreResolve && (
                      <motion.div
                        initial={{ height: 0, opacity: 0 }}
                        animate={{ height: 'auto', opacity: 1 }}
                        exit={{ height: 0, opacity: 0 }}
                        transition={{ duration: 0.2 }}
                        className="overflow-hidden"
                      >
                        <div className="pt-2 pb-1">
                          <label className="text-xs text-[var(--text-muted)]">DNS 服务器</label>
                          <AppSelect
                            value={dnsServer}
                            options={DNS_SERVERS}
                            onValueChange={setDnsServer}
                            disabled={loading}
                            ariaLabel="选择 DNS 服务器"
                            className="mt-1"
                          />
                        </div>
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              )}

              {error && (
                <div className="px-3 py-2 bg-[var(--status-error)]/10 border border-[var(--status-error)]/30 rounded-lg">
                  <p className="text-sm text-[var(--status-error)]">{error}</p>
                </div>
              )}
                </>
              )}
            </div>

            {!manualNodeEditorOpen && (
              <div className="flex items-center justify-end gap-2 p-5 border-t border-[var(--border-secondary)]">
              <button
                onClick={handleClose}
                disabled={loading}
                className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
              >
                取消
              </button>
              <button
                onClick={handleImport}
                disabled={loading || !canImport}
                className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>{loading ? (importType === 'custom' ? '创建中...' : '导入中...') : (importType === 'custom' ? '创建' : '导入')}</span>
              </button>
              </div>
            )}
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body
  )
}

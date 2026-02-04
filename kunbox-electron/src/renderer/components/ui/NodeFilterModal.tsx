import { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { motion, AnimatePresence } from 'framer-motion'
import { X, Plus, Trash2 } from 'lucide-react'
import type { FilterMode, NodeFilter } from '../../stores/nodesStore'

interface NodeFilterModalProps {
  isOpen: boolean
  onClose: () => void
  currentFilter: NodeFilter
  onApply: (filter: NodeFilter) => void
}

const FILTER_MODES: { id: FilterMode; label: string; description: string }[] = [
  { id: 'none', label: '不过滤', description: '显示所有节点' },
  { id: 'include', label: '仅显示', description: '只显示包含关键字的节点' },
  { id: 'exclude', label: '排除', description: '排除包含关键字的节点' }
]

export function NodeFilterModal({ isOpen, onClose, currentFilter, onApply }: NodeFilterModalProps) {
  const [filterMode, setFilterMode] = useState<FilterMode>(currentFilter.filterMode)
  const [includeKeywords, setIncludeKeywords] = useState<string[]>(currentFilter.includeKeywords)
  const [excludeKeywords, setExcludeKeywords] = useState<string[]>(currentFilter.excludeKeywords)
  const [newKeyword, setNewKeyword] = useState('')

  useEffect(() => {
    if (isOpen) {
      setFilterMode(currentFilter.filterMode)
      setIncludeKeywords([...currentFilter.includeKeywords])
      setExcludeKeywords([...currentFilter.excludeKeywords])
      setNewKeyword('')
    }
  }, [isOpen, currentFilter])

  const handleAddKeyword = () => {
    const trimmed = newKeyword.trim()
    if (!trimmed) return

    if (filterMode === 'include') {
      if (!includeKeywords.includes(trimmed)) {
        setIncludeKeywords([...includeKeywords, trimmed])
      }
    } else if (filterMode === 'exclude') {
      if (!excludeKeywords.includes(trimmed)) {
        setExcludeKeywords([...excludeKeywords, trimmed])
      }
    }
    setNewKeyword('')
  }

  const handleRemoveKeyword = (keyword: string) => {
    if (filterMode === 'include') {
      setIncludeKeywords(includeKeywords.filter(k => k !== keyword))
    } else if (filterMode === 'exclude') {
      setExcludeKeywords(excludeKeywords.filter(k => k !== keyword))
    }
  }

  const handleApply = () => {
    onApply({
      filterMode,
      includeKeywords,
      excludeKeywords
    })
    onClose()
  }

  const handleClear = () => {
    onApply({
      filterMode: 'none',
      includeKeywords: [],
      excludeKeywords: []
    })
    onClose()
  }

  const currentKeywords = filterMode === 'include' ? includeKeywords : excludeKeywords

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm"
            onClick={onClose}
          />
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            className="relative w-[420px] glass-card rounded-2xl border border-[var(--glass-border)] shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-5 border-b border-[var(--border-secondary)]">
              <h3 className="text-lg font-bold text-[var(--text-primary)]">节点过滤</h3>
              <button
                onClick={onClose}
                className="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-[var(--bg-elevated)] transition-colors"
              >
                <X className="w-4 h-4 text-[var(--text-muted)]" />
              </button>
            </div>

            <div className="p-5 space-y-5">
              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">过滤模式</label>
                <div className="grid grid-cols-3 gap-2">
                  {FILTER_MODES.map((mode) => (
                    <button
                      key={mode.id}
                      onClick={() => setFilterMode(mode.id)}
                      className={`p-3 rounded-xl border transition-all ${
                        filterMode === mode.id
                          ? 'border-[var(--accent-primary)] bg-[var(--accent-muted)]'
                          : 'border-[var(--border-secondary)] hover:border-[var(--text-faint)]'
                      }`}
                    >
                      <p className={`text-sm font-semibold ${
                        filterMode === mode.id ? 'text-[var(--accent-primary)]' : 'text-[var(--text-primary)]'
                      }`}>
                        {mode.label}
                      </p>
                    </button>
                  ))}
                </div>
                <p className="text-xs text-[var(--text-faint)]">
                  {FILTER_MODES.find(m => m.id === filterMode)?.description}
                </p>
              </div>

              {filterMode !== 'none' && (
                <div className="space-y-3">
                  <label className="text-sm font-medium text-[var(--text-muted)]">
                    {filterMode === 'include' ? '包含关键字' : '排除关键字'}
                  </label>
                  
                  <div className="flex gap-2">
                    <input
                      type="text"
                      value={newKeyword}
                      onChange={(e) => setNewKeyword(e.target.value)}
                      onKeyDown={(e) => e.key === 'Enter' && handleAddKeyword()}
                      placeholder="输入关键字..."
                      className="flex-1 px-3 py-2 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)]"
                    />
                    <button
                      onClick={handleAddKeyword}
                      disabled={!newKeyword.trim()}
                      className="px-3 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium disabled:opacity-50 disabled:cursor-not-allowed hover:opacity-90 transition-opacity"
                    >
                      <Plus className="w-4 h-4" />
                    </button>
                  </div>

                  {currentKeywords.length > 0 && (
                    <div className="flex flex-wrap gap-2 max-h-32 overflow-auto">
                      {currentKeywords.map((keyword) => (
                        <div
                          key={keyword}
                          className="flex items-center gap-1.5 px-2.5 py-1.5 bg-[var(--bg-elevated)] rounded-lg border border-[var(--border-secondary)]"
                        >
                          <span className="text-sm text-[var(--text-primary)]">{keyword}</span>
                          <button
                            onClick={() => handleRemoveKeyword(keyword)}
                            className="w-4 h-4 flex items-center justify-center rounded hover:bg-[var(--status-error)]/20 transition-colors"
                          >
                            <X className="w-3 h-3 text-[var(--text-muted)] hover:text-[var(--status-error)]" />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}

                  {currentKeywords.length === 0 && (
                    <p className="text-xs text-[var(--text-faint)] text-center py-4">
                      暂无关键字，添加关键字以过滤节点
                    </p>
                  )}
                </div>
              )}
            </div>

            <div className="flex items-center justify-between p-5 border-t border-[var(--border-secondary)]">
              <button
                onClick={handleClear}
                className="flex items-center gap-1.5 px-3 py-2 text-sm text-[var(--status-error)] hover:bg-[var(--status-error)]/10 rounded-lg transition-colors"
              >
                <Trash2 className="w-4 h-4" />
                <span>清除过滤</span>
              </button>
              <div className="flex gap-2">
                <button
                  onClick={onClose}
                  className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={handleApply}
                  className="px-4 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium hover:opacity-90 transition-opacity"
                >
                  应用
                </button>
              </div>
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body
  )
}

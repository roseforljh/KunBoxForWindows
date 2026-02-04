import { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { motion, AnimatePresence } from 'framer-motion'
import { X, Loader2, Plus } from 'lucide-react'
import type { Profile } from '@shared/types'

type AddNodeTarget =
  | { type: 'existing'; profileId: string }
  | { type: 'new'; profileName: string }

interface AddNodeModalProps {
  isOpen: boolean
  onClose: () => void
  onAdd: (link: string, target: AddNodeTarget) => Promise<void>
  profiles: Profile[]
  currentProfileId: string | null
}

const SUPPORTED_PROTOCOLS = ['ss://', 'vmess://', 'vless://', 'trojan://', 'hysteria2://', 'hysteria://', 'tuic://']

export function AddNodeModal({ isOpen, onClose, onAdd, profiles, currentProfileId }: AddNodeModalProps) {
  const [link, setLink] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const [isCreatingNew, setIsCreatingNew] = useState(false)
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null)
  const [newProfileName, setNewProfileName] = useState('')

  useEffect(() => {
    if (isOpen) {
      setSelectedProfileId(currentProfileId || profiles[0]?.id || null)
      setIsCreatingNew(false)
      setNewProfileName('')
    }
  }, [isOpen, currentProfileId, profiles])

  const handleClose = () => {
    if (loading) return
    setLink('')
    setError('')
    setIsCreatingNew(false)
    setNewProfileName('')
    onClose()
  }

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText()
      setLink(text.trim())
    } catch {
      setError('无法读取剪贴板')
    }
  }

  const handleAdd = async () => {
    const trimmed = link.trim()
    if (!trimmed) {
      setError('请输入节点链接')
      return
    }

    const hasValidProtocol = SUPPORTED_PROTOCOLS.some(p => trimmed.startsWith(p))
    if (!hasValidProtocol) {
      setError('不支持的协议，请使用 ss://, vmess://, vless://, trojan://, hysteria2:// 等格式')
      return
    }

    if (isCreatingNew && !newProfileName.trim()) {
      setError('请输入新配置文件名称')
      return
    }

    if (!isCreatingNew && !selectedProfileId) {
      setError('请选择目标配置文件')
      return
    }

    setError('')
    setLoading(true)

    try {
      const target: AddNodeTarget = isCreatingNew
        ? { type: 'new', profileName: newProfileName.trim() }
        : { type: 'existing', profileId: selectedProfileId! }

      await onAdd(trimmed, target)
      handleClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : '添加失败')
    } finally {
      setLoading(false)
    }
  }

  const canAdd = link.trim() && (isCreatingNew ? newProfileName.trim() : selectedProfileId)

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
            className="relative w-[480px] glass-card rounded-2xl border border-[var(--glass-border)] shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-5 border-b border-[var(--border-secondary)]">
              <h3 className="text-lg font-bold text-[var(--text-primary)]">添加节点</h3>
              <button
                onClick={handleClose}
                disabled={loading}
                className="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-[var(--bg-elevated)] transition-colors disabled:opacity-50"
              >
                <X className="w-4 h-4 text-[var(--text-muted)]" />
              </button>
            </div>

            <div className="p-5 space-y-5">
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label className="text-sm font-medium text-[var(--text-muted)]">节点链接</label>
                  <button
                    onClick={handlePaste}
                    disabled={loading}
                    className="text-xs text-[var(--accent-primary)] hover:underline disabled:opacity-50"
                  >
                    粘贴
                  </button>
                </div>
                <textarea
                  value={link}
                  onChange={(e) => setLink(e.target.value)}
                  placeholder="ss://..., vmess://..., vless://..."
                  disabled={loading}
                  rows={3}
                  className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] resize-none font-mono disabled:opacity-50"
                />
                <p className="text-xs text-[var(--text-faint)]">
                  支持 Shadowsocks, VMess, VLESS, Trojan, Hysteria2 等协议
                </p>
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--text-muted)]">添加到配置文件</label>
                <div className="max-h-[180px] overflow-y-auto space-y-1.5 pr-1">
                  {profiles.map((profile) => (
                    <button
                      key={profile.id}
                      onClick={() => {
                        setIsCreatingNew(false)
                        setSelectedProfileId(profile.id)
                      }}
                      disabled={loading}
                      className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-xl transition-all ${
                        !isCreatingNew && selectedProfileId === profile.id
                          ? 'bg-[var(--accent-muted)] border border-[var(--accent-primary)]'
                          : 'bg-[var(--bg-elevated)] border border-transparent hover:border-[var(--border-secondary)]'
                      } disabled:opacity-50`}
                    >
                      <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center shrink-0 ${
                        !isCreatingNew && selectedProfileId === profile.id
                          ? 'border-[var(--accent-primary)]'
                          : 'border-[var(--text-faint)]'
                      }`}>
                        {!isCreatingNew && selectedProfileId === profile.id && (
                          <div className="w-2 h-2 rounded-full bg-[var(--accent-primary)]" />
                        )}
                      </div>
                      <div className="flex-1 text-left">
                        <p className="text-sm font-medium text-[var(--text-primary)]">{profile.name}</p>
                        <p className="text-xs text-[var(--text-faint)]">{profile.nodeCount} 个节点</p>
                      </div>
                    </button>
                  ))}

                  <button
                    onClick={() => setIsCreatingNew(true)}
                    disabled={loading}
                    className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-xl transition-all ${
                      isCreatingNew
                        ? 'bg-[var(--accent-muted)] border border-[var(--accent-primary)]'
                        : 'bg-[var(--bg-elevated)] border border-transparent hover:border-[var(--border-secondary)]'
                    } disabled:opacity-50`}
                  >
                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center shrink-0 ${
                      isCreatingNew ? 'border-[var(--accent-primary)]' : 'border-[var(--text-faint)]'
                    }`}>
                      {isCreatingNew && (
                        <div className="w-2 h-2 rounded-full bg-[var(--accent-primary)]" />
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <Plus className="w-4 h-4 text-[var(--text-muted)]" />
                      <span className="text-sm font-medium text-[var(--text-primary)]">创建新配置文件</span>
                    </div>
                  </button>
                </div>
              </div>

              <AnimatePresence>
                {isCreatingNew && (
                  <motion.div
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2 }}
                    className="overflow-hidden"
                  >
                    <div className="space-y-2">
                      <label className="text-sm font-medium text-[var(--text-muted)]">新配置文件名称</label>
                      <input
                        type="text"
                        value={newProfileName}
                        onChange={(e) => setNewProfileName(e.target.value)}
                        placeholder="输入配置文件名称..."
                        disabled={loading}
                        className="w-full px-3 py-2.5 bg-[var(--bg-elevated)] border border-[var(--border-secondary)] rounded-lg text-sm text-[var(--text-primary)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
                      />
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {error && (
                <div className="px-3 py-2 bg-[var(--status-error)]/10 border border-[var(--status-error)]/30 rounded-lg">
                  <p className="text-sm text-[var(--status-error)]">{error}</p>
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2 p-5 border-t border-[var(--border-secondary)]">
              <button
                onClick={handleClose}
                disabled={loading}
                className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
              >
                取消
              </button>
              <button
                onClick={handleAdd}
                disabled={loading || !canAdd}
                className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-primary)] text-white rounded-lg text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>{loading ? '添加中...' : '添加'}</span>
              </button>
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body
  )
}
